mod syntax;
mod theme;

use std::{collections::HashMap, io, time::Duration};

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    domain::{
        config::{AppConfig, default_user_name},
        diff::{DiffDocument, DiffFile, DiffLineKind},
        review::{Author, CommentStatus, DiffSide, LineComment, ReviewSession, ReviewState},
    },
    git::diff::load_git_diff_head,
    services::review_service::{AddCommentInput, AddReplyInput, ReviewService},
};

use self::syntax::SyntaxPainter;
use self::theme::{ThemeColors, UiTheme, default_theme_name, load_themes, resolve_theme_index};

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

    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;

    let mut app = TuiApp::new(review_name, review, diff, config, themes, theme_index);
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
    while !app.should_quit {
        terminal.draw(|frame| draw(frame, app))?;

        if event::poll(Duration::from_millis(120)).context("event poll failed")? {
            match event::read().context("event read failed")? {
                Event::Key(key) => app.handle_key(key, service).await?,
                Event::Mouse(mouse) => app.handle_mouse(mouse)?,
                _ => {}
            }
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let root = frame.area();
    if app.content_fullscreen {
        app.last_file_area = None;
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(86), Constraint::Percentage(14)])
            .split(root);
        draw_diff_view(frame, app, sections[0]);
        draw_status_panel(frame, app, sections[1]);
        if app.settings_editor.is_some() {
            draw_settings_editor(frame, app);
        }
        if app.command_prompt.is_some() {
            draw_command_prompt(frame, app);
        }
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(22), Constraint::Percentage(78)])
        .split(root);

    draw_file_sidebar(frame, app, columns[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(88), Constraint::Percentage(12)])
        .split(columns[1]);

    draw_diff_view(frame, app, right[0]);
    draw_status_panel(frame, app, right[1]);
    if app.settings_editor.is_some() {
        draw_settings_editor(frame, app);
    }
    if app.command_prompt.is_some() {
        draw_command_prompt(frame, app);
    }
}

