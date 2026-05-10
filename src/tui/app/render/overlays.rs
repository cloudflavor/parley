use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::super::helpers::{format_comment_reference, format_timestamp_utc, slice_chars};
use super::helpers::{compute_scroll, fit_to_width, wrap_markdown_lines};
use super::status::spinner_frame;
use super::{
    AiLogEvent, AiLogSessionStatus, CommandPromptMode, FileHeatmapSortMode, ThreadSelectorEntry,
    TuiApp,
};
use crate::git::history::FileHeatmapEntry;
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

pub(super) fn draw_thread_selector(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let colors = app.theme().colors.clone();
    let root = frame.area();
    if root.width < 52 || root.height < 10 {
        return;
    }

    let width = root.width.saturating_sub(4).clamp(72, 140);
    let height = root.height.saturating_sub(4).clamp(12, 28);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    app.last_thread_selector_area = Some(area);

    let inner = Block::default().borders(Borders::ALL).inner(area);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let input_area = vertical[0];
    let list_area = vertical[1];
    let footer_area = vertical[2];

    let entries = app.filtered_thread_selector_entries();
    let query = app
        .thread_selector
        .as_ref()
        .map(|selector| selector.query.clone())
        .unwrap_or_default();
    let selected_index = app
        .thread_selector
        .as_ref()
        .map_or(0, |selector| selector.selected_index)
        .min(entries.len().saturating_sub(1));
    if let Some(selector) = app.thread_selector.as_mut() {
        selector.selected_index = selected_index;
    }
    let visible_rows = usize::from(list_area.height).max(1);
    app.last_thread_selector_visible_rows = visible_rows;
    if let Some(selector) = app.thread_selector.as_mut() {
        if selector.selected_index < selector.scroll {
            selector.scroll = selector.selected_index;
        }
        if selector.selected_index >= selector.scroll.saturating_add(visible_rows) {
            selector.scroll = selector
                .selected_index
                .saturating_sub(visible_rows.saturating_sub(1));
        }
        let max_scroll = entries.len().saturating_sub(visible_rows);
        selector.scroll = selector.scroll.min(max_scroll);
        app.last_thread_selector_scroll = selector.scroll;
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .title("Thread Selector")
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
        Paragraph::new(vec![Line::from(vec![
            Span::styled("thread> ", Style::default().fg(colors.text_muted)),
            Span::styled(query, Style::default().fg(colors.text_primary)),
        ])]),
        input_area,
    );

    let mut lines = Vec::new();
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matching threads)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        let scroll = app
            .thread_selector
            .as_ref()
            .map_or(0, |selector| selector.scroll);
        let content_width = usize::from(list_area.width).max(1);
        for (index, entry) in entries.iter().enumerate().skip(scroll).take(visible_rows) {
            lines.push(thread_selector_line(
                entry,
                index == selected_index,
                content_width,
                &colors,
            ));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), list_area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Ctrl+t/Esc close | Enter jump | type filter | j/k/PgUp/PgDn select",
            Style::default().fg(colors.status_help),
        ))),
        footer_area,
    );

    if let Some(selector) = app.thread_selector.as_ref() {
        let cursor_x = input_area.x.saturating_add(8).saturating_add(
            usize_to_u16_saturating(selector.cursor_col).min(input_area.width.saturating_sub(9)),
        );
        frame.set_cursor_position((cursor_x, input_area.y));
    }
}

