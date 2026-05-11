use std::collections::BTreeSet;
use std::env::current_dir;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use tokio::fs;

use crate::domain::ai::AiSessionMode;
use crate::domain::config::AppConfig;
use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk};
use crate::domain::reference::parse_file_references;
use crate::domain::review::{Author, LineComment, ReviewSession};

static AI_SESSION_PROMPTS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/prompts/ai_session");
const OUTPUT_CONTRACT: &str = r#"
Output contract:
- Your entire final output is stored verbatim as the review-thread reply.
- The final output must contain only the reply body that should appear in the thread.
- Reply directly and briefly, as a human code author.
- Always produce a final review-thread reply; Parley stores that reply and moves the thread to pending_human.
- Do not mark the thread addressed yourself.
- Do not say that you marked or will mark the thread addressed.
- The only target is the exact `Thread comment id` shown above.
- Reply only to that thread id. Do not infer target thread from file order, cursor position, latest visible thread, or latest reply.
- Do not answer, edit, summarize, or mention any other thread id.
- Do not include implementation transcripts, tool output, command logs, validation logs, JSON edit logs, investigation notes, or intermediate thinking.
- Do not mention skills, agents, worktrees, commits, staging, or cleanup.
- Maximum 120 words.
- Use at most 3 short bullets unless a blocker requires one extra sentence.
- Do not narrate reasoning, investigation, process, or uncertainty.
- Do not include phrases like "I see", "I found", "Looking at this", "It looks like", "You're right", or "The issue is".
- Do not include chain-of-thought, step-by-step analysis, hidden reasoning, or tool/process commentary.
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
            let author = match reply.author {
                Author::User => "user",
                Author::Ai => "ai",
            };
            thread.push_str(&format!("- {}: {}\n", author, reply.body));
        }
    }
    append_current_human_request(&mut thread, comment);
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

async fn file_range_snippet_for_comment(
    path: &Path,
    range: &crate::domain::review::CommentLineRange,
) -> Option<String> {
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
        |range| {
            format!(
                "{}:{}",
                format_optional_line_range(range.start_old_line, range.end_old_line),
                format_optional_line_range(range.start_new_line, range.end_new_line)
            )
        },
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