fn draw_settings_editor(frame: &mut Frame<'_>, app: &TuiApp) {
    let Some(editor) = app.settings_editor.as_ref() else {
        return;
    };

    let root = frame.area();
    let width = root.width.min(72);
    let height: u16 = 7;
    let x = root.x + root.width.saturating_sub(width) / 2;
    let y = root.y + root.height.saturating_sub(height) / 2;
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    let title = match editor.kind {
        SettingsEditorKind::UserName => "Set User Name",
    };
    let colors = &app.theme().colors;
    let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let horizontal_scroll = editor
        .cursor_col
        .saturating_sub(inner_width.saturating_sub(1));
    let visible_value = slice_chars(&editor.value, horizontal_scroll, inner_width);

    let content = vec![
        Line::from("Type a display name for your comments/replies."),
        Line::from(""),
        Line::from(visible_value),
        Line::from(""),
        Line::from(Span::styled(
            "Enter save | Esc cancel | ←/→ move | Backspace/Delete edit",
            Style::default().fg(colors.status_help),
        )),
    ];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(content).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors.thread_border))
                .title_style(
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD),
                ),
        ),
        area,
    );

    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add((editor.cursor_col.saturating_sub(horizontal_scroll)) as u16);
    let cursor_y = area.y.saturating_add(3);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_command_prompt(frame: &mut Frame<'_>, app: &TuiApp) {
    let Some(prompt) = app.command_prompt.as_ref() else {
        return;
    };

    let colors = &app.theme().colors;
    let root = frame.area();
    let height: u16 = 3;
    if root.height < height {
        return;
    }

    let area = Rect {
        x: root.x,
        y: root.y + root.height - height,
        width: root.width,
        height,
    };

    let prefix = match prompt.mode {
        CommandPromptMode::GotoLine => ":",
        CommandPromptMode::Search => "/",
    };

    let inner_width = usize::from(area.width.saturating_sub(4)).max(1);
    let horizontal_scroll = prompt
        .cursor_col
        .saturating_sub(inner_width.saturating_sub(1));
    let visible_value = slice_chars(&prompt.value, horizontal_scroll, inner_width);
    let content = vec![Line::from(format!("{prefix}{visible_value}"))];

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(content).block(
            Block::default()
                .title("Command")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors.thread_border))
                .title_style(
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD),
                ),
        ),
        area,
    );

    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(1)
        .saturating_add((prompt.cursor_col.saturating_sub(horizontal_scroll)) as u16);
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_file_sidebar(frame: &mut Frame<'_>, app: &mut TuiApp, area: ratatui::layout::Rect) {
    app.last_file_area = Some(area);
    let colors = app.theme().colors.clone();
    let comment_stats = app.file_comment_stats();

    let items: Vec<ListItem<'_>> = if app.diff.files.is_empty() {
        vec![ListItem::new("(no files in git diff HEAD)")]
    } else {
        app.diff
            .files
            .iter()
            .map(|file| {
                let (total_comments, open_comments) =
                    comment_stats.get(&file.path).copied().unwrap_or((0, 0));

                if total_comments == 0 {
                    return ListItem::new(file.path.clone());
                }

                let marker_style = if open_comments > 0 {
                    Style::default()
                        .fg(colors.comment_title)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors.text_muted)
                };
                let marker_text = if open_comments > 0 {
                    format!(" ●{} ", open_comments)
                } else {
                    format!(" ◌{} ", total_comments)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(marker_text, marker_style),
                    Span::raw(file.path.clone()),
                ]))
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.diff.files.is_empty() {
        state.select(Some(app.selected_file));
        app.last_file_scroll =
            compute_scroll(app.selected_file, area.height.saturating_sub(2) as usize);
    } else {
        app.last_file_scroll = 0;
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("Files")
                .borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM)
                .border_style(Style::default().fg(colors.thread_border))
                .title_style(
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .bg(colors.sidebar_highlight_bg)
                .fg(colors.sidebar_highlight_fg),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_diff_view(frame: &mut Frame<'_>, app: &mut TuiApp, area: ratatui::layout::Rect) {
    app.last_diff_area = Some(area);
    let colors = app.theme().colors.clone();
    if app.current_file().is_none() {
        app.last_diff_scroll = 0;
        app.last_diff_row_map.clear();
        frame.render_widget(
            Paragraph::new("No git changes against HEAD.").block(
                Block::default()
                    .title("Diff")
                    .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                    .border_style(Style::default().fg(colors.thread_border))
                    .title_style(
                        Style::default()
                            .fg(colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
            ),
            area,
        );
        return;
    }

    app.ensure_row_cache();
    let file_path = app.current_file().expect("checked").path.clone();
    let (lines, row_map, selected_visual_index) = {
        let rows = app.current_rows();
        let file_comments = app.comments_for_file(&file_path);
        let mut lines = Vec::new();
        let mut row_map = Vec::new();
        let mut selected_visual_index = 0usize;

        for (index, row) in rows.iter().enumerate() {
            if index == app.selected_line {
                selected_visual_index = lines.len();
            }
            let mut spans = Vec::new();
            let is_selected = index == app.selected_line;

            spans.push(Span::styled(
                if is_selected { "▌" } else { " " },
                if is_selected {
                    Style::default()
                        .fg(colors.selection_marker)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors.text_muted)
                },
            ));

            let old = row
                .old_line
                .map(|value| value.to_string())
                .unwrap_or_else(|| " ".to_string());
            let new = row
                .new_line
                .map(|value| value.to_string())
                .unwrap_or_else(|| " ".to_string());

            let (sign, sign_style) = match row.kind {
                DiffLineKind::Added => (
                    "+",
                    Style::default()
                        .fg(colors.added_sign)
                        .add_modifier(Modifier::BOLD),
                ),
                DiffLineKind::Removed => (
                    "-",
                    Style::default()
                        .fg(colors.removed_sign)
                        .add_modifier(Modifier::BOLD),
                ),
                DiffLineKind::Context => (" ", Style::default().fg(colors.context_sign)),
                DiffLineKind::HunkHeader | DiffLineKind::Meta => {
                    (" ", Style::default().fg(colors.text_muted))
                }
            };
            spans.push(Span::styled(sign, sign_style));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{:>5} {:>5}  ", old, new),
                Style::default().fg(colors.text_muted),
            ));

            match row.kind {
                DiffLineKind::Added => {
                    for (style, text) in &app.highlight_cache[index] {
                        spans.push(Span::styled(text.clone(), *style));
                    }
                }
                DiffLineKind::Removed => {
                    for (style, text) in &app.highlight_cache[index] {
                        spans.push(Span::styled(text.clone(), *style));
                    }
                }
                DiffLineKind::Context => {
                    for (style, text) in &app.highlight_cache[index] {
                        spans.push(Span::styled(text.clone(), *style));
                    }
                }
                DiffLineKind::HunkHeader => {
                    spans.push(Span::styled(
                        row.raw.clone(),
                        Style::default()
                            .fg(colors.hunk_header)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                DiffLineKind::Meta => {
                    spans.push(Span::styled(
                        row.raw.clone(),
                        Style::default().fg(colors.meta),
                    ));
                }
            }

            let line = if is_selected {
                Line::from(spans).patch_style(
                    Style::default()
                        .bg(colors.selected_line_bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Line::from(spans)
            };
            lines.push(line);
            row_map.push(index);

            for comment in file_comments
                .iter()
                .copied()
                .filter(|comment| comment_matches_display_row(comment, row))
            {
                let status = match comment.status {
                    CommentStatus::Open => "open",
                    CommentStatus::Addressed => "addressed",
                };
                let pane_inner_width = usize::from(area.width.saturating_sub(2));
                let inner_width = compute_thread_inner_width(pane_inner_width, 12);
                let comment_title = format!("comment #{} [{}]", comment.id, status);
                let comment_header = format!(
                    "{} | {}",
                    app.author_label(&comment.author),
                    format_timestamp_utc(comment.created_at_ms)
                );
                push_thread_box(
                    &mut lines,
                    &mut row_map,
                    ThreadBoxSpec {
                        source_row_index: index,
                        indent: 12,
                        inner_width,
                        header: &comment_header,
                        title: &comment_title,
                        body: &comment.body,
                        border_color: colors.thread_border,
                        title_color: colors.comment_title,
                        colors: &colors,
                    },
                );

                for reply in &comment.replies {
                    let reply_title = format!("reply #{}", reply.id);
                    let reply_header = format!(
                        "{} | {}",
                        app.author_label(&reply.author),
                        format_timestamp_utc(reply.created_at_ms)
                    );
                    push_thread_box(
                        &mut lines,
                        &mut row_map,
                        ThreadBoxSpec {
                            source_row_index: index,
                            indent: 14,
                            inner_width: compute_thread_inner_width(pane_inner_width, 14),
                            header: &reply_header,
                            title: &reply_title,
                            body: &reply.body,
                            border_color: colors.thread_border,
                            title_color: colors.reply_title,
                            colors: &colors,
                        },
                    );
                }
            }
        }

        (lines, row_map, selected_visual_index)
    };

    let viewport_height = area.height.saturating_sub(2) as usize;
    let scroll = compute_scroll(selected_visual_index, viewport_height);
    app.last_diff_scroll = scroll;
    app.last_diff_row_map = row_map;

    let title = format!("Diff: {}", app.current_file().expect("checked").path);
    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(colors.thread_border))
                .title_style(
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .scroll((scroll as u16, 0));

    frame.render_widget(widget, area);

    if app.inline_comment.is_some() {
        draw_inline_comment_editor(frame, app, area);
    }
}

fn draw_inline_comment_editor(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(inline) = app.inline_comment.as_ref() else {
        return;
    };
    let colors = &app.theme().colors;
    if area.height < 8 || area.width < 32 {
        return;
    }

    let box_height = area.height.min(12);
    let editor_area = Rect {
        x: area.x + 1,
        y: area.y + area.height - box_height,
        width: area.width.saturating_sub(2),
        height: box_height.saturating_sub(1),
    };

    let mode = if inline.preview_mode {
        "PREVIEW"
    } else {
        "EDIT"
    };
    let (title_kind, line_ref) = match &inline.mode {
        InlineDraftMode::Comment(target) => (
            "Comment Box".to_string(),
            format!(
                "{}:{}",
                target
                    .old_line
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "_".to_string()),
                target
                    .new_line
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "_".to_string())
            ),
        ),
        InlineDraftMode::Reply {
            comment_id,
            old_line,
            new_line,
            ..
        } => (
            format!("Reply Box #{}", comment_id),
            format!(
                "{}:{}",
                old_line
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "_".to_string()),
                new_line
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "_".to_string())
            ),
        ),
    };

    let help_line = "Ctrl+S save | Ctrl+P preview | Ctrl+A/E/K/B/F | ↑/↓ move lines | Esc collapse";

    frame.render_widget(Clear, editor_area);
    let block = Block::default()
        .title(format!("{title_kind} [{mode}] line {line_ref}"))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors.thread_border))
        .title_style(
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        );

    if inline.preview_mode {
        let mut content = render_markdown(&inline.buffer.to_text(), colors);
        if content.is_empty() {
            content.push(Line::from(""));
        }
        content.push(Line::from(""));
        content.push(Line::from(help_line));
        frame.render_widget(
            Paragraph::new(content)
                .block(block)
                .wrap(Wrap { trim: false }),
            editor_area,
        );
        return;
    }

    if editor_area.width < 3 || editor_area.height < 4 {
        frame.render_widget(Paragraph::new("").block(block), editor_area);
        return;
    }

    let inner_width = usize::from(editor_area.width.saturating_sub(2));
    let inner_height = usize::from(editor_area.height.saturating_sub(2));
    let text_height = inner_height.saturating_sub(1).max(1);

    let cursor_line = inline.buffer.cursor_line;
    let cursor_col = inline.buffer.cursor_col;
    let vertical_scroll = cursor_line.saturating_sub(text_height.saturating_sub(1));
    let horizontal_scroll = cursor_col.saturating_sub(inner_width.saturating_sub(1));

    let mut content = Vec::new();
    for offset in 0..text_height {
        let idx = vertical_scroll + offset;
        if let Some(line) = inline.buffer.lines.get(idx) {
            content.push(Line::from(slice_chars(
                line,
                horizontal_scroll,
                inner_width,
            )));
        } else {
            content.push(Line::from(""));
        }
    }
    content.push(Line::from(Span::styled(
        help_line,
        Style::default().fg(colors.status_help),
    )));

    frame.render_widget(Paragraph::new(content).block(block), editor_area);

    let cursor_x = editor_area
        .x
        .saturating_add(1)
        .saturating_add((cursor_col.saturating_sub(horizontal_scroll)) as u16);
    let cursor_y = editor_area
        .y
        .saturating_add(1)
        .saturating_add((cursor_line.saturating_sub(vertical_scroll)) as u16);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_markdown(buffer: &str, colors: &ThemeColors) -> Vec<Line<'static>> {
    let mut in_code_block = false;
    let mut rendered = Vec::new();

    for raw_line in buffer.lines() {
        if raw_line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            rendered.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default()
                    .fg(colors.markdown_fence)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if in_code_block {
            rendered.push(Line::from(Span::styled(
                raw_line.to_string(),
                Style::default().fg(colors.markdown_code_fg),
            )));
            continue;
        }

        if let Some(stripped) = raw_line.strip_prefix("### ") {
            rendered.push(Line::from(Span::styled(
                stripped.to_string(),
                Style::default()
                    .fg(colors.markdown_heading)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(stripped) = raw_line.strip_prefix("## ") {
            rendered.push(Line::from(Span::styled(
                stripped.to_string(),
                Style::default()
                    .fg(colors.markdown_heading)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(stripped) = raw_line.strip_prefix("# ") {
            rendered.push(Line::from(Span::styled(
                stripped.to_string(),
                Style::default()
                    .fg(colors.markdown_heading)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        if let Some(stripped) = raw_line.strip_prefix("> ") {
            rendered.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(colors.markdown_quote_mark)),
                Span::styled(
                    stripped.to_string(),
                    Style::default()
                        .fg(colors.markdown_quote_text)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
            continue;
        }

        if raw_line.starts_with("- ") || raw_line.starts_with("* ") {
            rendered.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(colors.markdown_bullet)),
                Span::styled(
                    raw_line[2..].to_string(),
                    Style::default().fg(colors.text_primary),
                ),
            ]));
            continue;
        }

        rendered.push(Line::from(parse_inline_markdown(raw_line, colors)));
    }

    if rendered.is_empty() {
        rendered.push(Line::from(Span::styled(
            "(empty markdown)",
            Style::default().fg(colors.text_primary),
        )));
    }
    rendered
}

fn parse_inline_markdown(input: &str, colors: &ThemeColors) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut bold = false;
    let mut code = false;

    let flush = |spans: &mut Vec<Span<'static>>, text: &mut String, bold: bool, code: bool| {
        if text.is_empty() {
            return;
        }
        let mut style = Style::default().fg(colors.text_primary);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if code {
            style = style
                .fg(colors.markdown_code_fg)
                .bg(colors.markdown_code_bg);
        }
        spans.push(Span::styled(std::mem::take(text), style));
    };

    while let Some(ch) = chars.next() {
        if ch == '`' {
            flush(&mut spans, &mut current, bold, code);
            code = !code;
            continue;
        }

        if ch == '*' && chars.peek() == Some(&'*') {
            chars.next();
            flush(&mut spans, &mut current, bold, code);
            bold = !bold;
            continue;
        }

        current.push(ch);
    }

    flush(&mut spans, &mut current, bold, code);
    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }
    spans
}

const THREAD_BOX_MIN_CONTENT_WIDTH: usize = 16;
const THREAD_BOX_MAX_CONTENT_WIDTH: usize = 79;
const MOUSE_WHEEL_SCROLL_LINES: usize = 3;
const MOUSE_WHEEL_FILE_SCROLL_FILES: usize = 3;

fn compute_thread_inner_width(pane_inner_width: usize, indent: usize) -> usize {
    // Each thread line uses: indent + "│ " + content + " │"
    let available = pane_inner_width.saturating_sub(indent + 4);
    available.clamp(THREAD_BOX_MIN_CONTENT_WIDTH, THREAD_BOX_MAX_CONTENT_WIDTH)
}

fn comment_matches_display_row(comment: &LineComment, row: &DisplayRow) -> bool {
    if !matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    ) {
        return false;
    }

    match comment.side {
        DiffSide::Left => comment.old_line.is_some() && comment.old_line == row.old_line,
        DiffSide::Right => comment.new_line.is_some() && comment.new_line == row.new_line,
    }
}

fn format_line_reference(old_line: Option<u32>, new_line: Option<u32>) -> String {
    match (old_line, new_line) {
        (Some(old), Some(new)) => format!("{old}:{new}"),
        (Some(old), None) => format!("{old}:_"),
        (None, Some(new)) => format!("_:{new}"),
        (None, None) => "_:_".to_string(),
    }
}

fn format_timestamp_utc(timestamp_ms: u64) -> String {
    let seconds = timestamp_ms / 1_000;
    let millis = timestamp_ms % 1_000;
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = (seconds % 86_400) as u32;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{millis:03} UTC")
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    // Howard Hinnant's civil-from-days algorithm.
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u32, day as u32)
}

struct ThreadBoxSpec<'a> {
    source_row_index: usize,
    indent: usize,
    inner_width: usize,
    header: &'a str,
    title: &'a str,
    body: &'a str,
    border_color: Color,
    title_color: Color,
    colors: &'a ThemeColors,
}

fn push_thread_box(
    lines: &mut Vec<Line<'static>>,
    row_map: &mut Vec<usize>,
    spec: ThreadBoxSpec<'_>,
) {
    let indent_str = " ".repeat(spec.indent);
    let indent = Style::default();
    let border = Style::default()
        .fg(spec.border_color)
        .bg(spec.colors.thread_background)
        .add_modifier(Modifier::BOLD);
    let title_style = Style::default()
        .fg(spec.title_color)
        .bg(spec.colors.thread_background)
        .add_modifier(Modifier::BOLD);
    let header_style = Style::default()
        .fg(spec.colors.text_muted)
        .bg(spec.colors.thread_background)
        .add_modifier(Modifier::BOLD);
    let body_style = Style::default()
        .fg(spec.colors.text_primary)
        .bg(spec.colors.thread_background);

    let horizontal = "─".repeat(spec.inner_width + 2);
    lines.push(Line::from(vec![
        Span::styled(indent_str.clone(), indent),
        Span::styled(format!("╭{horizontal}╮"), border),
    ]));
    row_map.push(spec.source_row_index);

    let title_text = fit_to_width(spec.title, spec.inner_width);
    lines.push(Line::from(vec![
        Span::styled(indent_str.clone(), indent),
        Span::styled("│ ".to_string(), border),
        Span::styled(title_text, title_style),
        Span::styled(" │".to_string(), border),
    ]));
    row_map.push(spec.source_row_index);

    let header_text = fit_to_width(spec.header, spec.inner_width);
    lines.push(Line::from(vec![
        Span::styled(indent_str.clone(), indent),
        Span::styled("│ ".to_string(), border),
        Span::styled(header_text, header_style),
        Span::styled(" │".to_string(), border),
    ]));
    row_map.push(spec.source_row_index);

    lines.push(Line::from(vec![
        Span::styled(indent_str.clone(), indent),
        Span::styled(format!("├{horizontal}┤"), border),
    ]));
    row_map.push(spec.source_row_index);

    for wrapped in wrap_markdown_lines(spec.body, spec.inner_width, spec.colors) {
        let mut row_spans = Vec::new();
        row_spans.push(Span::styled(indent_str.clone(), indent));
        row_spans.push(Span::styled("│ ".to_string(), border));

        let mut rendered_width = 0usize;
        for span in wrapped.spans {
            rendered_width += span.content.chars().count();
            let style = if span.style.bg.is_some() {
                span.style
            } else {
                span.style.bg(spec.colors.thread_background)
            };
            row_spans.push(Span::styled(span.content, style));
        }

        if rendered_width < spec.inner_width {
            row_spans.push(Span::styled(
                " ".repeat(spec.inner_width - rendered_width),
                body_style,
            ));
        }
        row_spans.push(Span::styled(" │".to_string(), border));

        lines.push(Line::from(row_spans));
        row_map.push(spec.source_row_index);
    }

    lines.push(Line::from(vec![
        Span::styled(indent_str, indent),
        Span::styled(format!("╰{horizontal}╯"), border),
    ]));
    row_map.push(spec.source_row_index);
}

fn fit_to_width(input: &str, width: usize) -> String {
    let mut out: String = input.chars().take(width).collect();
    let missing = width.saturating_sub(out.chars().count());
    if missing > 0 {
        out.push_str(&" ".repeat(missing));
    }
    out
}

fn wrap_markdown_lines(input: &str, width: usize, colors: &ThemeColors) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut out = Vec::new();
    for line in render_markdown(input, colors) {
        out.extend(wrap_styled_line(&line, width));
    }
    if out.is_empty() {
        out.push(Line::from(""));
    }
    out
}

