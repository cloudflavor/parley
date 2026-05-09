use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::super::helpers::{format_comment_reference, slice_chars};
use super::helpers::{compute_scroll, fit_to_width, wrap_markdown_lines};
use super::status::spinner_frame;
use super::{CommandPromptMode, TuiApp};
use crate::tui::app::help_docs::HELP_DOCS;
use crate::tui::theme::ThemeColors;
use crate::utils::cast::{i32_to_u16_saturating, usize_to_u16_saturating};

pub(super) fn draw_thread_navigator_overlay(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let colors = app.theme().colors.clone();
    let root = frame.area();
    if root.width < 40 || root.height < 10 {
        return;
    }

    let width = root.width.clamp(36, 54);
    let height = root.height.saturating_sub(4).clamp(8, 28);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width).saturating_sub(1),
        y: root.y + 1,
        width,
        height,
    };

    app.last_thread_nav_area = Some(area);

    let comments = app.comments_for_selected_file();
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let mut lines = Vec::new();
    let mut row_map = Vec::new();

    if comments.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no threads in current file)",
            Style::default().fg(colors.text_muted),
        )));
        row_map.push(usize::MAX);
    } else {
        for (index, comment) in comments.iter().enumerate() {
            let preview = comment.body.lines().next().unwrap_or("").trim();
            let line = format!(
                "#{} {} {}",
                comment.id,
                format_comment_reference(comment),
                preview
            );
            let clipped_line = fit_to_width(&line, inner_width);
            let style = if index == app.selected_comment {
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.text_primary)
            };
            lines.push(Line::from(Span::styled(clipped_line, style)));
            row_map.push(index);
        }
    }

    let scroll = compute_scroll(app.selected_comment, inner_height.max(1));
    app.last_thread_nav_scroll = scroll;
    app.last_thread_nav_row_map = row_map;

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Thread Navigator")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors.thread_border))
                    .title_style(
                        Style::default()
                            .fg(colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .scroll((usize_to_u16_saturating(scroll), 0)),
        area,
    );
}

