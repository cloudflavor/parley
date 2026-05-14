use crate::domain::ai::AiSessionMode;
use crate::domain::config::AppConfig;
use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk, DiffLine, DiffLineKind};
use crate::domain::reference::parse_file_references;
use crate::domain::review::{
    Author, CommentLineRange, DiffSide, LineComment, ReviewSession, StoredAnchorSnapshot,
};
use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use std::collections::BTreeSet;
use std::env::current_dir;
use std::path::{Path, PathBuf};
use tokio::fs;

static AI_SESSION_PROMPTS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/prompts/ai_session");
const OUTPUT_CONTRACT: &str = r#"
Output contract:
- Return only one JSON object. Do not wrap it in markdown fences.
- Required schema: {"thread_id": <id>, "reply": "<concise reply>", "status": "pending_human"}.
- `thread_id` must exactly equal the `Thread comment id` shown above.
- `reply` is the only text Parley stores as the review-thread reply.
- `status` must be exactly "pending_human". Parley applies the state change after validating the JSON.
- Reply directly and briefly, as a human code author.
- Do not mark the thread addressed yourself.
- Do not say that you marked or will mark the thread addressed.
- Do not infer target thread from file order, cursor position, latest visible thread, or latest reply.
- Do not answer, edit, summarize, or mention any other thread id.
- `reply` must not include implementation transcripts, tool output, command logs, validation logs, JSON edit logs, investigation notes, or intermediate thinking.
- `reply` must not mention skills, agents, worktrees, commits, staging, or cleanup.
- `reply` must be maximum 120 words.
- Use at most 3 short bullets inside `reply` unless a blocker requires one extra sentence.
- Do not narrate reasoning, investigation, process, or uncertainty inside `reply`.
- Do not include phrases like "I see", "I found", "Looking at this", "It looks like", "You're right", or "The issue is" inside `reply`.
- Do not include chain-of-thought, step-by-step analysis, hidden reasoning, or tool/process commentary inside `reply`.
"#;

pub(super) async fn build_thread_prompt(
    review_name: &str,
    comment_id: u64,
    review: &ReviewSession,
    diff_document: Option<&DiffDocument>,
    mode: AiSessionMode,
    task_prompt_override: Option<&str>,
) -> Result<String> {
    let Some(comment) = review
        .comments
        .iter()
        .find(|comment| comment.id == comment_id)
    else {
        return missing_comment_prompt(review_name, comment_id).await;
    };

    let mut thread = String::new();
    thread.push_str(&format!("Review: {review_name}\n"));
    thread.push_str(&format!(
        "Thread comment id: {}\nFile: {}\nLine: {}:{}\nSelected line range: {}\nStatus: {:?}\n",
        comment.id,
        comment.file_path,
        comment
            .old_line
            .map_or_else(|| "_".to_string(), |value| value.to_string()),
        comment
            .new_line
            .map_or_else(|| "_".to_string(), |value| value.to_string()),
        format_comment_line_range(comment),
        comment.status
    ));
    thread.push_str("\nOriginal comment:\n");
    thread.push_str(&comment.body);
    thread.push_str("\n\nReplies so far:\n");
    if comment.replies.is_empty() {
        thread.push_str("- (none)\n");
    } else {
        for reply in &comment.replies {
            thread.push_str(&format!("- {}: {}\n", reply.author.as_str(), reply.body));
        }
    }
    append_current_human_request(&mut thread, comment);
    append_anchor_context(&mut thread, comment, diff_document);
    append_target_file_and_diff_context(&mut thread, comment, diff_document).await;
    append_referenced_files_context(&mut thread, comment).await;

    let task_prompt = task_prompt_override
        .map(Ok)
        .unwrap_or_else(|| default_task_prompt(mode))?;
    append_task_prompt(&mut thread, task_prompt);
    Ok(thread)
}