fn wrap_styled_line(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut styled_chars: Vec<(Style, char)> = Vec::new();
    for span in &line.spans {
        for ch in span.content.chars() {
            styled_chars.push((span.style, ch));
        }
    }

    if styled_chars.is_empty() {
        return vec![Line::from("")];
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    while start < styled_chars.len() {
        let end = (start + width).min(styled_chars.len());
        out.push(line_from_styled_chars(&styled_chars[start..end]));
        start = end;
    }
    out
}

fn line_from_styled_chars(chars: &[(Style, char)]) -> Line<'static> {
    if chars.is_empty() {
        return Line::from("");
    }

    let mut spans = Vec::new();
    let mut current_style: Option<Style> = None;
    let mut current_text = String::new();

    for (style, ch) in chars {
        if current_style == Some(*style) {
            current_text.push(*ch);
            continue;
        }

        if let Some(previous_style) = current_style {
            spans.push(Span::styled(
                std::mem::take(&mut current_text),
                previous_style,
            ));
        }
        current_style = Some(*style);
        current_text.push(*ch);
    }

    if let Some(style) = current_style {
        spans.push(Span::styled(current_text, style));
    }

    Line::from(spans)
}

fn slice_chars(input: &str, start: usize, len: usize) -> String {
    if len == 0 {
        return String::new();
    }
    input.chars().skip(start).take(len).collect()
}

