use super::{AppConfig, DiffDocument, TuiApp, TuiAppInit, render};
use crate::domain::diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
use crate::domain::review::{
    Author, CommentStatus, DiffSide, LineAnchorSnapshot, LineComment, ReviewSession, ReviewState,
};
use crate::git::diff::DiffSource;
use crate::persistence::store::Store;
use crate::services::review_service::ReviewService;
use crate::tui::theme::{default_theme_name, load_themes, resolve_theme_index};
use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PERF_DRAW_FILES: usize = 120;
const PERF_DRAW_LINES_PER_FILE: usize = 160;
const PERF_DRAW_COMMENTS: usize = 800;
const PERF_LARGE_FILE_LINES: usize = 12_000;
const PERF_COMMENT_LOOKUP_FILES: usize = 1_000;
const PERF_COMMENT_LOOKUP_COMMENTS: usize = 40_000;
const PERF_STATUS_COMMENTS: usize = 10_000;
const PERF_DRAW_LARGE_REVIEW_MAX_MS: f64 = 50.0;
const PERF_REBUILD_ROW_CACHE_MAX_MS: f64 = 20.0;
const PERF_VISIBLE_FILE_INDICES_MAX_MS: f64 = 20.0;
const PERF_COMMENTS_FOR_FILE_TOTAL_MAX_MS: f64 = 20.0;
const PERF_MARK_STATUS_MAX_MS: f64 = if cfg!(debug_assertions) { 250.0 } else { 100.0 };

#[test]
fn rebuild_row_cache_should_defer_syntax_highlighting() -> Result<()> {
    let file = make_diff_file("src/lazy.rs", 12);
    let mut app = make_perf_app_with_files(vec![file], Vec::new())?;

    app.rebuild_row_cache_for_file(0);
    let cached = app
        .row_cache
        .get(&0)
        .expect("row cache should be populated");
    assert!(cached.highlights.iter().all(Option::is_none));

    let colors = app.theme().colors.clone();
    let mut painter = app
        .syntax_painter_for_file(0, &colors)
        .expect("syntax painter should be available");
    let highlighted =
        app.highlighted_segments_for_file_row_with_painter(0, 3, &mut painter, &colors);
    assert!(!highlighted.is_empty());
    let cached = app
        .row_cache
        .get(&0)
        .expect("row cache should remain populated");
    assert_eq!(
        cached
            .highlights
            .iter()
            .filter(|parts| parts.is_some())
            .count(),
        1
    );

    Ok(())
}

#[test]
fn perf_tui_draw_large_review() -> Result<()> {
    let mut app = make_perf_app(
        PERF_DRAW_FILES,
        PERF_DRAW_LINES_PER_FILE,
        PERF_DRAW_COMMENTS,
    )?;
    app.ensure_row_cache();
    let backend = TestBackend::new(180, 60);
    let mut terminal = Terminal::new(backend)?;
    warm_highlights_for_selected_file(&mut app);
    app.clear_diff_render_cache();

    let elapsed = measure(40, || {
        terminal
            .draw(|frame| render::draw(frame, black_box(&mut app)))
            .map(|_| ())?;
        Ok(())
    })?;

    assert_perf_under(
        "tui_draw_large_review",
        40,
        elapsed,
        PERF_DRAW_LARGE_REVIEW_MAX_MS,
    );
    Ok(())
}

#[test]
fn perf_rebuild_row_cache_large_file() -> Result<()> {
    let file = make_diff_file("src/large.rs", PERF_LARGE_FILE_LINES);
    let mut app = make_perf_app_with_files(vec![file], Vec::new())?;

    let elapsed = measure(20, || {
        app.row_cache.clear();
        app.rebuild_row_cache_for_file(0);
        black_box(app.row_cache.get(&0).map(|cache| cache.rows.len()));
        Ok(())
    })?;

    assert_perf_under(
        "rebuild_row_cache_large_file",
        20,
        elapsed,
        PERF_REBUILD_ROW_CACHE_MAX_MS,
    );
    Ok(())
}

#[test]
fn perf_visible_file_indices_many_files_and_comments() -> Result<()> {
    let mut app = make_perf_app(2_000, 8, 4_000)?;

    let elapsed = measure(200, || {
        black_box(app.visible_file_indices());
        Ok(())
    })?;

    assert_perf_under(
        "visible_file_indices_many_files_and_comments",
        200,
        elapsed,
        PERF_VISIBLE_FILE_INDICES_MAX_MS,
    );
    Ok(())
}

#[test]
fn perf_comments_for_file_many_comments() -> Result<()> {
    let app = make_perf_app(PERF_COMMENT_LOOKUP_FILES, 1, PERF_COMMENT_LOOKUP_COMMENTS)?;
    let target_file = format!("src/module_{:04}.rs", PERF_COMMENT_LOOKUP_FILES - 1);

    let elapsed = measure(2_000, || {
        black_box(app.comments_for_file(&target_file).len());
        Ok(())
    })?;

    assert_total_perf_under(
        "comments_for_file_many_comments",
        2_000,
        elapsed,
        PERF_COMMENTS_FOR_FILE_TOTAL_MAX_MS,
    );
    Ok(())
}

#[tokio::test]
async fn perf_mark_selected_comment_status_many_comments() -> Result<()> {
    let tempdir = tempfile::tempdir()?;
    let service = ReviewService::new(Store::from_project_root(tempdir.path()));
    let mut app = make_perf_app(50, 4, PERF_STATUS_COMMENTS)?;
    app.selected_file = 0;
    app.selected_comment = 0;
    service.save_review(&app.review).await?;

    let started_at = Instant::now();
    for _ in 0..20 {
        app.review.comments[0].status = CommentStatus::Pending;
        app.rebuild_comment_index();
        app.mark_selected_comment_status(&service, CommentStatus::Addressed, false)
            .await?;
        black_box(app.review.comments[0].status.clone());
    }
    let elapsed = started_at.elapsed();

    assert_perf_under(
        "mark_selected_comment_status_many_comments",
        20,
        elapsed,
        PERF_MARK_STATUS_MAX_MS,
    );
    Ok(())
}