pub(super) async fn load_task_prompt_override(
    config: &AppConfig,
    mode: AiSessionMode,
) -> Result<Option<String>> {
    let Some(path) = config.ai.prompt_path_for_mode(mode) else {
        return Ok(None);
    };
    let path = Path::new(path);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir()
            .context("failed to read current working directory for prompt path")?
            .join(path)
    };
    fs::read_to_string(&resolved)
        .await
        .with_context(|| format!("failed to read AI prompt markdown {}", resolved.display()))
        .map(Some)
}

fn default_task_prompt(mode: AiSessionMode) -> Result<&'static str> {
    match mode {
        AiSessionMode::Reply => prompt_template("reply_task.md"),
        AiSessionMode::Refactor => prompt_template("refactor_task.md"),
    }
}

fn append_task_prompt(prompt: &mut String, task_prompt: &str) {
    if !prompt.ends_with('\n') {
        prompt.push('\n');
    }
    if !task_prompt.starts_with('\n') {
        prompt.push('\n');
    }
    prompt.push_str(task_prompt);
    if !prompt.ends_with('\n') {
        prompt.push('\n');
    }
    prompt.push_str(OUTPUT_CONTRACT);
    if !prompt.ends_with('\n') {
        prompt.push('\n');
    }
}

fn append_current_human_request(prompt: &mut String, comment: &LineComment) {
    prompt.push_str("\n\nCurrent human request to address:\n");
    if let Some(reply) = comment
        .replies
        .iter()
        .rev()
        .find(|reply| matches!(reply.author, Author::User))
    {
        prompt.push_str("- latest human reply: ");
        prompt.push_str(&reply.body);
        prompt.push('\n');
    } else {
        prompt.push_str("- original comment: ");
        prompt.push_str(&comment.body);
        prompt.push('\n');
    }
    prompt.push_str(
        "Use the full thread history above only as context; answer or act on this current human request, not an earlier AI reply or another thread.\n",
    );
}

fn append_anchor_context(
    prompt: &mut String,
    comment: &LineComment,
    diff_document: Option<&DiffDocument>,
) {
    prompt.push_str("\n\nThread anchor context:\n");
    let projection = exact_current_projection(comment, diff_document);
    prompt.push_str(&format!(
        "- anchor status: {}\n",
        anchor_status_label(comment, diff_document, projection.as_ref())
    ));
    append_original_anchor_context(prompt, comment);
    append_current_projection_context(prompt, projection.as_ref(), diff_document);
}

fn anchor_status_label(
    comment: &LineComment,
    diff_document: Option<&DiffDocument>,
    projection: Option<&CurrentAnchorProjection>,
) -> &'static str {
    if comment.original_anchor.is_none() {
        return "legacy_stored_reference";
    }
    if projection.is_some() {
        return "exact_current_projection";
    }
    if diff_document.is_none() {
        return "current_projection_unavailable";
    }
    "outdated_or_detached"
}

fn append_original_anchor_context(prompt: &mut String, comment: &LineComment) {
    let Some(anchor) = comment.original_anchor.as_ref() else {
        prompt.push_str("- original anchor: unavailable (legacy review data)\n");
        prompt.push_str(&format!(
            "- stored reference: {} @ {} ({})\n",
            comment.file_path,
            format_comment_line_range(comment),
            comment.side.as_str()
        ));
        return;
    };

    prompt.push_str("- original anchor:\n");
    prompt.push_str(&format!("  file: {}\n", anchor.file_path));
    prompt.push_str(&format!("  side: {}\n", anchor.side.as_str()));
    prompt.push_str(&format!(
        "  reference: {}\n",
        format_anchor_reference(anchor)
    ));
    append_optional_text_block(prompt, "  selected text:", &anchor.selected_text, 4, None);
    append_optional_lines(prompt, "  before context:", &anchor.before_context, 4, None);
    append_optional_lines(prompt, "  after context:", &anchor.after_context, 4, None);
    if let Some(diff) = anchor.diff.as_ref() {
        prompt.push_str(&format!("  diff hunk: {}\n", diff.hunk_header));
        append_optional_lines(
            prompt,
            "  original hunk lines:",
            &diff.hunk_lines,
            4,
            Some(28),
        );
    }
    if let Some(source) = anchor.source.as_ref() {
        prompt.push_str(&format!(
            "  source hashes: file={}, selected={}\n",
            source.file_content_hash.as_deref().unwrap_or("_"),
            source.selected_text_hash.as_deref().unwrap_or("_")
        ));
    }
    if anchor.base_rev.is_some() || anchor.head_rev.is_some() {
        prompt.push_str(&format!(
            "  revisions: base={}, head={}\n",
            anchor.base_rev.as_deref().unwrap_or("_"),
            anchor.head_rev.as_deref().unwrap_or("_")
        ));
    }
}