fn point_in_rect(x: u16, y: u16, rect: Rect) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn compute_scroll(selected_index: usize, viewport_height: usize) -> usize {
    if viewport_height == 0 {
        return 0;
    }
    if selected_index >= viewport_height {
        selected_index - viewport_height + 1
    } else {
        0
    }
}

fn draw_status_panel(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let colors = &app.theme().colors;
    let mode_label = if let Some(inline) = app.inline_comment.as_ref() {
        let draft_kind = match inline.mode {
            InlineDraftMode::Comment(_) => "COMMENT",
            InlineDraftMode::Reply { .. } => "REPLY",
        };
        if inline.preview_mode {
            format!(
                "{draft_kind}_BOX_PREVIEW > {} chars",
                inline.buffer.char_len()
            )
        } else {
            format!("{draft_kind}_BOX_EDIT > {} chars", inline.buffer.char_len())
        }
    } else if app.settings_editor.is_some() {
        "SETTINGS_EDIT".to_string()
    } else {
        "NORMAL".to_string()
    };

    let help_line_1 = "keys: q quit | z content fullscreen | h/l file | j/k line | m/c comment | r reply | n/p thread";
    let help_line_2 = ":<line> goto | /<text> search | N/P search next/prev | u set name | t/T theme | a/o comment | s/w/d state";
    let line_1 = Line::from(vec![
        Span::styled(
            mode_label,
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::raw(format!(
            "review: {} ({:?}) | user: {}",
            app.review.name, app.review.state, app.config.user_name
        )),
    ]);
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let line_2 = build_right_tag_line(
        &app.status_line,
        &version,
        inner_width,
        Style::default().fg(colors.status_help),
    );
    let thread_line = if let Some(comment) = app.selected_comment_details() {
        let status = match comment.status {
            CommentStatus::Open => "open",
            CommentStatus::Addressed => "addressed",
        };
        Line::from(format!(
            "thread: #{} [{}] line {} | replies {} | {}",
            comment.id,
            status,
            format_line_reference(comment.old_line, comment.new_line),
            comment.replies.len(),
            format_timestamp_utc(comment.created_at_ms)
        ))
    } else {
        Line::from("thread: none")
    };

    let mut panel_lines = vec![line_1, line_2, thread_line];
    panel_lines.push(Line::from(Span::styled(
        help_line_1,
        Style::default().fg(colors.status_help),
    )));
    panel_lines.push(Line::from(Span::styled(
        help_line_2,
        Style::default().fg(colors.status_help),
    )));

    let panel = Paragraph::new(panel_lines).block(
        Block::default()
            .title("Status")
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(colors.thread_border))
            .title_style(
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(panel, area);
}

fn build_right_tag_line(
    left: &str,
    right: &str,
    width: usize,
    right_style: Style,
) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }

    let right_len = right.chars().count();
    if right_len >= width {
        return Line::from(Span::styled(
            right.chars().take(width).collect::<String>(),
            right_style,
        ));
    }

    let max_left_len = width.saturating_sub(right_len + 1);
    let clipped_left: String = left.chars().take(max_left_len).collect();
    let gap_len = width
        .saturating_sub(clipped_left.chars().count())
        .saturating_sub(right_len);

    Line::from(vec![
        Span::raw(clipped_left),
        Span::raw(" ".repeat(gap_len)),
        Span::styled(right.to_string(), right_style),
    ])
}

#[derive(Debug, Clone)]
struct DisplayRow {
    kind: DiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    raw: String,
    code: String,
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
struct TextBuffer {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
}

impl TextBuffer {
    fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
        }
    }

    fn char_len(&self) -> usize {
        let text_chars: usize = self.lines.iter().map(|line| line.chars().count()).sum();
        text_chars + self.lines.len().saturating_sub(1)
    }

    fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.line_len(self.cursor_line);
        }
    }

    fn move_right(&mut self) {
        let line_len = self.line_len(self.cursor_line);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
        }
    }

    fn move_down(&mut self) {
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
        }
    }

    fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    fn move_end(&mut self) {
        self.cursor_col = self.line_len(self.cursor_line);
    }

    fn insert_char(&mut self, ch: char) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        chars.insert(self.cursor_col, ch);
        self.lines[self.cursor_line] = chars.into_iter().collect();
        self.cursor_col += 1;
    }

    fn insert_spaces(&mut self, count: usize) {
        for _ in 0..count {
            self.insert_char(' ');
        }
    }

    fn insert_newline(&mut self) {
        let current = self.lines[self.cursor_line].clone();
        let left = slice_chars(&current, 0, self.cursor_col);
        let right = slice_chars(
            &current,
            self.cursor_col,
            current.chars().count().saturating_sub(self.cursor_col),
        );
        self.lines[self.cursor_line] = left;
        self.lines.insert(self.cursor_line + 1, right);
        self.cursor_line += 1;
        self.cursor_col = 0;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
            let remove_at = self.cursor_col - 1;
            if remove_at < chars.len() {
                chars.remove(remove_at);
                self.lines[self.cursor_line] = chars.into_iter().collect();
                self.cursor_col -= 1;
            }
            return;
        }

        if self.cursor_line > 0 {
            let current = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            let previous_len = self.line_len(self.cursor_line);
            self.lines[self.cursor_line].push_str(&current);
            self.cursor_col = previous_len;
        }
    }

    fn delete_char(&mut self) {
        let line_len = self.line_len(self.cursor_line);
        if self.cursor_col < line_len {
            let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
            chars.remove(self.cursor_col);
            self.lines[self.cursor_line] = chars.into_iter().collect();
            return;
        }

        if self.cursor_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
        }
    }

    fn kill_to_end(&mut self) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        chars.truncate(self.cursor_col);
        self.lines[self.cursor_line] = chars.into_iter().collect();
    }

    fn line_len(&self, idx: usize) -> usize {
        self.lines[idx].chars().count()
    }
}

