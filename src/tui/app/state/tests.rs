use crate::domain::diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
use crate::tui::app::*;

#[allow(dead_code)]
pub(crate) fn make_test_app(paths: Vec<&str>, comments: Vec<LineComment>) -> Result<TuiApp> {
    make_test_app_with_files_and_comments(
        paths
            .iter()
            .map(|p| diff_file_with_context_lines(p, &[]))
            .collect(),
        comments,
    )
}

pub(crate) fn make_test_app_with_files_and_comments(
    files: Vec<DiffFile>,
    comments: Vec<LineComment>,
) -> Result<TuiApp> {
    let review = ReviewSession {
        name: "test".to_string(),
        created_at_ms: 0,
        updated_at_ms: 0,
        state: ReviewState::Open,
        comments,
        next_comment_id: 100,
        next_reply_id: 1,
    };
    let diff = DiffDocument { files };
    let config = AppConfig::default();
    let themes = load_themes()?;
    let theme_index = resolve_theme_index(&themes, default_theme_name()).unwrap_or(0);

    Ok(TuiApp::new(TuiAppInit {
        review_name: "test".to_string(),
        review,
        diff,
        diff_source: DiffSource::WorkingTree,
        config,
        themes,
        theme_index,
        log_path: PathBuf::from("/tmp/test.log"),
        worktree_path: PathBuf::from("."),
    }))
}

pub(crate) fn diff_file_with_context_lines(path: &str, lines: &[(u32, &str)]) -> DiffFile {
    let mut hunk_lines = Vec::new();
    for (line_num, content) in lines {
        hunk_lines.push(DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(*line_num),
            new_line: Some(*line_num),
            raw: format!(" {content}"),
            code: content.to_string(),
        });
    }

    DiffFile {
        path: path.to_string(),
        header_lines: vec![
            format!("diff --git a/{path} b/{path}"),
            format!("--- a/{path}"),
            format!("+++ b/{path}"),
        ],
        hunks: vec![DiffHunk {
            header: "@@ -1,3 +1,3 @@".to_string(),
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            lines: hunk_lines,
        }],
    }
}

pub(crate) fn make_comment_with_anchor(
    id: u64,
    file_path: &str,
    status: CommentStatus,
    old_line: u32,
    new_line: u32,
) -> LineComment {
    LineComment {
        id,
        file_path: file_path.to_string(),
        old_line: Some(old_line),
        new_line: Some(new_line),
        line_range: None,
        side: DiffSide::Right,
        line_anchor: Some(LineAnchorSnapshot {
            target_code: "test".to_string(),
            before_context: vec![],
            after_context: vec![],
        }),
        original_anchor: None,
        detached: false,
        body: "test comment".to_string(),
        status,
        author: Author::User,
        created_at_ms: 0,
        updated_at_ms: 0,
        addressed_at_ms: None,
        replies: Vec::new(),
    }
}

pub(crate) fn cache_key(file_index: usize) -> DiffRenderCacheKey {
    DiffRenderCacheKey {
        file_index,
        pane_inner_width: 80,
        side_by_side_diff: false,
        search_query: None,
        selected_line: 0,
        selected_row_range: None,
        selected_comment_id: None,
        expanded_thread_ids: vec![],
        collapsed_thread_ids: vec![],
        review_state_code: 0,
        is_active: true,
    }
}

pub(crate) fn cache_entry() -> DiffRenderCacheEntry {
    DiffRenderCacheEntry::new(Vec::new(), Vec::new(), Vec::new())
}
