use std::{
    collections::{HashMap, VecDeque},
    io,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect, style::Style};
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
use super::theme::{UiTheme, default_theme_name, load_themes, resolve_theme_index};

pub async fn run_tui(
    service: ReviewService,
    review_name: String,
    requested_theme: Option<String>,
) -> Result<()> {
    let review = service
        .load_or_create_review(&review_name)
        .await
        .with_context(|| format!("failed to open review {review_name}"))?;
    let diff = load_git_diff_head().await?;
    let themes = load_themes()?;
    let mut config = service.load_config().await?;

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

    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;

    let mut app = TuiApp::new(
        review_name,
        review,
        diff,
        config,
        themes,
        theme_index,
        log_path,
    );
    let run_result = run_loop(&mut terminal, &mut app, &service).await;

    disable_raw_mode().context("failed to disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("failed to leave alternate screen")?;
    terminal.show_cursor().context("failed to show cursor")?;

    run_result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    service: &ReviewService,
) -> Result<()> {
    const MAX_EVENTS_PER_TICK: usize = 128;
    while !app.should_quit {
        terminal.draw(|frame| draw(frame, app))?;

        if event::poll(Duration::from_millis(120)).context("event poll failed")? {
            let mut processed = 0usize;
            loop {
                match event::read().context("event read failed")? {
                    Event::Key(key) => app.handle_key(key, service).await?,
                    Event::Mouse(mouse) => app.handle_mouse(mouse)?,
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

        if let Some(action) = app.pending_action.take() {
            match action {
                PendingUiAction::OpenLogsInLess => {
                    match open_log_in_less(terminal, &app.log_path) {
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
        }

        app.poll_ai_task(service).await?;
    }

    Ok(())
}

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
    selected_comment: usize,
    status_line: String,
    last_ai_detail: Option<String>,
    inline_comment: Option<InlineCommentState>,
    settings_editor: Option<SettingsEditorState>,
    command_prompt: Option<CommandPromptState>,
    pending_action: Option<PendingUiAction>,
    ai_task: Option<AiRunTask>,
    ai_progress_visible: bool,
    ai_progress_lines: VecDeque<String>,
    shortcuts_modal_visible: bool,
    shortcuts_modal_scroll: usize,
    search_query: Option<String>,
    last_shortcuts_modal_area: Option<Rect>,
    last_file_area: Option<Rect>,
    last_file_scroll: usize,
    last_diff_area: Option<Rect>,
    last_diff_scroll: usize,
    last_diff_row_map: Vec<usize>,
    last_diff_area_secondary: Option<Rect>,
    last_diff_scroll_secondary: usize,
    last_diff_row_map_secondary: Vec<usize>,
    last_thread_nav_area: Option<Rect>,
    last_thread_nav_scroll: usize,
    last_thread_nav_row_map: Vec<usize>,
    row_cache: HashMap<usize, CachedFileRows>,
    should_quit: bool,
}