#[derive(Debug)]
struct TuiApp {
    review_name: String,
    review: ReviewSession,
    config: AppConfig,
    themes: Vec<UiTheme>,
    theme_index: usize,
    diff: DiffDocument,
    selected_file: usize,
    content_fullscreen: bool,
    selected_line: usize,
    selected_comment: usize,
    status_line: String,
    inline_comment: Option<InlineCommentState>,
    settings_editor: Option<SettingsEditorState>,
    command_prompt: Option<CommandPromptState>,
    search_query: Option<String>,
    last_file_area: Option<Rect>,
    last_file_scroll: usize,
    last_diff_area: Option<Rect>,
    last_diff_scroll: usize,
    last_diff_row_map: Vec<usize>,
    row_cache: Vec<DisplayRow>,
    highlight_cache: Vec<Vec<(Style, String)>>,
    cached_file_index: Option<usize>,
    should_quit: bool,
}

impl TuiApp {
    fn new(
        review_name: String,
        review: ReviewSession,
        diff: DiffDocument,
        config: AppConfig,
        themes: Vec<UiTheme>,
        theme_index: usize,
    ) -> Self {
        Self {
            review_name,
            review,
            config,
            themes,
            theme_index,
            diff,
            selected_file: 0,
            content_fullscreen: false,
            selected_line: 0,
            selected_comment: 0,
            status_line: "ready".to_string(),
            inline_comment: None,
            settings_editor: None,
            command_prompt: None,
            search_query: None,
            last_file_area: None,
            last_file_scroll: 0,
            last_diff_area: None,
            last_diff_scroll: 0,
            last_diff_row_map: Vec::new(),
            row_cache: Vec::new(),
            highlight_cache: Vec::new(),
            cached_file_index: None,
            should_quit: false,
        }
    }

    fn theme(&self) -> &UiTheme {
        &self.themes[self.theme_index]
    }

    fn author_label(&self, author: &Author) -> &str {
        match author {
            Author::User => &self.config.user_name,
            Author::Ai => "AI",
        }
    }

    fn select_file(&mut self, index: usize) {
        if self.diff.files.is_empty() {
            self.selected_file = 0;
            return;
        }

        let clamped = index.min(self.diff.files.len().saturating_sub(1));
        if clamped == self.selected_file {
            return;
        }

        self.selected_file = clamped;
        self.selected_line = 0;
        self.selected_comment = 0;
        self.inline_comment = None;
        self.cached_file_index = None;
    }

    fn move_file_selection(&mut self, delta: isize) {
        if self.diff.files.is_empty() {
            self.selected_file = 0;
            return;
        }
        let max = self.diff.files.len().saturating_sub(1) as isize;
        let next = (self.selected_file as isize + delta).clamp(0, max) as usize;
        self.select_file(next);
    }

    fn current_file(&self) -> Option<&DiffFile> {
        self.diff.files.get(self.selected_file)
    }

    fn current_rows(&self) -> &[DisplayRow] {
        &self.row_cache
    }

    fn comments_for_file(&self, file_path: &str) -> Vec<&LineComment> {
        self.review
            .comments
            .iter()
            .filter(|comment| comment.file_path == file_path)
            .collect()
    }

    fn file_comment_stats(&self) -> HashMap<String, (usize, usize)> {
        let mut stats = HashMap::new();
        for comment in &self.review.comments {
            let entry = stats.entry(comment.file_path.clone()).or_insert((0, 0));
            entry.0 += 1;
            if matches!(comment.status, CommentStatus::Open) {
                entry.1 += 1;
            }
        }
        stats
    }

    fn comments_for_selected_file(&self) -> Vec<&LineComment> {
        let Some(file) = self.current_file() else {
            return Vec::new();
        };
        self.comments_for_file(&file.path)
    }

    fn selected_comment_details(&self) -> Option<&LineComment> {
        let comments = self.comments_for_selected_file();
        comments.get(self.selected_comment).copied()
    }

    fn constrain_selection(&mut self) {
        let rows_len = if self.cached_file_index == Some(self.selected_file) {
            self.current_rows().len()
        } else {
            0
        };
        if rows_len == 0 {
            self.selected_line = 0;
        } else if self.selected_line >= rows_len {
            self.selected_line = rows_len - 1;
        }

        let comments_len = self.comments_for_selected_file().len();
        if comments_len == 0 {
            self.selected_comment = 0;
        } else if self.selected_comment >= comments_len {
            self.selected_comment = comments_len - 1;
        }

        if self.selected_file >= self.diff.files.len() {
            self.selected_file = self.diff.files.len().saturating_sub(1);
        }

        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index >= rows_len
        {
            self.inline_comment = None;
        }
    }

    async fn handle_key(&mut self, key: KeyEvent, service: &ReviewService) -> Result<()> {
        if self.settings_editor.is_some() {
            return self.handle_settings_editor_key(key, service).await;
        }
        if self.inline_comment.is_some() {
            return self.handle_inline_comment_key(key, service).await;
        }
        if self.command_prompt.is_some() {
            return self.handle_command_prompt_key(key);
        }

        self.handle_normal_key(key, service).await
    }

