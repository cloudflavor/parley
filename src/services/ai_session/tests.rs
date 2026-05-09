use super::comment_is_targetable;
use super::prompt::{choose_best_hunk, format_hunk_excerpt, hunk_distance_to_anchor};
use super::provider::{
    detect_model_from_json_stream, detect_model_from_text, format_ai_reply_body,
};
use crate::domain::ai::AiSessionMode;
use crate::domain::diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
use crate::domain::review::CommentStatus;
use anyhow::{Result, anyhow};

#[test]
fn reply_mode_excludes_addressed_threads() {
    assert!(comment_is_targetable(
        CommentStatus::Open,
        AiSessionMode::Reply
    ));
    assert!(comment_is_targetable(
        CommentStatus::Pending,
        AiSessionMode::Reply
    ));
    assert!(!comment_is_targetable(
        CommentStatus::Addressed,
        AiSessionMode::Reply
    ));
}

#[test]
fn refactor_mode_targets_only_open_threads() {
    assert!(comment_is_targetable(
        CommentStatus::Open,
        AiSessionMode::Refactor
    ));
    assert!(!comment_is_targetable(
        CommentStatus::Pending,
        AiSessionMode::Refactor
    ));
    assert!(!comment_is_targetable(
        CommentStatus::Addressed,
        AiSessionMode::Refactor
    ));
}

#[test]
fn choose_best_hunk_prefers_exact_anchor_match() -> Result<()> {
    let file = DiffFile {
        path: "src/lib.rs".to_string(),
        header_lines: Vec::new(),
        hunks: vec![
            make_hunk(
                "@@ -1,3 +1,3 @@",
                1,
                1,
                vec![line_ctx(1, 1), line_ctx(2, 2)],
            ),
            make_hunk(
                "@@ -40,3 +40,3 @@",
                40,
                40,
                vec![line_ctx(40, 40), line_ctx(41, 41)],
            ),
        ],
    };

    let chosen = choose_best_hunk(&file, None, Some(41))
        .ok_or_else(|| anyhow!("hunk should be selected"))?;
    assert_eq!(chosen.new_start, 40);
    Ok(())
}

#[test]
fn choose_best_hunk_falls_back_to_nearest_start() -> Result<()> {
    let file = DiffFile {
        path: "src/lib.rs".to_string(),
        header_lines: Vec::new(),
        hunks: vec![
            make_hunk("@@ -10,2 +10,2 @@", 10, 10, vec![line_ctx(10, 10)]),
            make_hunk("@@ -80,2 +80,2 @@", 80, 80, vec![line_ctx(80, 80)]),
        ],
    };

    let chosen = choose_best_hunk(&file, None, Some(74))
        .ok_or_else(|| anyhow!("hunk should be selected"))?;
    assert_eq!(chosen.new_start, 80);
    assert!(hunk_distance_to_anchor(chosen, None, Some(74)) < 10);
    Ok(())
}

#[test]
fn hunk_excerpt_contains_anchor_line() {
    let hunk = make_hunk(
        "@@ -20,4 +20,4 @@",
        20,
        20,
        vec![
            line_ctx(20, 20),
            line_add(0, 21, "+let value = 1;"),
            line_ctx(22, 22),
        ],
    );
    let excerpt = format_hunk_excerpt(&hunk, None, Some(21), 8);
    assert!(excerpt.contains("+let value = 1;"));
    assert!(excerpt.contains("@@ -20,4 +20,4 @@"));
}

#[test]
fn ai_reply_body_includes_model_header() {
    let body = format_ai_reply_body(Some("gpt-5.4"), "Implemented fix.");
    assert!(body.starts_with("Model: gpt-5.4"));
    assert!(body.contains("Implemented fix."));
}

#[test]
fn ai_reply_body_omits_header_when_model_unknown() {
    let body = format_ai_reply_body(None, "Implemented fix.");
    assert_eq!(body, "Implemented fix.");
}

#[test]
fn detect_model_from_json_stream_reads_nested_model_slug() -> Result<()> {
    let stream = r#"{"event":"meta","payload":{"session":{"model_slug":"gpt-5.4"}}}"#;
    let detected =
        detect_model_from_json_stream(stream).ok_or_else(|| anyhow!("model should be detected"))?;
    assert_eq!(detected, "gpt-5.4");
    Ok(())
}

#[test]
fn detect_model_from_text_reads_model_marker() -> Result<()> {
    let detected = detect_model_from_text("run complete; model=gpt-5.4; tokens=100")
        .ok_or_else(|| anyhow!("model should be detected"))?;
    assert_eq!(detected, "gpt-5.4");
    Ok(())
}

fn make_hunk(header: &str, old_start: u32, new_start: u32, mut extra: Vec<DiffLine>) -> DiffHunk {
    let mut lines = vec![DiffLine {
        kind: DiffLineKind::HunkHeader,
        old_line: None,
        new_line: None,
        raw: header.to_string(),
        code: header.to_string(),
    }];
    lines.append(&mut extra);
    DiffHunk {
        old_start,
        old_count: 1,
        new_start,
        new_count: 1,
        header: header.to_string(),
        lines,
    }
}

fn line_ctx(old: u32, new: u32) -> DiffLine {
    DiffLine {
        kind: DiffLineKind::Context,
        old_line: Some(old),
        new_line: Some(new),
        raw: format!(" context {old}:{new}"),
        code: format!("context {old}:{new}"),
    }
}

fn line_add(old: u32, new: u32, raw: &str) -> DiffLine {
    DiffLine {
        kind: DiffLineKind::Added,
        old_line: if old == 0 { None } else { Some(old) },
        new_line: Some(new),
        raw: raw.to_string(),
        code: raw.trim_start_matches('+').to_string(),
    }
}