fn append_current_projection_context(
    prompt: &mut String,
    projection: Option<&CurrentAnchorProjection>,
    diff_document: Option<&DiffDocument>,
) {
    let Some(projection) = projection else {
        let reason = if diff_document.is_some() {
            "no exact match in current diff"
        } else {
            "current diff unavailable"
        };
        prompt.push_str(&format!(
            "- current projection: none ({reason}; confidence: none)\n"
        ));
        return;
    };

    prompt.push_str("- current projection:\n");
    prompt.push_str(&format!("  file: {}\n", projection.file_path));
    prompt.push_str(&format!("  side: {}\n", projection.side.as_str()));
    prompt.push_str(&format!(
        "  reference: {}\n",
        projection.line_range.as_ref().map_or_else(
            || format_line_pair(projection.old_line, projection.new_line),
            format_comment_line_range_value
        )
    ));
    prompt.push_str("  confidence: exact\n");
}

async fn append_target_file_and_diff_context(
    prompt: &mut String,
    comment: &LineComment,
    diff_document: Option<&DiffDocument>,
) {
    prompt.push_str("\n\nPrimary target context:\n");
    let target_line = comment
        .line_range
        .as_ref()
        .and_then(|range| range.start_new_line.or(range.start_old_line))
        .or(comment.new_line)
        .or(comment.old_line);
    match target_line {
        Some(line) => {
            prompt.push_str(&format!(
                "- thread anchor: {}:{}\n",
                comment.file_path, line
            ));
            if let Some(resolved) = resolve_workspace_path(&comment.file_path) {
                let mut snippet = None;
                if let Some(range) = comment.line_range.as_ref() {
                    snippet = file_range_snippet_for_comment(&resolved, range).await;
                }
                if snippet.is_none() {
                    snippet = file_line_snippet(&resolved, line).await;
                }
                if let Some(snippet) = snippet {
                    prompt.push_str(&format!(
                        "  file snippet for selected context in {}:\n{}",
                        comment.file_path, snippet
                    ));
                } else {
                    prompt.push_str("  file snippet: unavailable for requested line\n");
                }
            } else {
                prompt.push_str("  file snippet: file not found in workspace\n");
            }
        }
        None => {
            prompt.push_str(&format!(
                "- thread anchor: {} (line unavailable)\n",
                comment.file_path
            ));
        }
    }

    if let Some(document) = diff_document {
        if let Some(file) = find_diff_file(document, &comment.file_path) {
            if let Some(hunk) = choose_best_hunk(file, comment.old_line, comment.new_line) {
                let excerpt = format_hunk_excerpt(hunk, comment.old_line, comment.new_line, 28);
                prompt.push_str("  nearest diff hunk:\n");
                prompt.push_str(&excerpt);
            } else {
                prompt.push_str("  nearest diff hunk: none for this file\n");
            }
        } else {
            prompt.push_str("  nearest diff hunk: file not present in current git diff\n");
        }
    } else {
        prompt.push_str("  nearest diff hunk: unavailable (failed to load git diff)\n");
    }
}

