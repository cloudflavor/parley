use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect, style::Style, text::Line};
use tokio::task::JoinHandle;

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::{AppConfig, default_user_name};
use crate::domain::diff::{DiffDocument, DiffFile, DiffLineKind};
use crate::domain::review::{
    Author, CommentStatus, DiffSide, LineComment, ReviewSession, ReviewState,
};
use crate::git::diff::load_git_diff_head;
use crate::services::ai_session::{
    AiProgressEvent, RunAiSessionInput, run_ai_session_with_progress,
};
use crate::services::review_service::ReviewService;

use super::syntax::SyntaxPainter;
use super::terminal::TerminalSession;
use super::theme::{UiTheme, default_theme_name, load_themes, resolve_theme_index};

pub async fn run_tui(
    service: ReviewService,
    review_name: String,
    requested_theme: Option<String>,
    no_mouse: bool,
) -> Result<()> {
    let mut terminal_session = TerminalSession::new(!no_mouse)?;
    let review = service
        .load_or_create_review(&review_name)
        .await
        .with_context(|| format!("failed to open review {review_name}"))?;
    let themes = load_themes()?;
    let mut config = service.load_config().await?;
    let diff = load_git_diff_head(&config).await?;

    if config.user_name.trim().is_empty() || config.user_name == "User" {
        config.user_name = default_user_name();
    }

    if let Some(requested) = requested_theme {
        config.theme = requested;
        service.save_config(&config).await?;
    }

    let theme_index = resolve_theme_index(&themes, &config.theme)
        .unwrap_or_else(|| resolve_theme_index(&themes, default_theme_name()).unwrap_or(0));
    config.theme = themes[theme_index].name.clone();
    service.save_config(&config).await?;
    let log_path = service.review_log_path(&review_name)?;
    super::logging::init_file_tracing(&log_path, &config.log_level)
        .context("failed to initialize tui log writer")?;

    let mut app = TuiApp::new(
        review_name,
        review,
        diff,
        config,
        themes,
        theme_index,
        log_path,
    );
    let mouse_capture_enabled = terminal_session.mouse_capture_enabled();
    run_loop(
        terminal_session.terminal_mut(),
        mouse_capture_enabled,
        &mut app,
        &service,
    )
    .await
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mouse_capture_enabled: bool,
    app: &mut TuiApp,
    service: &ReviewService,
) -> Result<()> {
    const MAX_EVENTS_PER_TICK: usize = 128;
    const ACTIVE_REDRAW_INTERVAL: Duration = Duration::from_millis(120);
    const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(60);
    const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(400);
    let mut last_draw_at = Instant::now()
        .checked_sub(ACTIVE_REDRAW_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut force_draw = true;
    while !app.should_quit {
        let periodic = app.requires_periodic_redraw();
        let poll_timeout = if force_draw {
            Duration::from_millis(0)
        } else if periodic {
            ACTIVE_POLL_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        };

        if event::poll(poll_timeout).context("event poll failed")? {
            let mut processed = 0usize;
            loop {
                match event::read().context("event read failed")? {
                    Event::Key(key) => {
                        app.handle_key(key, service).await?;
                        app.invalidate_redraw();
                    }
                    Event::Mouse(mouse) => {
                        app.handle_mouse(mouse)?;
                        app.invalidate_redraw();
                    }
                    Event::Resize(_, _) => {
                        app.invalidate_redraw();
                    }
                    _ => {}
                }
                processed += 1;

                if processed >= MAX_EVENTS_PER_TICK
                    || !event::poll(Duration::from_millis(0)).context("event poll failed")?
                    || app.should_quit
                {
                    break;
                }
            }
        }

        let z_sequence_changed = app.flush_pending_key_sequences();
        if z_sequence_changed {
            app.invalidate_redraw();
        }
        let ai_changed = app.poll_ai_task(service).await?;
        if ai_changed {
            app.invalidate_redraw();
        }

        if let Some(action) = app.pending_action.take() {
            match action {
                PendingUiAction::OpenLogsInLess => {
                    match open_log_in_less(terminal, &app.log_path, mouse_capture_enabled) {
                        Ok(()) => {
                            app.status_line =
                                format!("opened logs in less: {}", app.log_path.display());
                        }
                        Err(error) => {
                            app.status_line = format!("open logs failed: {error}");
                        }
                    }
                }
            }
            app.invalidate_redraw();
        }

        let animation_due =
            app.requires_periodic_redraw() && last_draw_at.elapsed() >= ACTIVE_REDRAW_INTERVAL;
        if animation_due {
            app.invalidate_redraw();
        }

        if force_draw || app.take_redraw_invalidation() {
            terminal.draw(|frame| draw(frame, app))?;
            last_draw_at = Instant::now();
            force_draw = false;
        }
    }

    Ok(())
}

mod help_docs;
mod helpers;
mod input;
mod render;
mod state;

use helpers::{
    MOUSE_WHEEL_FILE_SCROLL_FILES, MOUSE_WHEEL_SCROLL_LINES, comment_matches_display_row,
    format_line_reference, format_timestamp_utc, insert_char_at, open_log_in_less, point_in_rect,
    remove_char_at, slice_chars,
};
use render::draw;

const AI_PROGRESS_MAX_LINES: usize = 300;
const DIFF_RENDER_CACHE_MAX_ENTRIES: usize = 64;
const INLINE_FILE_MENTION_MAX_CANDIDATES: usize = 120;
const INLINE_FILE_MENTION_MAX_VISIBLE_ROWS: usize = 6;
type HighlightParts = Vec<(Style, String)>;

#[derive(Debug, Clone)]
struct DisplayRow {
    kind: DiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    raw: String,
    code: String,
}

#[derive(Debug, Clone)]
struct CachedFileRows {
    rows: Vec<DisplayRow>,
    highlights: Vec<HighlightParts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DiffRenderCacheKey {
    file_index: usize,
    pane_inner_width: usize,
    side_by_side_diff: bool,
    search_query: Option<String>,
    thread_density_mode: ThreadDensityMode,
    selected_line: usize,
    selected_comment_id: Option<u64>,
    expanded_thread_ids: Vec<u64>,
    review_state_code: u8,
    is_active: bool,
}

#[derive(Debug, Clone)]
struct DiffRenderCacheEntry {
    lines: Vec<Line<'static>>,
    row_map: Vec<usize>,
    link_hits: Vec<FileReferenceHit>,
}

#[derive(Debug, Clone)]
struct CommentTarget {
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
    file_path: String,
}

#[derive(Debug, Clone)]
enum InlineDraftMode {
    Comment(CommentTarget),
    Reply {
        comment_id: u64,
        old_line: Option<u32>,
        new_line: Option<u32>,
    },
}

#[derive(Debug, Clone)]
struct InlineCommentState {
    row_index: usize,
    mode: InlineDraftMode,
    buffer: TextBuffer,
    preview_mode: bool,
    file_mention: Option<InlineFileMentionState>,
}

#[derive(Debug, Clone)]
struct InlineFileMentionState {
    replace_start_col: usize,
    replace_end_col: usize,
    path_query: String,
    line_suffix: Option<String>,
    candidates: Vec<String>,
    selected_index: usize,
    scroll: usize,
}

#[derive(Debug, Clone, Copy)]
struct ReplyTarget {
    selected_comment_index: usize,
    comment_id: u64,
    old_line: Option<u32>,
    new_line: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct ThreadAnchor {
    comment_index: usize,
    row_index: usize,
    comment_id: u64,
    old_line: Option<u32>,
    new_line: Option<u32>,
}

#[derive(Debug, Clone)]
struct FileReferenceHit {
    rendered_row_index: usize,
    col_start: usize,
    col_end: usize,
    path: String,
    line: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffPane {
    Primary,
    Secondary,
}

#[derive(Debug, Clone)]
enum CommandPromptMode {
    GotoLine,
    Search,
}

#[derive(Debug, Clone)]
struct CommandPromptState {
    mode: CommandPromptMode,
    value: String,
    cursor_col: usize,
}

#[derive(Debug, Clone)]
enum SettingsEditorKind {
    UserName,
}

#[derive(Debug, Clone)]
struct SettingsEditorState {
    kind: SettingsEditorKind,
    value: String,
    cursor_col: usize,
}

#[derive(Debug, Clone)]
struct ThemePickerState {
    selected_index: usize,
    scroll: usize,
}

#[derive(Debug, Clone)]
struct FileSearchState {
    query: String,
    cursor_col: usize,
    focused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileFilterMode {
    All,
    Open,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSortMode {
    Path,
    OpenCountDesc,
    TotalCountDesc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ThreadDensityMode {
    Compact,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandPaletteAction {
    ToggleFullscreen,
    ToggleSplitDiff,
    ToggleSideBySideDiff,
    ToggleThreadNavigator,
    RefreshReviewAndDiff,
    SetReviewOpen,
    SetReviewUnderReview,
    SetReviewDone,
    OpenUserNameEditor,
    OpenThemePicker,
    ToggleLightDarkTheme,
    CycleAiProvider,
    RunAiReviewRefactor,
    RunAiThreadRefactor,
    RunAiThreadReply,
    CancelAiRun,
    JumpNextThread,
    JumpPrevThread,
    CycleFileFilter,
    CycleFileSort,
    ToggleActiveFileGroup,
    CollapseAllFileGroups,
    CycleThreadDensityMode,
    ToggleSelectedThreadExpansion,
    OpenShortcuts,
}

#[derive(Debug, Clone)]
struct CommandPaletteItem {
    action: CommandPaletteAction,
    label: &'static str,
    keywords: &'static str,
}

#[derive(Debug, Clone)]
struct CommandPaletteState {
    query: String,
    cursor_col: usize,
    selected_index: usize,
    scroll: usize,
}

#[derive(Debug, Clone)]
enum PendingUiAction {
    OpenLogsInLess,
}

#[derive(Debug)]
struct AiRunTask {
    started_at: Instant,
    provider: AiProvider,
    mode: AiSessionMode,
    handle: JoinHandle<Result<crate::services::ai_session::AiSessionResult>>,
    progress_rx: Receiver<AiProgressEvent>,
}

#[derive(Debug, Clone)]
struct TextBuffer {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
}
#[derive(Debug)]
struct TuiApp {
    review_name: String,
    review: ReviewSession,
    config: AppConfig,
    themes: Vec<UiTheme>,
    theme_index: usize,
    diff: DiffDocument,
    ai_provider: AiProvider,
    log_path: PathBuf,
    selected_file: usize,
    secondary_selected_file: usize,
    active_diff_pane: DiffPane,
    split_diff_view: bool,
    side_by_side_diff: bool,
    file_pane_width_delta: i16,
    content_fullscreen: bool,
    thread_nav_visible: bool,
    selected_line: usize,
    secondary_selected_line: usize,
    primary_viewport_top_row: usize,
    secondary_viewport_top_row: usize,
    selected_comment: usize,
    status_line: String,
    last_ai_detail: Option<String>,
    inline_comment: Option<InlineCommentState>,
    command_palette: Option<CommandPaletteState>,
    theme_picker: Option<ThemePickerState>,
    file_search: FileSearchState,
    file_filter_mode: FileFilterMode,
    file_sort_mode: FileSortMode,
    collapsed_file_groups: HashSet<String>,
    thread_density_mode: ThreadDensityMode,
    expanded_threads: HashSet<u64>,
    collapsed_threads: HashSet<u64>,
    settings_editor: Option<SettingsEditorState>,
    command_prompt: Option<CommandPromptState>,
    pending_action: Option<PendingUiAction>,
    ai_task: Option<AiRunTask>,
    ai_progress_visible: bool,
    ai_progress_lines: VecDeque<String>,
    ai_progress_scroll: usize,
    ai_progress_follow_tail: bool,
    shortcuts_modal_visible: bool,
    shortcuts_modal_scroll: usize,
    shortcuts_modal_doc_index: usize,
    shortcuts_modal_zoom_step: i16,
    search_query: Option<String>,
    last_ai_progress_area: Option<Rect>,
    last_shortcuts_modal_area: Option<Rect>,
    last_file_area: Option<Rect>,
    last_file_search_area: Option<Rect>,
    last_file_scroll: usize,
    last_file_row_map: Vec<Option<usize>>,
    last_file_group_map: Vec<Option<String>>,
    last_diff_area: Option<Rect>,
    last_diff_scroll: usize,
    last_diff_row_map: Vec<usize>,
    last_diff_link_hits: Vec<FileReferenceHit>,
    pending_scroll_anchor_row: Option<usize>,
    last_diff_area_secondary: Option<Rect>,
    last_diff_scroll_secondary: usize,
    last_diff_row_map_secondary: Vec<usize>,
    last_diff_link_hits_secondary: Vec<FileReferenceHit>,
    pending_scroll_anchor_row_secondary: Option<usize>,
    last_thread_nav_area: Option<Rect>,
    last_thread_nav_scroll: usize,
    last_thread_nav_row_map: Vec<usize>,
    row_cache: HashMap<usize, CachedFileRows>,
    diff_render_cache: HashMap<DiffRenderCacheKey, DiffRenderCacheEntry>,
    diff_render_cache_order: VecDeque<DiffRenderCacheKey>,
    pending_z_prefix_at: Option<Instant>,
    redraw_invalidated: bool,
    should_quit: bool,
}
