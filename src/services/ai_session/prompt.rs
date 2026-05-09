use std::collections::BTreeSet;
use std::env::current_dir;
use std::fs::read_to_string;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

use crate::domain::ai::AiSessionMode;
use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk};
use crate::domain::reference::parse_file_references;
use crate::domain::review::{Author, LineComment, ReviewSession};

static AI_SESSION_PROMPTS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/prompts/ai_session");

pub(super) fn build_thread_prompt(
    review_name: &str,
    comment_id: u64,
    review: &ReviewSession,
    diff_document: Option<&DiffDocument>,
    mode: AiSessionMode,
) -> String {
    let Some(comment) = review
        .comments
        .iter()
        .find(|comment| comment.id == comment_id)
    else {
        return missing_comment_prompt(review_name, comment_id);
    };

    let mut thread = String::new();
    thread.push_str(&format!("Review: {review_name}\n"));
    thread.push_str(&format!(
        "Thread comment id: {}\nFile: {}\nLine: {}:{}\nStatus: {:?}\n",
        comment.id,
        comment.file_path,
        comment
            .old_line
            .map_or_else(|| "_".to_string(), |value| value.to_string()),
        comment
            .new_line
            .map_or_else(|| "_".to_string(), |value| value.to_string()),
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
    append_target_file_and_diff_context(&mut thread, comment, diff_document);
    append_referenced_files_context(&mut thread, comment);

    match mode {
        AiSessionMode::Reply => {
            thread.push_str(prompt_template("reply_task.md"));
        }
        AiSessionMode::Refactor => {
            thread.push_str(prompt_template("refactor_task.md"));
        }
    }
    thread
}

fn append_target_file_and_diff_context(
    prompt: &mut String,
    comment: &LineComment,
    diff_document: Option<&DiffDocument>,
) {
    prompt.push_str("\n\nPrimary target context:\n");
    let target_line = comment.new_line.or(comment.old_line);
    match target_line {
        Some(line) => {
            prompt.push_str(&format!(
                "- thread anchor: {}:{}\n",
                comment.file_path, line
            ));
            if let Some(resolved) = resolve_workspace_path(&comment.file_path) {
                if let Some(snippet) = file_line_snippet(&resolved, line) {
                    prompt.push_str(&format!(
                        "  file snippet around {}:{}:\n{}",
                        comment.file_path, line, snippet
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

fn append_referenced_files_context(prompt: &mut String, comment: &LineComment) {
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
    for (path, line) in ordered.into_iter().take(8) {
        let marker = if let Some(value) = line {
            format!("{path}:{value}")
        } else {
            path.clone()
        };
        prompt.push_str(&format!("- {marker}\n"));
        if let (Some(value), Some(resolved)) = (line, resolve_workspace_path(&path))
            && let Some(snippet) = file_line_snippet(&resolved, value)
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

fn file_line_snippet(path: &Path, line: u32) -> Option<String> {
    if line == 0 {
        return None;
    }
    let text = read_to_string(path).ok()?;
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

fn prompt_template(path: &str) -> &'static str {
    AI_SESSION_PROMPTS_DIR
        .get_file(path)
        .unwrap_or_else(|| panic!("missing ai session prompt template: {path}"))
        .contents_utf8()
        .unwrap_or_else(|| panic!("invalid utf-8 in ai session prompt template: {path}"))
}

fn missing_comment_prompt(review_name: &str, comment_id: u64) -> String {
    prompt_template("comment_not_found.md")
        .replace("{review_name}", review_name)
        .replace("{comment_id}", &comment_id.to_string())
}