async fn append_referenced_files_context(prompt: &mut String, comment: &LineComment) {
    let mut ordered = BTreeSet::new();
    for reference in parse_file_references(&comment.body) {
        ordered.insert((reference.path, reference.line));
    }
    for reply in &comment.replies {
        for reference in parse_file_references(&reply.body) {
            ordered.insert((reference.path, reference.line));
        }
    }
    if ordered.is_empty() {
        return;
    }

    prompt.push_str("\n\nReferenced files from thread mentions:\n");
    let target_file_path_std = comment.file_path.to_string();
    for (path, line) in ordered.into_iter().take(8) {
        // Only fetch context from the target file. Other referenced files may contain
        // their own threads which would distract the AI from its assigned target thread.
        if path != target_file_path_std {
            continue;
        }
        let marker = if let Some(value) = line {
            format!("{path}:{value}")
        } else {
            path.clone()
        };
        prompt.push_str(&format!("- {marker}\n"));
        if let (Some(value), Some(resolved)) = (line, resolve_workspace_path(&path))
            && let Some(snippet) = file_line_snippet(&resolved, value).await
        {
            prompt.push_str(&format!("  context from {}:\n", resolved.display()));
            prompt.push_str(&snippet);
        }
    }
}

fn find_diff_file<'a>(document: &'a DiffDocument, path: &str) -> Option<&'a DiffFile> {
    document.files.iter().find(|file| file.path == path)
}

#[derive(Debug)]
struct CurrentAnchorProjection {
    file_path: String,
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line_range: Option<CommentLineRange>,
}

fn exact_current_projection(
    comment: &LineComment,
    diff_document: Option<&DiffDocument>,
) -> Option<CurrentAnchorProjection> {
    let anchor = comment.original_anchor.as_ref()?;
    let file = find_diff_file(diff_document?, &anchor.file_path)?;
    if let Some(range) = anchor.line_range.as_ref() {
        return exact_current_range_projection(file, anchor, range);
    }

    file.hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .find(|line| {
            line_matches_anchor_reference(
                line,
                anchor.side,
                anchor.old_line,
                anchor.new_line,
            ) && selected_text_matches_line(line, &anchor.selected_text)
        })
        .map(|line| CurrentAnchorProjection {
            file_path: anchor.file_path.clone(),
            side: anchor.side,
            old_line: line.old_line,
            new_line: line.new_line,
            line_range: None,
        })
}