pub(super) fn draw_ai_progress_popup(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let colors = app.theme().colors.clone();
    let root = frame.area();
    if root.width < 40 || root.height < 10 {
        return;
    }

    let width = (root.width.saturating_mul(3) / 4).clamp(60, 160);
    let height = root.height.clamp(8, 18);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + 1,
        width,
        height,
    };
    app.last_ai_progress_area = Some(area);

    let file_path = app.ai_log_file_path();
    let running_count = app.running_ai_tasks_for_file(&file_path);
    let title = if let Some(task) = app.first_running_ai_task_for_file(&file_path) {
        format!(
            "AI Logs {} {}:{} ({running_count} running) - {}",
            spinner_frame(task.started_at),
            task.provider.as_str(),
            task.mode.as_str(),
            file_path
        )
    } else {
        format!("AI Logs - {file_path}")
    };

    let inner_height = area.height.saturating_sub(2) as usize;
    if inner_height == 0 {
        return;
    }
    let log_rows = inner_height.saturating_sub(1).max(1);

    let log_entries = app
        .ai_progress_lines_for_file(&file_path)
        .cloned()
        .unwrap_or_default();
    let total = log_entries.len();
    let mut lines = Vec::new();
    if total == 0 {
        lines.push(Line::from(Span::styled(
            "(no AI logs for this file yet)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        let content_width = usize::from(area.width.saturating_sub(2)).max(1);
        let mut wrapped_entries = Vec::new();
        for entry in &log_entries {
            let style = if entry.starts_with("stderr: ") || entry.contains(" stderr: ") {
                Style::default().fg(colors.removed_sign)
            } else if entry.contains(" system: ") {
                Style::default().fg(colors.accent)
            } else {
                Style::default().fg(colors.text_primary)
            };
            wrapped_entries.extend(wrap_plain_styled_lines(entry, content_width, style));
        }
        let max_scroll = wrapped_entries.len().saturating_sub(log_rows);
        let scroll = app.ai_progress_resolved_scroll(max_scroll);
        let end = (scroll + log_rows).min(wrapped_entries.len());
        lines.extend(wrapped_entries.into_iter().skip(scroll).take(end - scroll));
    }
    lines.push(Line::from(Span::styled(
        "H hide/show | K cancel current-file runs | PgUp/PgDn/Home/End scroll",
        Style::default().fg(colors.status_help),
    )));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors.thread_border))
                    .title_style(
                        Style::default()
                            .fg(colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn wrap_plain_styled_lines(input: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let effective_width = width.max(1);
    let mut out = Vec::new();
    for raw_line in input.lines() {
        if raw_line.is_empty() {
            out.push(Line::from(Span::styled(String::new(), style)));
            continue;
        }
        let chars: Vec<char> = raw_line.chars().collect();
        let mut start = 0usize;
        while start < chars.len() {
            let end = (start + effective_width).min(chars.len());
            let chunk: String = chars[start..end].iter().collect();
            out.push(Line::from(Span::styled(chunk, style)));
            start = end;
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(String::new(), style)));
    }
    out
}

pub(super) fn draw_shortcuts_modal(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let root = frame.area();
    let base_width = root
        .width
        .saturating_sub(2)
        .clamp(64, 132)
        .saturating_mul(14)
        / 10;
    let base_height = root
        .height
        .saturating_sub(2)
        .clamp(16, 30)
        .saturating_mul(14)
        / 10;
    let width = scaled_modal_axis(
        base_width,
        root.width.saturating_sub(2),
        app.shortcuts_modal_zoom_step,
        4,
        56,
    );
    let height = scaled_modal_axis(
        base_height,
        root.height.saturating_sub(2),
        app.shortcuts_modal_zoom_step,
        2,
        14,
    );
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    app.last_shortcuts_modal_area = Some(area);
    let colors = app.theme().colors.clone();

    let docs_count = HELP_DOCS.len();
    let doc_index = app
        .shortcuts_modal_doc_index
        .min(docs_count.saturating_sub(1));
    app.shortcuts_modal_doc_index = doc_index;
    let doc = HELP_DOCS[doc_index];

    let inner = Block::default().borders(Borders::ALL).inner(area);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let tabs_area = vertical[0];
    let content_area = vertical[1];
    let footer_area = vertical[2];

    let body = doc.body;
    let mut lines = wrap_markdown_lines(body, usize::from(content_area.width).max(1), &colors);
    if lines.is_empty() {
        lines.push(Line::from("(empty)"));
    }
    let content_height = usize::from(content_area.height).max(1);
    let max_scroll = lines.len().saturating_sub(content_height);
    let scroll = app.shortcuts_modal_scroll.min(max_scroll);
    app.shortcuts_modal_scroll = scroll;

    frame.render_widget(Clear, area);
    let title = format!("  Help Docs [{}/{}]  ", doc_index + 1, docs_count);
    frame.render_widget(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors.thread_border))
            .title_style(
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        area,
    );
    frame.render_widget(
        Paragraph::new(vec![help_docs_tabs_line(doc_index, &colors)]).wrap(Wrap { trim: true }),
        tabs_area,
    );
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((usize_to_u16_saturating(scroll), 0)),
        content_area,
    );
    let source = doc.source_path;
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![
            Span::styled(
                format!("source: {source}  "),
                Style::default().fg(colors.status_help),
            ),
            Span::styled(
                "1-9/Tab switch doc | </> zoom | j/k/PgUp/PgDn scroll | Esc/? close",
                Style::default().fg(colors.status_help),
            ),
        ])]),
        footer_area,
    );
}