    async fn handle_normal_key(&mut self, key: KeyEvent, service: &ReviewService) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('z') => {
                self.content_fullscreen = !self.content_fullscreen;
                if self.content_fullscreen {
                    self.status_line = "content fullscreen enabled".into();
                } else {
                    self.status_line = "content fullscreen disabled".into();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_file_selection(-1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_file_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.ensure_row_cache();
                self.selected_line = self.selected_line.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.ensure_row_cache();
                let max = self.current_rows().len().saturating_sub(1);
                self.selected_line = (self.selected_line + 1).min(max);
            }
            KeyCode::Char('g') => {
                self.selected_line = 0;
            }
            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.ensure_row_cache();
                self.selected_line = self.current_rows().len().saturating_sub(1);
            }
            KeyCode::Char('c') | KeyCode::Char('m') => {
                self.ensure_row_cache();
                self.toggle_inline_comment_for_selected_line();
            }
            KeyCode::Char('r') => {
                self.ensure_row_cache();
                self.start_inline_reply_for_selected_comment();
            }
            KeyCode::Char(':') => {
                self.command_prompt = Some(CommandPromptState {
                    mode: CommandPromptMode::GotoLine,
                    value: String::new(),
                    cursor_col: 0,
                });
                self.status_line = "goto line prompt".into();
            }
            KeyCode::Char('/') => {
                self.command_prompt = Some(CommandPromptState {
                    mode: CommandPromptMode::Search,
                    value: self.search_query.clone().unwrap_or_default(),
                    cursor_col: self
                        .search_query
                        .as_ref()
                        .map(|value| value.chars().count())
                        .unwrap_or(0),
                });
                self.status_line = "search prompt".into();
            }
            KeyCode::Char('n') => {
                self.ensure_row_cache();
                self.jump_thread(true);
            }
            KeyCode::Char('p') => {
                self.ensure_row_cache();
                self.jump_thread(false);
            }
            KeyCode::Char('N') => {
                self.ensure_row_cache();
                self.jump_search(false);
            }
            KeyCode::Char('P') => {
                self.ensure_row_cache();
                self.jump_search(true);
            }
            KeyCode::Char('u') => {
                self.open_user_name_editor();
            }
            KeyCode::Char('t') => {
                if let Err(error) = self.cycle_theme(service).await {
                    self.status_line = format!("theme change failed: {error}");
                }
            }
            KeyCode::Char('T') => {
                if let Err(error) = self.toggle_light_dark_theme(service).await {
                    self.status_line = format!("theme variant toggle failed: {error}");
                }
            }
            KeyCode::Char(']') => {
                let max = self.comments_for_selected_file().len().saturating_sub(1);
                self.selected_comment = (self.selected_comment + 1).min(max);
            }
            KeyCode::Char('[') => {
                self.selected_comment = self.selected_comment.saturating_sub(1);
            }
            KeyCode::Char('a') => {
                if let Some(comment) = self.selected_comment_details() {
                    let comment_id = comment.id;
                    match service
                        .mark_addressed(&self.review_name, comment_id, Author::User)
                        .await
                    {
                        Ok(_) => {
                            self.reload_review(service).await?;
                            self.status_line = format!("comment #{comment_id} marked addressed");
                        }
                        Err(error) => {
                            self.status_line = format!("mark addressed failed: {error}");
                        }
                    }
                }
            }
            KeyCode::Char('o') => {
                if let Some(comment) = self.selected_comment_details() {
                    let comment_id = comment.id;
                    match service
                        .mark_open(&self.review_name, comment_id, Author::User)
                        .await
                    {
                        Ok(_) => {
                            self.reload_review(service).await?;
                            self.status_line = format!("comment #{comment_id} marked open");
                        }
                        Err(error) => {
                            self.status_line = format!("mark open failed: {error}");
                        }
                    }
                }
            }
            KeyCode::Char('s') => {
                if let Err(error) = self.set_state(service, ReviewState::Pending).await {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('w') => {
                if let Err(error) = self
                    .set_state(service, ReviewState::WaitingForResponse)
                    .await
                {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('d') => {
                if let Err(error) = self.set_state(service, ReviewState::Done).await {
                    self.status_line = error.to_string();
                }
            }
            _ => {}
        }

        self.constrain_selection();
        Ok(())
    }

    async fn handle_settings_editor_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.settings_editor = None;
            self.status_line = "settings edit cancelled".into();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Enter) {
            return self.save_settings_editor(service).await;
        }

        let Some(editor) = self.settings_editor.as_mut() else {
            return Ok(());
        };

        match key.code {
            KeyCode::Left => {
                editor.cursor_col = editor.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                editor.cursor_col = (editor.cursor_col + 1).min(editor.value.chars().count());
            }
            KeyCode::Home => editor.cursor_col = 0,
            KeyCode::End => editor.cursor_col = editor.value.chars().count(),
            KeyCode::Backspace => {
                if editor.cursor_col > 0 {
                    remove_char_at(&mut editor.value, editor.cursor_col - 1);
                    editor.cursor_col -= 1;
                }
            }
            KeyCode::Delete => {
                if editor.cursor_col < editor.value.chars().count() {
                    remove_char_at(&mut editor.value, editor.cursor_col);
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut editor.value, editor.cursor_col, ch);
                editor.cursor_col += 1;
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_command_prompt_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.command_prompt = None;
            self.status_line = "command cancelled".into();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Enter) {
            return self.run_command_prompt();
        }

        let Some(prompt) = self.command_prompt.as_mut() else {
            return Ok(());
        };

        match key.code {
            KeyCode::Left => {
                prompt.cursor_col = prompt.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                prompt.cursor_col = (prompt.cursor_col + 1).min(prompt.value.chars().count());
            }
            KeyCode::Home => prompt.cursor_col = 0,
            KeyCode::End => prompt.cursor_col = prompt.value.chars().count(),
            KeyCode::Backspace => {
                if prompt.cursor_col > 0 {
                    remove_char_at(&mut prompt.value, prompt.cursor_col - 1);
                    prompt.cursor_col -= 1;
                }
            }
            KeyCode::Delete => {
                if prompt.cursor_col < prompt.value.chars().count() {
                    remove_char_at(&mut prompt.value, prompt.cursor_col);
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut prompt.value, prompt.cursor_col, ch);
                prompt.cursor_col += 1;
            }
            _ => {}
        }

        Ok(())
    }

    fn run_command_prompt(&mut self) -> Result<()> {
        let Some(prompt) = self.command_prompt.take() else {
            return Ok(());
        };

        match prompt.mode {
            CommandPromptMode::GotoLine => self.goto_line_from_prompt(&prompt.value),
            CommandPromptMode::Search => self.search_from_prompt(&prompt.value),
        }
    }

    fn goto_line_from_prompt(&mut self, input: &str) -> Result<()> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            self.status_line = "goto line expects a number".into();
            return Ok(());
        }

        let Ok(target) = trimmed.parse::<u32>() else {
            self.status_line = format!("invalid line number: {trimmed}");
            return Ok(());
        };

        if self.goto_line_number(target) {
            self.status_line = format!("jumped to line {target}");
        } else {
            self.status_line = format!("line {target} not found in current diff file");
        }
        Ok(())
    }