fn exact_current_range_projection(
    file: &DiffFile,
    anchor: &StoredAnchorSnapshot,
    range: &CommentLineRange,
) -> Option<CurrentAnchorProjection> {
    let lines = file
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| {
            line_in_anchor_range(line.old_line, range.start_old_line, range.end_old_line)
                || line_in_anchor_range(line.new_line, range.start_new_line, range.end_new_line)
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let projected_text = lines
        .iter()
        .map(|line| normalize_anchor_text(&line.code))
        .collect::<Vec<_>>()
        .join("\n");
    if !anchor.selected_text.is_empty()
        && normalize_anchor_text(&anchor.selected_text) != projected_text
    {
        return None;
    }

    Some(CurrentAnchorProjection {
        file_path: anchor.file_path.clone(),
        side: anchor.side,
        old_line: anchor.old_line,
        new_line: anchor.new_line,
        line_range: Some(range.clone()),
    })
}

fn line_matches_anchor_reference(
    line: &DiffLine,
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
) -> bool {
    if !is_commentable_diff_line(line) {
        return false;
    }
    match (old_line, new_line) {
        (Some(old), Some(new)) => line.old_line == Some(old) && line.new_line == Some(new),
        (Some(old), None) => !matches!(side, DiffSide::Right) && line.old_line == Some(old),
        (None, Some(new)) => !matches!(side, DiffSide::Left) && line.new_line == Some(new),
        (None, None) => false,
    }
}

fn selected_text_matches_line(line: &DiffLine, selected_text: &str) -> bool {
    selected_text.is_empty()
        || normalize_anchor_text(selected_text) == normalize_anchor_text(&line.code)
}

fn line_in_anchor_range(line: Option<u32>, start: Option<u32>, end: Option<u32>) -> bool {
    let Some(line) = line else {
        return false;
    };
    let Some(start) = start else {
        return false;
    };
    let end = end.unwrap_or(start);
    line >= start.min(end) && line <= start.max(end)
}

fn is_commentable_diff_line(line: &DiffLine) -> bool {
    matches!(
        line.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    )
}

pub(super) fn choose_best_hunk(
    file: &DiffFile,
    old_line: Option<u32>,
    new_line: Option<u32>,
) -> Option<&DiffHunk> {
    if file.hunks.is_empty() {
        return None;
    }

    for hunk in &file.hunks {
        if hunk_contains_anchor(hunk, old_line, new_line) {
            return Some(hunk);
        }
    }

    let mut scored = file
        .hunks
        .iter()
        .map(|hunk| (hunk_distance_to_anchor(hunk, old_line, new_line), hunk))
        .collect::<Vec<_>>();
    scored.sort_by_key(|(distance, _)| *distance);
    scored.first().map(|(_, hunk)| *hunk)
}

fn hunk_contains_anchor(hunk: &DiffHunk, old_line: Option<u32>, new_line: Option<u32>) -> bool {
    hunk.lines.iter().any(|line| {
        old_line.is_some() && line.old_line == old_line
            || new_line.is_some() && line.new_line == new_line
    })
}

pub(super) fn hunk_distance_to_anchor(
    hunk: &DiffHunk,
    old_line: Option<u32>,
    new_line: Option<u32>,
) -> u32 {
    let mut best = u32::MAX;
    if let Some(target_old) = old_line {
        best = best.min(line_distance(hunk.old_start, target_old));
    }
    if let Some(target_new) = new_line {
        best = best.min(line_distance(hunk.new_start, target_new));
    }
    if best == u32::MAX { 0 } else { best }
}

fn line_distance(base: u32, target: u32) -> u32 {
    base.abs_diff(target)
}

pub(super) fn format_hunk_excerpt(
    hunk: &DiffHunk,
    old_line: Option<u32>,
    new_line: Option<u32>,
    max_lines: usize,
) -> String {
    if hunk.lines.is_empty() || max_lines == 0 {
        return String::new();
    }
    let center = hunk
        .lines
        .iter()
        .position(|line| {
            old_line.is_some() && line.old_line == old_line
                || new_line.is_some() && line.new_line == new_line
        })
        .unwrap_or(0);
    let half_window = max_lines / 2;
    let mut start = center.saturating_sub(half_window);
    let end = (start + max_lines).min(hunk.lines.len());
    if end - start < max_lines && end == hunk.lines.len() {
        start = end.saturating_sub(max_lines);
    }

    let mut out = String::new();
    for line in &hunk.lines[start..end] {
        out.push_str("    ");
        out.push_str(&line.raw);
        out.push('\n');
    }
    out
}

fn resolve_workspace_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        current_dir().ok()?.join(trimmed)
    };
    if !candidate.is_file() {
        return None;
    }
    Some(candidate)
}

async fn file_line_snippet(path: &Path, line: u32) -> Option<String> {
    if line == 0 {
        return None;
    }
    let text = fs::read_to_string(path).await.ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let target = usize::try_from(line.saturating_sub(1)).ok()?;
    if target >= lines.len() {
        return None;
    }

    let start = target.saturating_sub(2);
    let end = (target + 3).min(lines.len());
    let mut out = String::new();
    for (idx, content) in lines[start..end].iter().enumerate() {
        let absolute = start + idx + 1;
        out.push_str(&format!("    {absolute:>5} | {content}\n"));
    }
    Some(out)
}

async fn file_range_snippet_for_comment(path: &Path, range: &CommentLineRange) -> Option<String> {
    let start = range.start_new_line.or(range.start_old_line)?;
    let end = range.end_new_line.or(range.end_old_line).unwrap_or(start);
    file_range_snippet(path, start.min(end), start.max(end)).await
}