fn scaled_modal_axis(base: u16, available: u16, zoom_step: i16, unit: i32, min_bound: u16) -> u16 {
    let max_value = available.max(min_bound);
    let min_value = min_bound.min(max_value);
    let proposed = i32::from(base) + i32::from(zoom_step) * unit;
    i32_to_u16_saturating(proposed.clamp(i32::from(min_value), i32::from(max_value)))
}

fn help_docs_tabs_line(selected_index: usize, colors: &ThemeColors) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, doc) in HELP_DOCS.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(" | ", Style::default().fg(colors.text_muted)));
        }
        let style = if idx == selected_index {
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(colors.text_primary)
        };
        spans.push(Span::styled(format!("{} {}", idx + 1, doc.title), style));
    }
    Line::from(spans)
}

pub(super) fn draw_command_prompt(frame: &mut Frame<'_>, app: &TuiApp) {
    let Some(prompt) = app.command_prompt.as_ref() else {
        return;
    };

    let colors = app.theme().colors.clone();
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

    let cursor_x =
        area.x
            .saturating_add(1)
            .saturating_add(1)
            .saturating_add(usize_to_u16_saturating(
                prompt.cursor_col.saturating_sub(horizontal_scroll),
            ));
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

pub(super) fn draw_command_palette(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let Some(palette_snapshot) = app.command_palette.as_ref() else {
        return;
    };
    let palette_query = palette_snapshot.query.clone();
    let palette_cursor_col = palette_snapshot.cursor_col;
    let palette_selected = palette_snapshot.selected_index;
    let palette_scroll = palette_snapshot.scroll;

    let root = frame.area();
    if root.width < 44 || root.height < 10 {
        return;
    }

    let width = root.width.saturating_sub(4).clamp(52, 96);
    let height = root.height.saturating_sub(4).clamp(10, 22);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let colors = app.theme().colors.clone();

    let all_items = TuiApp::command_palette_items();
    let filtered_items = TuiApp::command_palette_filtered_items(&palette_query, &all_items);
    let max_visible_rows = usize::from(area.height.saturating_sub(4)).max(1);
    let (selected_index, scroll) = if filtered_items.is_empty() {
        (0usize, 0usize)
    } else {
        let selected_index = palette_selected.min(filtered_items.len().saturating_sub(1));
        let mut scroll = palette_scroll;
        if selected_index < scroll {
            scroll = selected_index;
        } else if selected_index >= scroll.saturating_add(max_visible_rows) {
            scroll = selected_index.saturating_sub(max_visible_rows.saturating_sub(1));
        }
        (selected_index, scroll)
    };

    if let Some(palette) = app.command_palette.as_mut() {
        palette.selected_index = selected_index;
        palette.scroll = scroll;
    }

    let query_prefix = "Search: ";
    let query_width = usize::from(area.width.saturating_sub(2)).saturating_sub(query_prefix.len());
    let query_width = query_width.max(1);
    let query_scroll = palette_cursor_col.saturating_sub(query_width.saturating_sub(1));
    let visible_query = slice_chars(&palette_query, query_scroll, query_width);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            query_prefix,
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(visible_query, Style::default().fg(colors.text_primary)),
    ]));

    if filtered_items.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matching commands)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        for (offset, item) in filtered_items
            .iter()
            .enumerate()
            .skip(scroll)
            .take(max_visible_rows)
        {
            let is_selected = offset == selected_index;
            let mut style = Style::default().fg(colors.text_primary);
            let marker = if is_selected { "▶ " } else { "  " };
            if is_selected {
                style = style
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD);
            }
            lines.push(Line::from(Span::styled(
                format!("{marker}{}", item.label),
                style,
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "Enter run | Esc close | j/k move | type to filter",
        Style::default().fg(colors.status_help),
    )));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title("Command Palette")
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
        .saturating_add(usize_to_u16_saturating(query_prefix.len()))
        .saturating_add(usize_to_u16_saturating(
            palette_cursor_col.saturating_sub(query_scroll),
        ));
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}
