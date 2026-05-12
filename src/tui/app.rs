use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::{self, JoinHandle};

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::{AgentTransport, AppConfig, default_user_name};
use crate::domain::diff::{DiffDocument, DiffFile, DiffLineKind};
use crate::domain::review::{
    Author, CommentLineRange, CommentStatus, DiffSide, LineAnchorSnapshot, LineComment,
    ReviewSession, ReviewState,
};
use crate::git::diff::{
    DiffSource, load_git_diff, load_root_directory_file, load_root_directory_file_list,
};
use crate::git::history::{FileHeatmapEntry, file_heatmap};
use crate::services::ai_session::{
    AiProgressEvent, RunAiSessionInput, run_ai_session_with_progress,
};
use crate::services::review_service::ReviewService;

use super::syntax::SyntaxPainter;
use super::terminal::TerminalSession;
use super::theme::{UiTheme, default_theme_name, load_themes, resolve_theme_index};

mod help_docs;
mod helpers;
mod input;
#[cfg(test)]
mod perf_tests;
mod render;
mod state;

use helpers::{
    MOUSE_WHEEL_FILE_SCROLL_FILES, MOUSE_WHEEL_SCROLL_LINES,
    comment_line_range_contains_display_row, comment_matches_display_row,
    comment_reference_matches_display_row, format_comment_reference, format_line_range_reference,
    format_line_reference, insert_char_at, open_file_in_pager, point_in_rect, remove_char_at,
    suspend_tui_process,
};
use render::draw;
pub(super) use render::{
    DiffRenderCacheEntry, DiffRenderCacheKey, DisplayRow, FileReferenceHit, HighlightParts,
};