async fn file_range_snippet(path: &Path, start_line: u32, end_line: u32) -> Option<String> {
    if start_line == 0 || end_line == 0 {
        return None;
    }
    let text = fs::read_to_string(path).await.ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let start = usize::try_from(start_line.saturating_sub(1)).ok()?;
    let end = usize::try_from(end_line).ok()?;
    if start >= lines.len() || start >= end {
        return None;
    }

    let context_start = start.saturating_sub(2);
    let context_end = (end + 2).min(lines.len());
    let mut out = String::new();
    for (idx, content) in lines[context_start..context_end].iter().enumerate() {
        let absolute = context_start + idx + 1;
        let marker = if absolute >= usize::try_from(start_line).ok()?
            && absolute <= usize::try_from(end_line).ok()?
        {
            ">"
        } else {
            " "
        };
        out.push_str(&format!("  {marker} {absolute:>5} | {content}\n"));
    }
    Some(out)
}

fn format_comment_line_range(comment: &LineComment) -> String {
    comment.line_range.as_ref().map_or_else(
        || format_line_pair(comment.old_line, comment.new_line),
        format_comment_line_range_value,
    )
}

fn format_anchor_reference(anchor: &StoredAnchorSnapshot) -> String {
    anchor.line_range.as_ref().map_or_else(
        || format_line_pair(anchor.old_line, anchor.new_line),
        format_comment_line_range_value,
    )
}

fn format_comment_line_range_value(range: &CommentLineRange) -> String {
    format!(
        "{}:{}",
        format_optional_line_range(range.start_old_line, range.end_old_line),
        format_optional_line_range(range.start_new_line, range.end_new_line)
    )
}

fn format_line_pair(old_line: Option<u32>, new_line: Option<u32>) -> String {
    format!(
        "{}:{}",
        old_line.map_or_else(|| "_".to_string(), |value| value.to_string()),
        new_line.map_or_else(|| "_".to_string(), |value| value.to_string())
    )
}

fn format_optional_line_range(start: Option<u32>, end: Option<u32>) -> String {
    match (start, end) {
        (Some(start), Some(end)) if start != end => format!("{start}-{end}"),
        (Some(start), _) => start.to_string(),
        (None, Some(end)) => end.to_string(),
        (None, None) => "_".to_string(),
    }
}

fn append_optional_text_block(
    prompt: &mut String,
    label: &str,
    value: &str,
    indent: usize,
    max_lines: Option<usize>,
) {
    if value.is_empty() {
        return;
    }
    prompt.push_str(label);
    prompt.push('\n');
    append_indented_lines(prompt, value.lines(), indent, max_lines);
}

fn append_optional_lines(
    prompt: &mut String,
    label: &str,
    values: &[String],
    indent: usize,
    max_lines: Option<usize>,
) {
    if values.is_empty() {
        return;
    }
    prompt.push_str(label);
    prompt.push('\n');
    append_indented_lines(prompt, values.iter().map(String::as_str), indent, max_lines);
}

fn append_indented_lines<'a>(
    prompt: &mut String,
    values: impl Iterator<Item = &'a str>,
    indent: usize,
    max_lines: Option<usize>,
) {
    let prefix = " ".repeat(indent);
    for (index, value) in values.enumerate() {
        if max_lines.is_some_and(|limit| index >= limit) {
            prompt.push_str(&prefix);
            prompt.push_str("...\n");
            return;
        }
        prompt.push_str(&prefix);
        prompt.push_str(value);
        prompt.push('\n');
    }
}

fn normalize_anchor_text(value: &str) -> String {
    value.trim().to_string()
}

fn prompt_template(path: &str) -> Result<&'static str> {
    let file = AI_SESSION_PROMPTS_DIR
        .get_file(path)
        .ok_or_else(|| anyhow::anyhow!("missing ai session prompt template: {path}"))?;
    let contents = file
        .contents_utf8()
        .ok_or_else(|| anyhow::anyhow!("invalid utf-8 in ai session prompt template: {path}"))?;
    Ok(contents)
}

async fn missing_comment_prompt(review_name: &str, comment_id: u64) -> Result<String> {
    let template = prompt_template("comment_not_found.md")?;
    Ok(template
        .replace("{review_name}", review_name)
        .replace("{comment_id}", &comment_id.to_string()))
}