fn thread_selector_line(
    entry: &ThreadSelectorEntry,
    selected: bool,
    width: usize,
    colors: &ThemeColors,
) -> Line<'static> {
    let marker = if selected { ">" } else { " " };
    let text = fit_to_width(
        &format!(
            "{marker} #{} [{:?}] {}:{} - {}",
            entry.comment_id, entry.status, entry.file_path, entry.line_reference, entry.preview
        ),
        width,
    );
    let style = if selected {
        Style::default()
            .bg(colors.sidebar_highlight_bg)
            .fg(colors.sidebar_highlight_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_primary)
    };
    Line::from(Span::styled(text, style))
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

    let sessions = app
        .ai_log_sessions_for_file(&file_path)
        .cloned()
        .unwrap_or_default();
    let total = sessions
        .iter()
        .map(|session| session.events.len())
        .sum::<usize>();
    let mut lines = Vec::new();
    if total == 0 {
        lines.push(Line::from(Span::styled(
            "(no AI logs for this file yet)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        let content_width = usize::from(area.width.saturating_sub(2)).max(1);
        let mut wrapped_entries = Vec::new();
        for session in sessions.iter().rev() {
            let status_label = if matches!(session.status, AiLogSessionStatus::Running) {
                format!(
                    "{} {}",
                    session.status.as_str(),
                    spinner_frame(session.started_at)
                )
            } else {
                session.status.as_str().to_string()
            };
            let header = format!(
                "#{} {}:{} {} {}",
                session.id,
                session.provider.as_str(),
                session.mode.as_str(),
                status_label,
                format_timestamp_utc(session.started_at_ms),
            );
            wrapped_entries.extend(wrap_plain_styled_lines(
                &header,
                content_width,
                ai_session_status_style(session.status, &colors),
            ));
            for event in &session.events {
                let text = format_ai_event_line(event);
                wrapped_entries.extend(wrap_plain_styled_lines(
                    &text,
                    content_width,
                    ai_event_style(event, &colors),
                ));
            }
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

pub(super) fn draw_ai_activity_overlay(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let colors = app.theme().colors.clone();
    let root = frame.area();
    if root.width < 48 || root.height < 10 {
        return;
    }

    let width = root.width.saturating_sub(4).clamp(68, 140);
    let height = root.height.saturating_sub(4).clamp(12, 28);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    app.last_ai_activity_area = Some(area);

    let inner_height = usize::from(area.height.saturating_sub(2)).max(1);
    let list_rows = inner_height.saturating_sub(3).max(1);
    let content_width = usize::from(area.width.saturating_sub(2)).max(1);
    let entries = app.ai_activity_entries();
    let max_index = entries.len().saturating_sub(1);
    app.ai_activity_selected = app.ai_activity_selected.min(max_index);
    if app.ai_activity_selected < app.ai_activity_scroll {
        app.ai_activity_scroll = app.ai_activity_selected;
    }
    if app.ai_activity_selected >= app.ai_activity_scroll.saturating_add(list_rows) {
        app.ai_activity_scroll = app
            .ai_activity_selected
            .saturating_sub(list_rows.saturating_sub(1));
    }
    let max_scroll = entries.len().saturating_sub(list_rows);
    app.ai_activity_scroll = app.ai_activity_scroll.min(max_scroll);

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(
            "{} running | {} unread | Enter opens file session logs",
            app.ai_activity_running_count(),
            app.ai_activity_unread_count()
        ),
        Style::default().fg(colors.text_muted),
    )));

    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no AI sessions yet)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        for (index, entry) in entries
            .iter()
            .enumerate()
            .skip(app.ai_activity_scroll)
            .take(list_rows)
        {
            let marker = if index == app.ai_activity_selected {
                ">"
            } else {
                " "
            };
            let finished = entry
                .finished_at_ms
                .map(format_timestamp_utc)
                .unwrap_or_else(|| "active".to_string());
            let unread = if entry.unread_events > 0 {
                format!(" unread:{}", entry.unread_events)
            } else {
                String::new()
            };
            let last = entry
                .last_event
                .as_ref()
                .map(|event| format!(" - {}", event.message.trim()))
                .unwrap_or_default();
            let raw = format!(
                "{marker} #{} {}:{} {} events:{}{} {} -> {}{}",
                entry.session_id,
                entry.provider.as_str(),
                entry.mode.as_str(),
                entry.status.as_str(),
                entry.event_count,
                unread,
                finished,
                entry.file_path,
                last,
            );
            let line = fit_to_width(&raw, content_width);
            let mut style = ai_session_status_style(entry.status, &colors);
            if index == app.ai_activity_selected {
                style = style
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg);
            }
            lines.push(Line::from(Span::styled(line, style)));
        }
    }
    lines.push(Line::from(Span::styled(
        "L/Esc close | Enter jump | j/k/PgUp/PgDn/Home/End select",
        Style::default().fg(colors.status_help),
    )));
    lines.push(Line::from(Span::styled(
        format!("runtime log: {}", app.log_path.display()),
        Style::default().fg(colors.text_muted),
    )));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("AI Activity")
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

fn format_ai_event_line(event: &AiLogEvent) -> String {
    format!(
        "[{}] {}: {}",
        format_timestamp_utc(event.timestamp_ms),
        event.stream,
        event.message.trim()
    )
}