fn measure(iterations: usize, mut run: impl FnMut() -> Result<()>) -> Result<Duration> {
    let started_at = Instant::now();
    for _ in 0..iterations {
        run()?;
    }
    Ok(started_at.elapsed())
}

fn assert_perf_under(name: &str, iterations: usize, elapsed: Duration, max_ms: f64) {
    let per_iter = elapsed.as_secs_f64() * 1_000.0 / iterations as f64;
    eprintln!(
        "PERF {name}: {iterations} iteration(s), total={elapsed:?}, per_iter={per_iter:.3}ms"
    );
    assert!(
        per_iter <= max_ms,
        "{name} exceeded threshold: {per_iter:.3}ms > {max_ms:.3}ms"
    );
}

fn assert_total_perf_under(name: &str, iterations: usize, elapsed: Duration, max_total_ms: f64) {
    let total_ms = elapsed.as_secs_f64() * 1_000.0;
    let per_iter = total_ms / iterations as f64;
    eprintln!(
        "PERF {name}: {iterations} iteration(s), total={elapsed:?}, per_iter={per_iter:.3}ms"
    );
    assert!(
        total_ms <= max_total_ms,
        "{name} exceeded threshold: total {total_ms:.3}ms > {max_total_ms:.3}ms"
    );
}

fn warm_highlights_for_selected_file(app: &mut TuiApp) {
    let file_index = app.active_file_index();
    let colors = app.theme().colors.clone();
    let Some(mut painter) = app.syntax_painter_for_file(file_index, &colors) else {
        return;
    };
    let Some(row_count) = app.row_count_for_file(file_index) else {
        return;
    };
    for row_index in 0..row_count {
        black_box(app.highlighted_segments_for_file_row_with_painter(
            file_index,
            row_index,
            &mut painter,
            &colors,
        ));
    }
}

fn make_perf_app(file_count: usize, lines_per_file: usize, comment_count: usize) -> Result<TuiApp> {
    let files = (0..file_count)
        .map(|index| make_diff_file(&format!("src/module_{index:04}.rs"), lines_per_file))
        .collect::<Vec<_>>();
    let comments = make_comments(file_count, lines_per_file, comment_count);
    make_perf_app_with_files(files, comments)
}

fn make_perf_app_with_files(files: Vec<DiffFile>, comments: Vec<LineComment>) -> Result<TuiApp> {
    let review = ReviewSession {
        name: "perf".to_string(),
        state: ReviewState::Open,
        created_at_ms: 0,
        updated_at_ms: 0,
        comments,
        next_comment_id: 1_000_000,
        next_reply_id: 1,
    };
    let themes = load_themes()?;
    let theme_index = resolve_theme_index(&themes, default_theme_name()).unwrap_or(0);

    Ok(TuiApp::new(TuiAppInit {
        review_name: "perf".to_string(),
        review,
        diff: DiffDocument { files },
        diff_source: DiffSource::WorkingTree,
        config: AppConfig::default(),
        themes,
        theme_index,
        log_path: "perf.log".into(),
        worktree_path: PathBuf::from("."),
    }))
}

fn make_diff_file(path: &str, lines: usize) -> DiffFile {
    let diff_lines = (1..=lines)
        .map(|line| {
            let line_number = u32::try_from(line).unwrap_or(u32::MAX);
            let code = format!("pub fn function_{line:05}() -> usize {{ {line} }}");
            DiffLine {
                kind: DiffLineKind::Context,
                old_line: Some(line_number),
                new_line: Some(line_number),
                raw: format!(" {code}"),
                code,
            }
        })
        .collect::<Vec<_>>();

    DiffFile {
        path: path.to_string(),
        header_lines: vec![
            format!("diff --git a/{path} b/{path}"),
            format!("--- a/{path}"),
            format!("+++ b/{path}"),
        ],
        hunks: vec![DiffHunk {
            old_start: 1,
            old_count: u32::try_from(lines).unwrap_or(u32::MAX),
            new_start: 1,
            new_count: u32::try_from(lines).unwrap_or(u32::MAX),
            header: format!("@@ -1,{lines} +1,{lines} @@"),
            lines: diff_lines,
        }],
    }
}

fn make_comments(
    file_count: usize,
    lines_per_file: usize,
    comment_count: usize,
) -> Vec<LineComment> {
    (0..comment_count)
        .map(|index| {
            let file_index = index % file_count.max(1);
            let line = (index % lines_per_file.max(1)) + 1;
            let line_number = u32::try_from(line).unwrap_or(u32::MAX);
            LineComment {
                id: u64::try_from(index + 1).unwrap_or(u64::MAX),
                file_path: format!("src/module_{file_index:04}.rs"),
                old_line: Some(line_number),
                new_line: Some(line_number),
                line_range: None,
                side: DiffSide::Right,
                line_anchor: Some(LineAnchorSnapshot {
                    target_code: format!("pub fn function_{line:05}() -> usize {{ {line} }}"),
                    before_context: Vec::new(),
                    after_context: Vec::new(),
                }),
                original_anchor: None,
                detached: false,
                body: format!("Perf comment {index}: this is a long enough body to wrap in the TUI and exercise thread rendering paths."),
                author: Author::User,
                status: CommentStatus::Open,
                replies: Vec::new(),
                created_at_ms: 0,
                updated_at_ms: 0,
                addressed_at_ms: None,
            }
        })
        .collect()
}