/// # Errors
///
/// Returns an error when terminal setup fails, review/config/diff data cannot be loaded, settings
/// cannot be saved, logging cannot be initialized, or TUI event handling fails.
pub async fn run_tui(
    service: ReviewService,
    review_name: String,
    no_mouse: bool,
    diff_source: DiffSource,
    create_review_if_missing: bool,
) -> Result<()> {
    let mut terminal_session = TerminalSession::new(!no_mouse)?;
    let review = if create_review_if_missing {
        service.load_or_create_review(&review_name).await?
    } else {
        service
            .load_review(&review_name)
            .await
            .with_context(|| {
                format!(
                    "failed to open review {review_name}; create it first with `parley review create {review_name}`"
                )
            })?
    };
    let themes = load_themes()?;
    let mut config = service.load_config().await?;
    let diff = if matches!(diff_source, DiffSource::RootDirectory) {
        DiffDocument { files: Vec::new() }
    } else {
        load_git_diff(&config, &diff_source).await?
    };

    if config.user_name.trim().is_empty() || config.user_name == "User" {
        config.user_name = default_user_name();
    }

    let theme_index = resolve_theme_index(&themes, &config.theme)
        .unwrap_or_else(|| resolve_theme_index(&themes, default_theme_name()).unwrap_or(0));
    config.theme = themes[theme_index].name.clone();
    service.save_config(&config).await?;
    let log_path = service.review_log_path(&review_name)?;
    super::logging::init_file_tracing(&log_path, &config.log_level)
        .await
        .context("failed to initialize tui log writer")?;

    let mut app = TuiApp::new(TuiAppInit {
        review_name,
        review,
        diff,
        diff_source,
        config,
        themes,
        theme_index,
        log_path,
    });
    if matches!(app.diff_source, DiffSource::RootDirectory) {
        app.root_diff_load_started_at = Some(Instant::now());
        app.status_line = "Loading reviewable root files...".into();
        let config = app.config.clone();
        let diff_source = app.diff_source.clone();
        app.root_diff_load_task = Some(task::spawn(async move {
            let _ = diff_source;
            load_root_directory_file_list(&config).await
        }));
    } else {
        app.refresh_review_and_diff(&service).await?;
    }
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
                        app.handle_mouse(mouse).await?;
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
        let heatmap_changed = app.poll_file_heatmap().await?;
        if heatmap_changed {
            app.invalidate_redraw();
        }
        let diff_load_updated = app.poll_root_directory_diff_load(service).await?;
        if diff_load_updated {
            app.invalidate_redraw();
        }
        let root_file_load_updated = app.poll_root_directory_file_load().await?;
        if root_file_load_updated {
            app.invalidate_redraw();
        }

        if let Some(action) = app.pending_action.take() {
            match action {
                PendingUiAction::SuspendTuiProcess => {
                    match suspend_tui_process(terminal, mouse_capture_enabled) {
                        Ok(()) => {
                            app.status_line = "resumed parley (Ctrl+Z suspend)".into();
                        }
                        Err(error) => {
                            app.status_line = format!("suspend failed: {error}");
                        }
                    }
                }
                PendingUiAction::OpenFileInPager(path) => {
                    match open_file_in_pager(terminal, mouse_capture_enabled, &path) {
                        Ok(()) => {
                            app.status_line = format!("returned from pager: {}", path.display());
                        }
                        Err(error) => {
                            app.status_line = format!("pager failed: {error}");
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

const AI_PROGRESS_MAX_LINES: usize = 300;
const AI_LOG_MAX_SESSIONS_PER_FILE: usize = 32;
const DIFF_RENDER_CACHE_MAX_ENTRIES: usize = 64;
const INLINE_FILE_MENTION_MAX_VISIBLE_ROWS: usize = 6;
const INLINE_FILE_MENTION_MAX_CANDIDATES: usize = 120;

#[derive(Debug, Clone)]
struct CachedFileRows {
    rows: Vec<DisplayRow>,
    highlights: Vec<Option<HighlightParts>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FileCommentStats {
    total: usize,
    open: usize,
    pending: usize,
}

type RootFileLoadResult = Result<(usize, Option<DiffFile>)>;
type FileHeatmapLoadResult = Result<Vec<FileHeatmapEntry>>;

#[derive(Debug, Clone)]
struct CommentTarget {
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line_range: Option<CommentLineRange>,
    file_path: String,
    line_anchor: LineAnchorSnapshot,
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
    file_reference_picker: Option<InlineFileReferencePickerState>,
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

#[derive(Debug, Clone)]
struct InlineFileReferencePickerState {
    path: String,
    replace_start_col: usize,
    replace_end_col: usize,
    origin_pane: DiffPane,
    origin_file_index: usize,
    origin_row_index: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffPane {
    Primary,
    Secondary,
}

#[derive(Debug, Clone)]
enum CommandPromptMode {
    GotoLine,
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
    CreateReview,
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
struct CommitPickerEntry {
    oid: String,
    short_oid: String,
    summary: String,
}

#[derive(Debug, Clone)]
struct CommitPickerState {
    commits: Vec<CommitPickerEntry>,
    query: String,
    cursor_col: usize,
    selected_index: usize,
    scroll: usize,
}

#[derive(Debug, Clone)]
struct ReviewPickerEntry {
    name: String,
    state: ReviewState,
    open_count: usize,
    pending_count: usize,
    addressed_count: usize,
}

#[derive(Debug, Clone)]
struct ReviewPickerState {
    reviews: Vec<ReviewPickerEntry>,
    query: String,
    cursor_col: usize,
    selected_index: usize,
    scroll: usize,
}

#[derive(Debug, Clone)]
struct ThreadSelectorEntry {
    comment_id: u64,
    file_path: String,
    status: CommentStatus,
    line_reference: String,
    preview: String,
}

#[derive(Debug, Clone)]
struct ThreadSelectorState {
    query: String,
    cursor_col: usize,
    selected_index: usize,
    scroll: usize,
}

#[derive(Debug, Clone)]
struct FileSearchState {
    query: String,
    cursor_col: usize,
    focused: bool,
}

#[derive(Debug, Clone)]
struct CodeSearchResult {
    path: String,
    line: u32,
    column: u32,
    text: String,
}

#[derive(Debug, Clone)]
struct CodeSearchState {
    query: String,
    cursor_col: usize,
    results: Vec<CodeSearchResult>,
    selected_index: usize,
    scroll: usize,
    engine: Option<&'static str>,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileFilterMode {
    All,
    Open,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ThreadDensityMode {
    Compact,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSortMode {
    Path,
    OpenCountDesc,
    TotalCountDesc,
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
    OpenUserNameEditor,
    OpenThemePicker,
    OpenCommitPicker,
    OpenReviewPicker,
    OpenThreadSelector,
    CreateReview,
    OpenCodeSearch,
    ToggleLightDarkTheme,
    CycleAiProvider,
    ToggleAiTransport,
    RunAiReviewRefactor,
    RunAiThreadRefactor,
    RunAiThreadReply,
    CancelAiRun,
    ShowAiActivity,
    JumpNextThread,
    JumpPrevThread,
    CycleFileFilter,
    CycleFileSort,
    ToggleActiveFileGroup,
    CollapseAllFileGroups,
    ShowFileHeatmap,
    CycleThreadDensityMode,
    ToggleSelectedThreadExpansion,
    ToggleRootDocumentRendering,
    OpenShortcuts,
}

#[derive(Debug, Clone)]
struct CommandPaletteItem {
    action: CommandPaletteAction,
    label: &'static str,
    keywords: &'static str,
}

#[derive(Debug, Clone)]
struct FileHeatmapState {
    entries: Vec<FileHeatmapEntry>,
    scroll: usize,
    sort_mode: FileHeatmapSortMode,
    sort_descending: bool,
    loaded_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileHeatmapSortMode {
    Churn,
    Added,
    Removed,
    Commits,
    NetGrowth,
    NetShrink,
    Volatility,
    Path,
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
    SuspendTuiProcess,
    OpenFileInPager(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AiLogSessionStatus {
    Running,
    Finished,
    Failed,
    Cancelled,
}

impl AiLogSessionStatus {
    fn as_str(self) -> &'static str {
        match self {
            AiLogSessionStatus::Running => "running",
            AiLogSessionStatus::Finished => "finished",
            AiLogSessionStatus::Failed => "failed",
            AiLogSessionStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
struct AiLogEvent {
    timestamp_ms: u64,
    stream: String,
    message: String,
}

#[derive(Debug, Clone)]
struct AiLogSession {
    id: u64,
    file_path: String,
    provider: AiProvider,
    mode: AiSessionMode,
    started_at: Instant,
    started_at_ms: u64,
    finished_at_ms: Option<u64>,
    status: AiLogSessionStatus,
    unread_events: usize,
    events: VecDeque<AiLogEvent>,
}

#[derive(Debug, Clone)]
struct AiActivityEntry {
    session_id: u64,
    file_path: String,
    provider: AiProvider,
    mode: AiSessionMode,
    status: AiLogSessionStatus,
    started_at_ms: u64,
    finished_at_ms: Option<u64>,
    unread_events: usize,
    event_count: usize,
    last_event: Option<AiLogEvent>,
}

#[derive(Debug)]
struct AiRunTask {
    log_session_id: u64,
    started_at: Instant,
    last_log_heartbeat_at: Instant,
    file_path: String,
    provider: AiProvider,
    mode: AiSessionMode,
    handle: JoinHandle<Result<crate::services::ai_session::AiSessionResult>>,
    progress_rx: UnboundedReceiver<AiProgressEvent>,
}

#[derive(Debug, Clone)]
struct TextBuffer {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
}

#[derive(Debug)]
struct TuiAppInit {
    review_name: String,
    review: ReviewSession,
    diff: DiffDocument,
    diff_source: DiffSource,
    config: AppConfig,
    themes: Vec<UiTheme>,
    theme_index: usize,
    log_path: PathBuf,
}

#[derive(Debug)]
struct TuiApp {
    review_name: String,
    review: ReviewSession,
    comment_indices_by_file: HashMap<String, Vec<usize>>,
    comment_stats_by_file: HashMap<String, FileCommentStats>,
    diff_source: DiffSource,
    config: AppConfig,
    themes: Vec<UiTheme>,
    theme_index: usize,
    diff: DiffDocument,
    ai_provider: AiProvider,
    ai_transport: Option<AgentTransport>,
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
    selected_visual_row: Option<usize>,
    secondary_selected_visual_row: Option<usize>,
    comment_selection_anchor: Option<(DiffPane, usize)>,
    primary_viewport_top_row: usize,
    secondary_viewport_top_row: usize,
    selected_comment: usize,
    status_line: String,
    last_status_line_snapshot: String,
    status_toast_message: Option<String>,
    status_toast_until: Option<Instant>,
    last_ai_detail: Option<String>,
    inline_comment: Option<InlineCommentState>,
    command_palette: Option<CommandPaletteState>,
    theme_picker: Option<ThemePickerState>,
    commit_picker: Option<CommitPickerState>,
    review_picker: Option<ReviewPickerState>,
    thread_selector: Option<ThreadSelectorState>,
    code_search: Option<CodeSearchState>,
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
    ai_tasks: Vec<AiRunTask>,
    ai_progress_visible: bool,
    ai_activity_visible: bool,
    ai_activity_selected: usize,
    ai_activity_scroll: usize,
    selected_ai_log_session_id: Option<u64>,
    next_ai_log_session_id: u64,
    ai_log_sessions_by_file: HashMap<String, VecDeque<AiLogSession>>,
    ai_progress_scroll: usize,
    ai_progress_follow_tail: bool,
    file_heatmap: Option<FileHeatmapState>,
    file_heatmap_task: Option<JoinHandle<FileHeatmapLoadResult>>,
    file_heatmap_started_at: Option<Instant>,
    root_diff_load_task: Option<JoinHandle<Result<DiffDocument>>>,
    root_file_load_task: Option<JoinHandle<RootFileLoadResult>>,
    root_hydrated_files: HashSet<usize>,
    root_diff_load_started_at: Option<Instant>,
    root_document_rendering: bool,
    shortcuts_modal_visible: bool,
    shortcuts_modal_scroll: usize,
    shortcuts_modal_doc_index: usize,
    shortcuts_modal_zoom_step: i16,
    search_query: Option<String>,
    last_ai_progress_area: Option<Rect>,
    last_shortcuts_modal_area: Option<Rect>,
    last_file_heatmap_area: Option<Rect>,
    last_file_area: Option<Rect>,
    last_file_search_area: Option<Rect>,
    last_code_search_area: Option<Rect>,
    last_ai_activity_area: Option<Rect>,
    last_thread_selector_area: Option<Rect>,
    last_thread_selector_scroll: usize,
    last_thread_selector_visible_rows: usize,
    last_code_search_scroll: usize,
    last_code_search_visible_rows: usize,
    last_file_scroll: usize,
    file_sidebar_manual_scroll: bool,
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