fn ai_event_style(event: &AiLogEvent, colors: &ThemeColors) -> Style {
    match event.stream.as_str() {
        "stderr" => Style::default().fg(colors.removed_sign),
        "system" => Style::default().fg(colors.accent),
        "tool" => Style::default().fg(colors.added_sign),
        "plan" => Style::default().fg(colors.accent),
        "thought" => Style::default().fg(colors.text_muted),
        _ => Style::default().fg(colors.text_primary),
    }
}

fn ai_session_status_style(status: AiLogSessionStatus, colors: &ThemeColors) -> Style {
    match status {
        AiLogSessionStatus::Running => Style::default().fg(colors.accent),
        AiLogSessionStatus::Finished => Style::default().fg(colors.added_sign),
        AiLogSessionStatus::Failed => Style::default().fg(colors.removed_sign),
        AiLogSessionStatus::Cancelled => Style::default().fg(colors.text_muted),
    }
}

pub(super) fn draw_file_heatmap_overlay(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let colors = app.theme().colors.clone();
    let root = frame.area();
    if root.width < 60 || root.height < 12 {
        return;
    }

    let width = root.width.saturating_sub(4).clamp(72, 132);
    let height = root.height.saturating_sub(4).clamp(12, 32);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    app.last_file_heatmap_area = Some(area);
    let inner_height = usize::from(area.height.saturating_sub(2)).max(1);
    let content_width = usize::from(area.width.saturating_sub(2)).max(1);
    let list_rows = inner_height.saturating_sub(3).max(1);
    let loading = app.file_heatmap_is_loading();
    let (entries, raw_scroll, loaded_at, sort_mode, sort_label, sort_direction) = app
        .file_heatmap
        .as_ref()
        .map(|heatmap| {
            (
                heatmap.entries.clone(),
                heatmap.scroll,
                heatmap.loaded_at.is_some(),
                heatmap.sort_mode,
                heatmap.sort_mode.label(),
                heatmap.sort_direction_label(),
            )
        })
        .unwrap_or_else(|| {
            (
                Vec::new(),
                0,
                false,
                FileHeatmapSortMode::Churn,
                "churn",
                "desc",
            )
        });
    let max_scroll = entries.len().saturating_sub(list_rows);
    let scroll = raw_scroll.min(max_scroll);
    if let Some(heatmap) = app.file_heatmap.as_mut() {
        heatmap.scroll = scroll;
    }

    let title = if loading {
        app.file_heatmap_started_at.map_or_else(
            || "Git File Heatmap".to_string(),
            |started_at| {
                format!(
                    "Git File Heatmap {} scanning history",
                    spinner_frame(started_at)
                )
            },
        )
    } else if loaded_at {
        format!("Git File Heatmap sort: {sort_label} {sort_direction}")
    } else {
        "Git File Heatmap".to_string()
    };

    let mut lines = Vec::new();
    if entries.is_empty() {
        let message = if loading {
            "Scanning git history for touched-file hotspots..."
        } else {
            "No git file history found."
        };
        lines.push(Line::from(Span::styled(
            message,
            Style::default().fg(colors.text_muted),
        )));
    } else {
        let max_heat = entries
            .iter()
            .map(|entry| heatmap_metric_value(entry, sort_mode))
            .max()
            .unwrap_or(1);
        lines.push(Line::from(Span::styled(
            format!(
                "sort: {sort_label} {sort_direction} | heat follows sort metric: red hottest, blue high, green medium, gray low"
            ),
            Style::default().fg(colors.text_muted),
        )));
        lines.push(file_heatmap_header(&colors));
        for (index, entry) in entries.iter().enumerate().skip(scroll).take(list_rows) {
            lines.push(file_heatmap_line(
                index + 1,
                entry,
                sort_mode,
                max_heat,
                content_width,
                &colors,
            ));
        }
    }
    lines.push(Line::from(""));
    let footer = if loaded_at {
        "M/Esc close | s sort | S reverse | j/k/PgUp/PgDn scroll | one square per file"
    } else {
        "M/Esc close | scanning runs only on explicit request"
    };
    lines.push(Line::from(Span::styled(
        footer,
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

fn file_heatmap_line(
    rank: usize,
    entry: &FileHeatmapEntry,
    sort_mode: FileHeatmapSortMode,
    max_heat: isize,
    width: usize,
    colors: &ThemeColors,
) -> Line<'static> {
    let prefix = format!(
        "{rank:>4} {heat_width} {changes:>8} {commits:>7} +{insertions}/-{deletions} ",
        heat_width = "  ",
        changes = entry.changes,
        commits = entry.commits,
        insertions = entry.insertions,
        deletions = entry.deletions,
    );
    let remaining = width.saturating_sub(prefix.chars().count()).max(8);
    let path = fit_to_width(&entry.path, remaining);
    let mut spans = vec![Span::styled(
        format!("{rank:>4} "),
        Style::default().fg(colors.text_muted),
    )];
    spans.push(heatmap_cell(
        heatmap_metric_value(entry, sort_mode),
        max_heat,
        colors,
    ));
    spans.push(Span::raw(format!(
        " {changes:>8} {commits:>7} +{insertions}/-{deletions} ",
        changes = entry.changes,
        commits = entry.commits,
        insertions = entry.insertions,
        deletions = entry.deletions,
    )));
    spans.push(Span::styled(path, Style::default().fg(colors.text_primary)));
    Line::from(spans)
}

fn heatmap_metric_value(entry: &FileHeatmapEntry, sort_mode: FileHeatmapSortMode) -> isize {
    match sort_mode {
        FileHeatmapSortMode::Churn => entry.changes as isize,
        FileHeatmapSortMode::Added => entry.insertions as isize,
        FileHeatmapSortMode::Removed => entry.deletions as isize,
        FileHeatmapSortMode::Commits => entry.commits as isize,
        FileHeatmapSortMode::NetGrowth => entry.insertions as isize - entry.deletions as isize,
        FileHeatmapSortMode::NetShrink => entry.deletions as isize - entry.insertions as isize,
        FileHeatmapSortMode::Volatility => entry.changes.saturating_mul(entry.commits) as isize,
        FileHeatmapSortMode::Path => entry.changes as isize,
    }
}

fn file_heatmap_header(colors: &ThemeColors) -> Line<'static> {
    Line::from(vec![
        Span::styled("rank ", Style::default().fg(colors.text_muted)),
        Span::styled("heat ", Style::default().fg(colors.text_muted)),
        Span::styled(" changes ", Style::default().fg(colors.text_muted)),
        Span::styled("commits ", Style::default().fg(colors.text_muted)),
        Span::styled("+/- ", Style::default().fg(colors.text_muted)),
        Span::styled("path", Style::default().fg(colors.text_muted)),
    ])
}

fn heatmap_cell(value: isize, max_value: isize, colors: &ThemeColors) -> Span<'static> {
    Span::styled(
        "  ".to_string(),
        heat_cell_style(heat_level(value, max_value), colors),
    )
}

fn heat_level(value: isize, max_value: isize) -> usize {
    if value <= 0 || max_value <= 0 {
        return 0;
    }

    let ratio = value as f64 / max_value as f64;
    if ratio >= 0.75 {
        4
    } else if ratio >= 0.40 {
        3
    } else if ratio >= 0.15 {
        2
    } else {
        1
    }
}

fn heat_cell_style(level: usize, colors: &ThemeColors) -> Style {
    let bg = match level {
        0 => colors.thread_background,
        1 => Color::DarkGray,
        2 => Color::Green,
        3 => Color::Blue,
        _ => Color::Red,
    };
    Style::default().bg(bg).fg(bg)
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

pub(super) fn draw_code_search(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let Some(search_snapshot) = app.code_search.as_ref() else {
        return;
    };
    let query = search_snapshot.query.clone();
    let cursor_col = search_snapshot.cursor_col;
    let selected_index = search_snapshot.selected_index;
    let scroll = search_snapshot.scroll;
    let message = search_snapshot.message.clone();
    let results = search_snapshot.results.clone();
    let engine = search_snapshot.engine;

    let root = frame.area();
    if root.width < 48 || root.height < 10 {
        return;
    }

    let width = root.width.saturating_sub(4).clamp(56, 112);
    let height = root.height.saturating_sub(4).clamp(10, 24);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let colors = app.theme().colors.clone();
    let query_prefix = "/ ";
    let query_width = usize::from(area.width.saturating_sub(2)).saturating_sub(query_prefix.len());
    let query_width = query_width.max(1);
    let query_scroll = cursor_col.saturating_sub(query_width.saturating_sub(1));
    let visible_query = slice_chars(&query, query_scroll, query_width);
    let visible_rows = usize::from(area.height.saturating_sub(5)).max(1);
    let selected_index = selected_index.min(results.len().saturating_sub(1));
    let mut scroll = scroll;
    if !results.is_empty() {
        if selected_index < scroll {
            scroll = selected_index;
        } else if selected_index >= scroll.saturating_add(visible_rows) {
            scroll = selected_index.saturating_sub(visible_rows.saturating_sub(1));
        }
    }
    if let Some(search) = app.code_search.as_mut() {
        search.selected_index = selected_index;
        search.scroll = scroll;
    }
    app.last_code_search_area = Some(area);
    app.last_code_search_scroll = scroll;
    app.last_code_search_visible_rows = visible_rows;

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
    lines.push(Line::from(Span::styled(
        message,
        Style::default().fg(colors.status_help),
    )));

    if results.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matches)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
        for (idx, result) in results.iter().enumerate().skip(scroll).take(visible_rows) {
            let is_selected = idx == selected_index;
            let marker = if is_selected { "▶ " } else { "  " };
            let row = format!(
                "{marker}{}:{}:{} {}",
                result.path,
                result.line,
                result.column,
                result.text.trim()
            );
            let style = if is_selected {
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(code_search_filetype_color(&result.path, &colors))
            };
            lines.push(Line::from(Span::styled(
                fit_to_width(&row, inner_width),
                style,
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        "Enter open | Esc close | ↑/↓ move | type updates live | rg with grep fallback",
        Style::default().fg(colors.status_help),
    )));

    frame.render_widget(Clear, area);
    let title = code_search_title(results.len(), engine);
    frame.render_widget(
        Paragraph::new(lines).block(
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
        .saturating_add(usize_to_u16_saturating(query_prefix.len()))
        .saturating_add(usize_to_u16_saturating(
            cursor_col.saturating_sub(query_scroll),
        ));
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn code_search_title(result_count: usize, engine: Option<&'static str>) -> String {
    let match_label = if result_count == 1 {
        "match"
    } else {
        "matches"
    };
    match engine {
        Some(engine) => format!("Code Search - {result_count} {match_label} via {engine}"),
        None => "Code Search".to_string(),
    }
}

fn code_search_filetype_color(path: &str, colors: &ThemeColors) -> Color {
    let extension = path.rsplit('.').next().unwrap_or_default();
    match extension {
        "rs" => Color::Rgb(224, 142, 80),
        "ts" | "tsx" => Color::Rgb(93, 173, 226),
        "js" | "jsx" | "mjs" | "cjs" => Color::Rgb(240, 220, 96),
        "py" => Color::Rgb(99, 196, 132),
        "go" => Color::Rgb(96, 190, 210),
        "md" | "markdown" => Color::Rgb(190, 150, 240),
        "toml" | "yaml" | "yml" | "json" => colors.accent,
        _ => colors.text_primary,
    }
}

#[cfg(test)]
mod heatmap_tests {
    use super::*;

    #[test]
    fn file_heatmap_line_places_one_heat_cell_before_metrics() -> anyhow::Result<()> {
        let entry = FileHeatmapEntry {
            path: "src/hot.rs".to_string(),
            commits: 2,
            changes: 4,
            insertions: 3,
            deletions: 1,
        };
        let themes = crate::tui::theme::load_themes()?;
        let colors = &themes[0].colors;

        let line = file_heatmap_line(1, &entry, FileHeatmapSortMode::Churn, 4, 80, colors);

        assert_eq!(line.spans.len(), 4);
        Ok(())
    }

    #[test]
    fn heat_level_bands_churn_by_ratio_to_hottest_file() {
        assert_eq!(heat_level(0, 10), 0);
        assert_eq!(heat_level(1, 10), 1);
        assert_eq!(heat_level(2, 10), 2);
        assert_eq!(heat_level(4, 10), 3);
        assert_eq!(heat_level(10, 10), 4);
    }

    #[test]
    fn heatmap_metric_value_follows_sort_mode() {
        let entry = FileHeatmapEntry {
            path: "src/hot.rs".to_string(),
            commits: 3,
            changes: 10,
            insertions: 7,
            deletions: 3,
        };

        assert_eq!(heatmap_metric_value(&entry, FileHeatmapSortMode::Added), 7);
        assert_eq!(
            heatmap_metric_value(&entry, FileHeatmapSortMode::Volatility),
            30
        );
        assert_eq!(
            heatmap_metric_value(&entry, FileHeatmapSortMode::NetShrink),
            -4
        );
    }

    #[test]
    fn code_search_title_includes_result_count_and_engine() {
        assert_eq!(
            code_search_title(1, Some("rg")),
            "Code Search - 1 match via rg"
        );
        assert_eq!(
            code_search_title(12, Some("grep")),
            "Code Search - 12 matches via grep"
        );
    }
}