    fn goto_line_number(&mut self, target: u32) -> bool {
        if target == 0 {
            return false;
        }
        self.ensure_row_cache();

        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.new_line == Some(target))
        {
            self.selected_line = row_index;
            return true;
        }

        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.old_line == Some(target))
        {
            self.selected_line = row_index;
            return true;
        }

        false
    }

    fn search_from_prompt(&mut self, input: &str) -> Result<()> {
        let query = input.trim();
        if query.is_empty() {
            self.status_line = "search expects non-empty text".into();
            return Ok(());
        }
        self.search_query = Some(query.to_string());
        if self.find_search_match(query, true) {
            self.status_line = format!("search match: {query}");
        } else {
            self.status_line = format!("no match for: {query}");
        }
        Ok(())
    }

    fn jump_search(&mut self, forward: bool) {
        let Some(query) = self.search_query.clone() else {
            self.status_line = "no active search (use /text)".into();
            return;
        };

        if self.find_search_match(&query, forward) {
            self.status_line = format!("search match: {query}");
        } else {
            self.status_line = format!("no further match for: {query}");
        }
    }

    fn find_search_match(&mut self, query: &str, forward: bool) -> bool {
        self.ensure_row_cache();
        let rows = self.current_rows();
        if rows.is_empty() {
            return false;
        }

        let len = rows.len();
        let query_lower = query.to_lowercase();
        let mut index = self.selected_line;

        for _ in 0..len {
            index = if forward {
                (index + 1) % len
            } else {
                (index + len - 1) % len
            };

            let haystack = rows[index].raw.to_lowercase();
            if haystack.contains(&query_lower) {
                self.selected_line = index;
                return true;
            }
        }

        false
    }

    fn jump_thread(&mut self, forward: bool) {
        self.ensure_row_cache();
        let comments = self.comments_for_selected_file();
        if comments.is_empty() {
            self.status_line = "no comments in current file".into();
            return;
        }

        let mut anchors: Vec<ThreadAnchor> = comments
            .iter()
            .enumerate()
            .filter_map(|(comment_index, comment)| {
                self.current_rows()
                    .iter()
                    .position(|row| comment_matches_display_row(comment, row))
                    .map(|row_index| ThreadAnchor {
                        comment_index,
                        row_index,
                        comment_id: comment.id,
                        old_line: comment.old_line,
                        new_line: comment.new_line,
                    })
            })
            .collect();
        if anchors.is_empty() {
            self.status_line = "no thread anchors visible in current file".into();
            return;
        }

        anchors.sort_by_key(|anchor| (anchor.row_index, anchor.comment_index));
        let current_row = self.selected_line;
        let current_comment = self.selected_comment;

        let target = if forward {
            anchors
                .iter()
                .copied()
                .find(|anchor| {
                    anchor.row_index > current_row
                        || (anchor.row_index == current_row
                            && anchor.comment_index > current_comment)
                })
                .unwrap_or(anchors[0])
        } else {
            anchors
                .iter()
                .rev()
                .copied()
                .find(|anchor| {
                    anchor.row_index < current_row
                        || (anchor.row_index == current_row
                            && anchor.comment_index < current_comment)
                })
                .unwrap_or(*anchors.last().expect("anchors checked as non-empty"))
        };

        self.selected_comment = target.comment_index;
        self.selected_line = target.row_index;
        self.status_line = format!(
            "thread #{} at line {}",
            target.comment_id,
            format_line_reference(target.old_line, target.new_line)
        );
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.settings_editor.is_some() || self.command_prompt.is_some() {
            return Ok(());
        }

        if let Some(file_area) = self.last_file_area
            && point_in_rect(mouse.column, mouse.row, file_area)
        {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > file_area.y
                        && mouse.row < file_area.y + file_area.height.saturating_sub(1) =>
                {
                    let row = self.last_file_scroll
                        + usize::from(mouse.row.saturating_sub(file_area.y + 1));
                    self.select_file(row);
                    if self.selected_file < self.diff.files.len() {
                        self.status_line =
                            format!("selected file {}", self.diff.files[self.selected_file].path);
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.move_file_selection(-(MOUSE_WHEEL_FILE_SCROLL_FILES as isize));
                }
                MouseEventKind::ScrollDown => {
                    self.move_file_selection(MOUSE_WHEEL_FILE_SCROLL_FILES as isize);
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(diff_area) = self.last_diff_area
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll + view_row;
                    if let Some(row_index) = self.last_diff_row_map.get(visible_row_index).copied()
                    {
                        self.selected_line = row_index;
                        self.toggle_inline_comment_for_row(row_index);
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.selected_line =
                        self.selected_line.saturating_sub(MOUSE_WHEEL_SCROLL_LINES);
                }
                MouseEventKind::ScrollDown => {
                    let max = self.current_rows().len().saturating_sub(1);
                    self.selected_line = self
                        .selected_line
                        .saturating_add(MOUSE_WHEEL_SCROLL_LINES)
                        .min(max);
                }
                _ => {}
            }
            self.constrain_selection();
        }

        Ok(())
    }

    async fn handle_inline_comment_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.inline_comment = None;
            self.status_line = "comment box collapsed".into();
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            if let Err(error) = self.submit_inline_comment(service).await {
                self.status_line = format!("save comment failed: {error}");
            }
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('p')) {
            if let Some(inline) = self.inline_comment.as_mut() {
                inline.preview_mode = !inline.preview_mode;
                self.status_line = if inline.preview_mode {
                    "markdown preview enabled".into()
                } else {
                    "markdown preview disabled".into()
                };
            }
            return Ok(());
        }

        let Some(inline) = self.inline_comment.as_mut() else {
            return Ok(());
        };

        if inline.preview_mode {
            return Ok(());
        }

        match key.code {
            KeyCode::Left => inline.buffer.move_left(),
            KeyCode::Right => inline.buffer.move_right(),
            KeyCode::Up => inline.buffer.move_up(),
            KeyCode::Down => inline.buffer.move_down(),
            KeyCode::Home => inline.buffer.move_home(),
            KeyCode::End => inline.buffer.move_end(),
            KeyCode::Enter => inline.buffer.insert_newline(),
            KeyCode::Tab => inline.buffer.insert_spaces(4),
            KeyCode::Backspace => inline.buffer.backspace(),
            KeyCode::Delete => inline.buffer.delete_char(),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                inline.buffer.insert_char(ch);
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_home();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_end();
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.kill_to_end();
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_up();
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_down();
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_left();
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_right();
            }
            _ => {}
        }
        Ok(())
    }

    fn comment_target_for_row(&self, row_index: usize) -> Option<CommentTarget> {
        let file = self.current_file()?;
        let row = self.current_rows().get(row_index)?.clone();

        let (side, old_line, new_line) = match row.kind {
            DiffLineKind::Added => (DiffSide::Right, None, row.new_line),
            DiffLineKind::Removed => (DiffSide::Left, row.old_line, None),
            DiffLineKind::Context => (DiffSide::Right, row.old_line, row.new_line),
            _ => return None,
        };

        Some(CommentTarget {
            side,
            old_line,
            new_line,
            file_path: file.path.clone(),
        })
    }

    fn toggle_inline_comment_for_selected_line(&mut self) {
        self.toggle_inline_comment_for_row(self.selected_line);
    }

    fn toggle_inline_comment_for_row(&mut self, row_index: usize) {
        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index == row_index
            && matches!(inline.mode, InlineDraftMode::Comment(_))
        {
            self.inline_comment = None;
            self.status_line = "comment box collapsed".into();
            return;
        }

        let Some(target) = self.comment_target_for_row(row_index) else {
            self.inline_comment = None;
            self.status_line = "selected line cannot receive comments".into();
            return;
        };

        self.inline_comment = Some(InlineCommentState {
            row_index,
            mode: InlineDraftMode::Comment(target),
            buffer: TextBuffer::new(),
            preview_mode: false,
        });
        self.status_line = "comment box expanded".into();
    }

    fn start_inline_reply_for_selected_comment(&mut self) {
        let Some(target) = self.reply_target_for_selected_line() else {
            self.status_line = "no comment on selected line".into();
            return;
        };
        self.selected_comment = target.selected_comment_index;
        let selected_comment_id = target.comment_id;
        let old_line = target.old_line;
        let new_line = target.new_line;

        if let Some(inline) = self.inline_comment.as_ref()
            && matches!(
                inline.mode,
                InlineDraftMode::Reply {
                    comment_id,
                    ..
                } if comment_id == selected_comment_id
            )
        {
            self.inline_comment = None;
            self.status_line = "reply box collapsed".into();
            return;
        }

        self.inline_comment = Some(InlineCommentState {
            row_index: self.selected_line,
            mode: InlineDraftMode::Reply {
                comment_id: selected_comment_id,
                old_line,
                new_line,
            },
            buffer: TextBuffer::new(),
            preview_mode: false,
        });
        self.status_line = format!("reply box opened for comment #{}", selected_comment_id);
    }

    fn reply_target_for_selected_line(&self) -> Option<ReplyTarget> {
        let row = self.current_rows().get(self.selected_line)?;
        let comments = self.comments_for_selected_file();
        let matches: Vec<(usize, &LineComment)> = comments
            .into_iter()
            .enumerate()
            .filter(|(_, comment)| comment_matches_display_row(comment, row))
            .collect();
        if matches.is_empty() {
            return None;
        }

        let selected = if let Some(selected) = matches
            .iter()
            .find(|(idx, _)| *idx == self.selected_comment)
            .copied()
        {
            selected
        } else {
            matches.last().copied()?
        };

        Some(ReplyTarget {
            selected_comment_index: selected.0,
            comment_id: selected.1.id,
            old_line: selected.1.old_line,
            new_line: selected.1.new_line,
        })
    }

    async fn submit_inline_comment(&mut self, service: &ReviewService) -> Result<()> {
        let Some(inline) = self.inline_comment.take() else {
            return Ok(());
        };

        if inline.buffer.is_blank() {
            self.status_line = "comment body cannot be empty".into();
            self.inline_comment = Some(inline);
            return Ok(());
        }

        let body = inline.buffer.to_text();

        match inline.mode {
            InlineDraftMode::Comment(target) => {
                service
                    .add_comment(
                        &self.review_name,
                        AddCommentInput {
                            file_path: target.file_path,
                            old_line: target.old_line,
                            new_line: target.new_line,
                            side: target.side,
                            body,
                            author: Author::User,
                        },
                    )
                    .await
                    .context("failed to save comment")?;
                self.status_line = "comment saved".into();
            }
            InlineDraftMode::Reply {
                comment_id,
                old_line,
                new_line,
            } => {
                service
                    .add_reply(
                        &self.review_name,
                        AddReplyInput {
                            comment_id,
                            author: Author::User,
                            body,
                        },
                    )
                    .await
                    .context("failed to save reply")?;
                self.status_line = format!(
                    "reply saved on #{} at {}:{}",
                    comment_id,
                    old_line
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "_".to_string()),
                    new_line
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "_".to_string())
                );
            }
        }
        self.reload_review(service).await?;
        Ok(())
    }

    fn ensure_row_cache(&mut self) {
        if self.cached_file_index == Some(self.selected_file) {
            return;
        }
        self.rebuild_row_cache();
    }

    fn rebuild_row_cache(&mut self) {
        self.row_cache.clear();
        self.highlight_cache.clear();

        let Some(file) = self.current_file() else {
            self.cached_file_index = Some(self.selected_file);
            return;
        };

        let mut rows = Vec::new();
        for header in &file.header_lines {
            rows.push(DisplayRow {
                kind: DiffLineKind::Meta,
                old_line: None,
                new_line: None,
                raw: header.clone(),
                code: header.clone(),
            });
        }
        for hunk in &file.hunks {
            for line in &hunk.lines {
                rows.push(DisplayRow {
                    kind: line.kind.clone(),
                    old_line: line.old_line,
                    new_line: line.new_line,
                    raw: line.raw.clone(),
                    code: line.code.clone(),
                });
            }
        }

        let mut painter = SyntaxPainter::for_path(&file.path);
        let mut highlights = Vec::with_capacity(rows.len());
        for row in &rows {
            let parts = match row.kind {
                DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context => {
                    painter.highlight(&row.code)
                }
                _ => Vec::new(),
            };
            highlights.push(parts);
        }

        self.row_cache = rows;
        self.highlight_cache = highlights;
        self.cached_file_index = Some(self.selected_file);
    }

    async fn set_state(&mut self, service: &ReviewService, next: ReviewState) -> Result<()> {
        service
            .set_state(&self.review_name, next.clone())
            .await
            .with_context(|| format!("failed to set state to {:?}", next))?;
        self.reload_review(service).await?;
        self.status_line = format!("review state set to {:?}", next);
        Ok(())
    }

    async fn reload_review(&mut self, service: &ReviewService) -> Result<()> {
        self.review = service.load_review(&self.review_name).await?;
        self.cached_file_index = None;
        self.constrain_selection();
        Ok(())
    }

    fn open_user_name_editor(&mut self) {
        let value = self.config.user_name.clone();
        let cursor_col = value.chars().count();
        self.settings_editor = Some(SettingsEditorState {
            kind: SettingsEditorKind::UserName,
            value,
            cursor_col,
        });
        self.status_line = "editing user name".into();
    }

    async fn save_settings_editor(&mut self, service: &ReviewService) -> Result<()> {
        let Some(editor) = self.settings_editor.take() else {
            return Ok(());
        };

        match editor.kind {
            SettingsEditorKind::UserName => {
                let next = editor.value.trim();
                if next.is_empty() {
                    self.status_line = "user name cannot be empty".into();
                    self.settings_editor = Some(SettingsEditorState {
                        kind: SettingsEditorKind::UserName,
                        value: editor.value,
                        cursor_col: editor.cursor_col,
                    });
                    return Ok(());
                }
                self.config.user_name = next.to_string();
                service.save_config(&self.config).await?;
                self.status_line = format!("user name set to {}", self.config.user_name);
            }
        }
        Ok(())
    }

    async fn cycle_theme(&mut self, service: &ReviewService) -> Result<()> {
        if self.themes.is_empty() {
            self.status_line = "no themes loaded".into();
            return Ok(());
        }
        self.theme_index = (self.theme_index + 1) % self.themes.len();
        self.config.theme = self.theme().name.clone();
        service.save_config(&self.config).await?;
        self.status_line = format!("theme set to {}", self.config.theme);
        Ok(())
    }

    async fn toggle_light_dark_theme(&mut self, service: &ReviewService) -> Result<()> {
        if self.themes.is_empty() {
            self.status_line = "no themes loaded".into();
            return Ok(());
        }

        let current = self.theme().name.clone();
        let candidate = if let Some(prefix) = current.strip_suffix("_dark") {
            format!("{prefix}_light")
        } else if let Some(prefix) = current.strip_suffix("_light") {
            format!("{prefix}_dark")
        } else if current.contains("dark") {
            current.replace("dark", "light")
        } else if current.contains("light") {
            current.replace("light", "dark")
        } else {
            "gruvbox_light".to_string()
        };

        let target_index = resolve_theme_index(&self.themes, &candidate).unwrap_or_else(|| {
            resolve_theme_index(&self.themes, "gruvbox_light")
                .or_else(|| resolve_theme_index(&self.themes, "gruvbox_dark"))
                .unwrap_or(self.theme_index)
        });

        self.theme_index = target_index;
        self.config.theme = self.theme().name.clone();
        service.save_config(&self.config).await?;
        self.status_line = format!("theme set to {}", self.config.theme);
        Ok(())
    }
}

fn insert_char_at(text: &mut String, char_index: usize, ch: char) {
    let mut chars: Vec<char> = text.chars().collect();
    let idx = char_index.min(chars.len());
    chars.insert(idx, ch);
    *text = chars.into_iter().collect();
}

fn remove_char_at(text: &mut String, char_index: usize) {
    let mut chars: Vec<char> = text.chars().collect();
    if char_index < chars.len() {
        chars.remove(char_index);
        *text = chars.into_iter().collect();
    }
}
