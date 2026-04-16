use std::{collections::HashSet, time::Instant};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::domain::{
    diff::DiffLineKind,
    review::{CommentStatus, ReviewState},
};

use super::super::theme::ThemeColors;
use super::helpers::{
    comment_matches_display_row, format_line_reference, format_timestamp_utc, slice_chars,
};
use super::{CommandPromptMode, DiffPane, DisplayRow, InlineDraftMode, SettingsEditorKind, TuiApp};

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let root = frame.area();
    app.last_shortcuts_modal_area = None;
    app.last_thread_nav_area = None;
    app.last_thread_nav_scroll = 0;
    app.last_thread_nav_row_map.clear();
    app.last_diff_area_secondary = None;
    app.last_diff_scroll_secondary = 0;
    app.last_diff_row_map_secondary.clear();
    let status_height = compute_status_height(root.height);

    if app.content_fullscreen {
        app.last_file_area = None;
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(status_height)])
            .split(root);
        draw_diff_view_for_pane(frame, app, sections[0], app.active_diff_pane);
        draw_status_panel(frame, app, sections[1]);
        if app.thread_nav_visible {
            draw_thread_navigator_overlay(frame, app);
        }
        if app.settings_editor.is_some() {
            draw_settings_editor(frame, app);
        }
        if app.command_prompt.is_some() {
            draw_command_prompt(frame, app);
        }
        if app.ai_progress_visible {
            draw_ai_progress_popup(frame, app);
        }
        if app.shortcuts_modal_visible {
            draw_shortcuts_modal(frame, app);
        }
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(status_height)])
        .split(root);

    let file_pane_width = app.computed_file_pane_width(sections[0].width);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(file_pane_width), Constraint::Min(0)])
        .split(sections[0]);

    draw_file_sidebar(frame, app, columns[0]);
    if app.split_diff_view {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[1]);
        draw_diff_view_for_pane(frame, app, panes[0], DiffPane::Primary);
        draw_diff_view_for_pane(frame, app, panes[1], DiffPane::Secondary);
    } else {
        draw_diff_view_for_pane(frame, app, columns[1], app.active_diff_pane);
    }
    draw_status_panel(frame, app, sections[1]);
    if app.thread_nav_visible {
        draw_thread_navigator_overlay(frame, app);
    }
    if app.settings_editor.is_some() {
        draw_settings_editor(frame, app);
    }
    if app.command_prompt.is_some() {
        draw_command_prompt(frame, app);
    }
    if app.ai_progress_visible {
        draw_ai_progress_popup(frame, app);
    }
    if app.shortcuts_modal_visible {
        draw_shortcuts_modal(frame, app);
    }
}

fn compute_status_height(total_height: u16) -> u16 {
    if total_height >= 24 {
        8
    } else if total_height >= 16 {
        7
    } else if total_height >= 10 {
        6
    } else {
        4
    }
}

fn draw_thread_navigator_overlay(frame: &mut Frame<'_>, app: &mut TuiApp) {
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
                format_line_reference(comment.old_line, comment.new_line),
                preview
            );
            let style = if index == app.selected_comment {
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.text_primary)
            };
            lines.push(Line::from(Span::styled(line, style)));
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
            .wrap(Wrap { trim: true })
            .scroll((scroll as u16, 0)),
        area,
    );
}

fn draw_ai_progress_popup(frame: &mut Frame<'_>, app: &TuiApp) {
    let colors = &app.theme().colors;
    let root = frame.area();
    if root.width < 40 || root.height < 10 {
        return;
    }

    let width = root.width.clamp(44, 120);
    let height = root.height.clamp(8, 18);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + 1,
        width,
        height,
    };

    let title = if let Some(task) = app.ai_task.as_ref() {
        format!(
            "AI Stream {} {}:{}",
            spinner_frame(task.started_at),
            task.provider.as_str(),
            task.mode.as_str()
        )
    } else {
        "AI Stream".to_string()
    };

    let inner_height = area.height.saturating_sub(2) as usize;
    if inner_height == 0 {
        return;
    }
    let log_rows = inner_height.saturating_sub(1).max(1);

    let total = app.ai_progress_lines.len();
    let mut lines = Vec::new();
    if total == 0 {
        lines.push(Line::from(Span::styled(
            "(no AI progress yet)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        let content_width = usize::from(area.width.saturating_sub(2)).max(1);
        let mut wrapped_entries = Vec::new();
        for entry in &app.ai_progress_lines {
            let style = if entry.contains(" stderr: ") {
                Style::default().fg(colors.removed_sign)
            } else if entry.contains(" system: ") {
                Style::default().fg(colors.accent)
            } else {
                Style::default().fg(colors.text_primary)
            };
            wrapped_entries.extend(wrap_plain_styled_lines(entry, content_width, style));
        }
        let start = wrapped_entries.len().saturating_sub(log_rows);
        lines.extend(wrapped_entries.into_iter().skip(start));
    }
    lines.push(Line::from(Span::styled(
        "H hide/show | K cancel run | L open full logs",
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

fn draw_shortcuts_modal(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let root = frame.area();
    let width = root.width.saturating_sub(2).min(132).max(64);
    let height = root.height.saturating_sub(2).min(30).max(16);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    app.last_shortcuts_modal_area = Some(area);
    let colors = app.theme().colors.clone();

    let markdown = "\
# PARLAR QUICK KEYS\n\
> `Esc`, `?`, or `F1` closes this pane.\n\
\n\
## Move\n\
- **Files:** `h/l`\n\
- **Lines:** `j/k`\n\
- **Top/Bottom:** `g/G`\n\
- **Go to line:** `:<line>`\n\
- **Search:** `/query`, then `n/p`\n\
\n\
## Threads\n\
- **New comment:** `m` or `c`\n\
- **Reply:** `r`\n\
- **Next/Prev thread:** `N/P`\n\
- **Thread list select:** `[/]`\n\
- **Thread state:** `a` addressed, `o` open\n\
\n\
## Review\n\
- **Set Pending:** `s`\n\
- **Set Waiting:** `w`\n\
- **Set Done:** `d`\n\
\n\
## Layout\n\
- **Fullscreen:** `z`\n\
- **Split view:** `V`\n\
- **Side-by-side diff:** `S`\n\
- **Active pane:** `Tab`\n\
- **Files pane width:** `</>`\n\
- **Thread navigator:** `b`\n\
\n\
## AI\n\
- **Refactor thread/review:** `x` / `A`\n\
- **Reply in thread:** `X`\n\
- **Cancel run:** `K`\n\
- **AI stream popup:** `H`\n\
- **Open logs in `less`:** `L`\n\
- **Refresh review + diff:** `R`\n\
\n\
## Settings\n\
- **User name:** `u`\n\
- **AI provider:** `v`\n\
- **Theme cycle/toggle:** `t/T`\n\
";
    let mut lines = wrap_markdown_lines(
        markdown,
        usize::from(area.width.saturating_sub(2)).max(1),
        &colors,
    );
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tip: key bindings are case-sensitive (for example `n` vs `N`).",
        Style::default().fg(colors.status_help),
    )));
    let content_height = usize::from(area.height.saturating_sub(2)).max(1);
    let max_scroll = lines.len().saturating_sub(content_height);
    let scroll = app.shortcuts_modal_scroll.min(max_scroll);
    app.shortcuts_modal_scroll = scroll;

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("  Help  ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors.thread_border))
                    .title_style(
                        Style::default()
                            .fg(colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0)),
        area,
    );
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
    let search_query = app.search_query.as_deref();

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
                    return ListItem::new(Line::from(search_highlighted_text_spans(
                        &file.path,
                        search_query,
                        &colors,
                    )));
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
                let mut spans = vec![Span::styled(marker_text, marker_style)];
                spans.extend(search_highlighted_text_spans(
                    &file.path,
                    search_query,
                    &colors,
                ));

                ListItem::new(Line::from(spans))
            })
            .collect()
    };

    let mut state = ListState::default();
    if !app.diff.files.is_empty() {
        state.select(Some(app.active_file_index()));
        app.last_file_scroll = compute_scroll(
            app.active_file_index(),
            area.height.saturating_sub(2) as usize,
        );
    } else {
        app.last_file_scroll = 0;
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title("Files")
                .borders(Borders::TOP | Borders::LEFT)
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

fn draw_diff_view_for_pane(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect, pane: DiffPane) {
    let colors = app.theme().colors.clone();
    let is_active = !app.split_diff_view || pane == app.active_diff_pane;
    let file_index = match pane {
        DiffPane::Primary => app.selected_file,
        DiffPane::Secondary => app.secondary_selected_file,
    };
    if pane == DiffPane::Primary {
        app.last_diff_area = Some(area);
    } else {
        app.last_diff_area_secondary = Some(area);
    }

    let Some(file_path) = app.diff.files.get(file_index).map(|file| file.path.clone()) else {
        let title = if pane == DiffPane::Primary {
            "Diff A"
        } else {
            "Diff B"
        };
        let borders = diff_pane_borders(app.split_diff_view, pane);
        frame.render_widget(
            Paragraph::new("No git changes against HEAD.").block(
                Block::default()
                    .title(title)
                    .borders(borders)
                    .border_style(Style::default().fg(colors.thread_border))
                    .title_style(
                        Style::default()
                            .fg(colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
            ),
            area,
        );
        if pane == DiffPane::Primary {
            app.last_diff_scroll = 0;
            app.last_diff_row_map.clear();
        } else {
            app.last_diff_scroll_secondary = 0;
            app.last_diff_row_map_secondary.clear();
        }
        return;
    };

    app.ensure_row_cache_for_file(file_index);
    let Some((rows, highlights)) = app.rows_and_highlights_for_file(file_index) else {
        return;
    };

    let selected_line = app.line_for_pane(pane);
    let file_comments = app.comments_for_file(&file_path);
    let mut lines = Vec::new();
    let mut row_map = Vec::new();
    let mut selected_visual_index = 0usize;
    let mut rendered_comment_ids = HashSet::new();

    for (index, row) in rows.iter().enumerate() {
        if index == selected_line {
            selected_visual_index = lines.len();
        }
        let is_selected = index == selected_line;
        let pane_inner_width = usize::from(area.width.saturating_sub(2)).max(1);
        let highlighted_segments =
            apply_search_highlighting(&highlights[index], app.search_query.as_deref(), &colors);
        let rendered = if app.side_by_side_diff
            && matches!(
                row.kind,
                DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
            ) {
            build_side_by_side_row_lines(
                row,
                &highlighted_segments,
                is_selected,
                is_active,
                pane_inner_width,
                &colors,
            )
        } else {
            build_unified_row_lines(
                row,
                &highlighted_segments,
                is_selected,
                is_active,
                pane_inner_width,
                &colors,
            )
        };
        for line in rendered {
            lines.push(line);
            row_map.push(index);
        }

        for comment in file_comments
            .iter()
            .copied()
            .filter(|comment| comment_matches_display_row(comment, row))
        {
            rendered_comment_ids.insert(comment.id);
            let status = comment_status_label(&comment.status);
            let review_state = review_state_label(&app.review.state);
            let pane_inner_width = usize::from(area.width.saturating_sub(2));
            let inner_width = compute_thread_inner_width(pane_inner_width, 12);
            let comment_title_prefix = format!("comment #{} [", comment.id);
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
                    title_prefix: &comment_title_prefix,
                    title_status: Some(status),
                    title_suffix: &format!(" | review: {review_state}]"),
                    title_status_style: Some(comment_status_style(&comment.status, &colors)),
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
                        title_prefix: &reply_title,
                        title_status: None,
                        title_suffix: "",
                        title_status_style: None,
                        body: &reply.body,
                        border_color: colors.thread_border,
                        title_color: colors.reply_title,
                        colors: &colors,
                    },
                );
            }
        }
    }

    let fallback_source_row_index = selected_line.min(rows.len().saturating_sub(1));
    let pane_inner_width = usize::from(area.width.saturating_sub(2));
    for comment in file_comments
        .iter()
        .copied()
        .filter(|comment| !rendered_comment_ids.contains(&comment.id))
    {
        let status = comment_status_label(&comment.status);
        let review_state = review_state_label(&app.review.state);
        let inner_width = compute_thread_inner_width(pane_inner_width, 12);
        let comment_title_prefix = format!("comment #{} [", comment.id);
        let comment_header = format!(
            "{} | {} | anchor {} not in current diff",
            app.author_label(&comment.author),
            format_timestamp_utc(comment.created_at_ms),
            format_line_reference(comment.old_line, comment.new_line)
        );
        push_thread_box(
            &mut lines,
            &mut row_map,
            ThreadBoxSpec {
                source_row_index: fallback_source_row_index,
                indent: 12,
                inner_width,
                header: &comment_header,
                title_prefix: &comment_title_prefix,
                title_status: Some(status),
                title_suffix: &format!(" | review: {review_state}]"),
                title_status_style: Some(comment_status_style(&comment.status, &colors)),
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
                    source_row_index: fallback_source_row_index,
                    indent: 14,
                    inner_width: compute_thread_inner_width(pane_inner_width, 14),
                    header: &reply_header,
                    title_prefix: &reply_title,
                    title_status: None,
                    title_suffix: "",
                    title_status_style: None,
                    body: &reply.body,
                    border_color: colors.thread_border,
                    title_color: colors.reply_title,
                    colors: &colors,
                },
            );
        }
    }

    let viewport_height = area.height.saturating_sub(2) as usize;
    let scroll = compute_scroll(selected_visual_index, viewport_height);
    if pane == DiffPane::Primary {
        app.last_diff_scroll = scroll;
        app.last_diff_row_map = row_map;
    } else {
        app.last_diff_scroll_secondary = scroll;
        app.last_diff_row_map_secondary = row_map;
    }

    let title = format!(
        "Diff {}{}: {}",
        if pane == DiffPane::Primary { "A" } else { "B" },
        if is_active { "*" } else { "" },
        file_path
    );
    let borders = diff_pane_borders(app.split_diff_view, pane);
    let border_color = if is_active {
        colors.accent
    } else {
        colors.thread_border
    };
    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(title)
                .borders(borders)
                .border_style(Style::default().fg(border_color))
                .title_style(
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .scroll((scroll as u16, 0));
    frame.render_widget(widget, area);

    if is_active && app.inline_comment.is_some() {
        draw_inline_comment_editor(frame, app, area);
    }
}

fn diff_pane_borders(split: bool, pane: DiffPane) -> Borders {
    if !split {
        return Borders::TOP | Borders::LEFT | Borders::RIGHT;
    }
    match pane {
        DiffPane::Primary => Borders::TOP | Borders::LEFT | Borders::RIGHT,
        DiffPane::Secondary => Borders::TOP | Borders::RIGHT,
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
            format_editor_line_reference(target.old_line, target.new_line),
        ),
        InlineDraftMode::Reply {
            comment_id,
            old_line,
            new_line,
            ..
        } => (
            format!("Reply Box #{}", comment_id),
            format_editor_line_reference(*old_line, *new_line),
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

fn format_editor_line_reference(old_line: Option<u32>, new_line: Option<u32>) -> String {
    match (old_line, new_line) {
        (Some(old), Some(new)) => format!("{old}:{new}"),
        (Some(old), None) => format!("{old} (left)"),
        (None, Some(new)) => format!("{new} (right)"),
        (None, None) => "-".to_string(),
    }
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
            let mut spans = vec![Span::styled(
                "> ",
                Style::default().fg(colors.markdown_quote_mark),
            )];
            for span in parse_inline_markdown(stripped, colors) {
                spans.push(Span::styled(
                    span.content.into_owned(),
                    span.style
                        .fg(colors.markdown_quote_text)
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            rendered.push(Line::from(spans));
            continue;
        }

        if raw_line.starts_with("- ") || raw_line.starts_with("* ") {
            let mut spans = vec![Span::styled("• ", Style::default().fg(colors.markdown_bullet))];
            spans.extend(parse_inline_markdown(&raw_line[2..], colors));
            rendered.push(Line::from(spans));
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
fn compute_thread_inner_width(pane_inner_width: usize, indent: usize) -> usize {
    // Each thread line uses: indent + "│ " + content + " │"
    let available = pane_inner_width.saturating_sub(indent + 4);
    available.clamp(THREAD_BOX_MIN_CONTENT_WIDTH, THREAD_BOX_MAX_CONTENT_WIDTH)
}

struct ThreadBoxSpec<'a> {
    source_row_index: usize,
    indent: usize,
    inner_width: usize,
    header: &'a str,
    title_prefix: &'a str,
    title_status: Option<&'a str>,
    title_suffix: &'a str,
    title_status_style: Option<Style>,
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

    let mut title_spans = vec![Span::styled(spec.title_prefix.to_string(), title_style)];
    if let Some(status) = spec.title_status {
        title_spans.push(Span::styled(
            status.to_string(),
            spec.title_status_style.unwrap_or(title_style),
        ));
    }
    if !spec.title_suffix.is_empty() {
        title_spans.push(Span::styled(spec.title_suffix.to_string(), title_style));
    }
    let title_spans = fit_spans_to_width(title_spans, spec.inner_width, title_style);
    let mut title_row_spans = vec![
        Span::styled(indent_str.clone(), indent),
        Span::styled("│ ".to_string(), border),
    ];
    title_row_spans.extend(title_spans);
    title_row_spans.push(Span::styled(" │".to_string(), border));
    lines.push(Line::from(title_row_spans));
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

fn fit_spans_to_width(spans: Vec<Span<'static>>, width: usize, pad_style: Style) -> Vec<Span<'static>> {
    let mut styled_chars: Vec<(Style, char)> = Vec::new();
    for span in spans {
        for ch in span.content.chars() {
            styled_chars.push((span.style, ch));
        }
    }

    if styled_chars.len() > width {
        styled_chars.truncate(width);
    }

    let mut line = line_from_styled_chars(&styled_chars);
    let rendered_width: usize = line.spans.iter().map(|span| span.content.chars().count()).sum();
    if rendered_width < width {
        line.spans.push(Span::styled(" ".repeat(width - rendered_width), pad_style));
    }
    line.spans
}

fn apply_search_highlighting(
    segments: &[(Style, String)],
    query: Option<&str>,
    colors: &ThemeColors,
) -> Vec<(Style, String)> {
    let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) else {
        return segments.to_vec();
    };
    if segments.is_empty() {
        return Vec::new();
    }

    let line_text: String = segments.iter().map(|(_, text)| text.as_str()).collect();
    let ranges = find_case_insensitive_match_ranges(&line_text, query);
    if ranges.is_empty() {
        return segments.to_vec();
    }

    let mut styled_chars: Vec<(Style, char)> = Vec::new();
    for (style, text) in segments {
        for ch in text.chars() {
            styled_chars.push((*style, ch));
        }
    }

    let mut range_index = 0usize;
    for (char_index, (style, _)) in styled_chars.iter_mut().enumerate() {
        while range_index < ranges.len() && char_index >= ranges[range_index].1 {
            range_index += 1;
        }
        if range_index >= ranges.len() {
            break;
        }
        let (start, end) = ranges[range_index];
        if char_index >= start && char_index < end {
            let mut next = (*style)
                .bg(colors.sidebar_highlight_bg)
                .add_modifier(Modifier::BOLD);
            if next.fg.is_none() {
                next = next.fg(colors.sidebar_highlight_fg);
            }
            *style = next;
        }
    }

    line_from_styled_chars(&styled_chars)
        .spans
        .into_iter()
        .map(|span| (span.style, span.content.to_string()))
        .collect()
}

fn find_case_insensitive_match_ranges(input: &str, query: &str) -> Vec<(usize, usize)> {
    let haystack = input.to_lowercase();
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = haystack[offset..].find(&needle) {
        let start_byte = offset + found;
        let end_byte = start_byte + needle.len();
        let start_char = haystack[..start_byte].chars().count();
        let end_char = haystack[..end_byte].chars().count();
        ranges.push((start_char, end_char));
        offset = end_byte;
    }
    ranges
}

fn search_highlighted_text_spans(
    text: &str,
    query: Option<&str>,
    colors: &ThemeColors,
) -> Vec<Span<'static>> {
    apply_search_highlighting(&[(Style::default(), text.to_string())], query, colors)
        .into_iter()
        .map(|(style, text)| Span::styled(text, style))
        .collect()
}

fn build_unified_row_lines(
    row: &DisplayRow,
    highlighted_segments: &[(Style, String)],
    is_selected: bool,
    is_active: bool,
    pane_inner_width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    let marker_style = if is_selected {
        Style::default()
            .fg(colors.selection_marker)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    };
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

    let content = match row.kind {
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context => {
            styled_segments_line(
                highlighted_segments,
                Style::default().fg(colors.text_primary),
            )
        }
        DiffLineKind::HunkHeader => Line::from(Span::styled(
            row.raw.clone(),
            Style::default()
                .fg(colors.hunk_header)
                .add_modifier(Modifier::BOLD),
        )),
        DiffLineKind::Meta => Line::from(Span::styled(
            row.raw.clone(),
            Style::default().fg(colors.meta),
        )),
    };

    let prefix_width = 15usize;
    let content_width = pane_inner_width.saturating_sub(prefix_width).max(1);
    let wrapped_content = wrap_styled_line(&content, content_width);
    let wrapped_content = if wrapped_content.is_empty() {
        vec![blank_line(
            content_width,
            Style::default().fg(colors.text_primary),
        )]
    } else {
        wrapped_content
            .into_iter()
            .map(|line| {
                pad_line_to_width(
                    line,
                    content_width,
                    Style::default().fg(colors.text_primary),
                )
            })
            .collect()
    };

    let mut out = Vec::with_capacity(wrapped_content.len());
    for (visual_index, content_line) in wrapped_content.into_iter().enumerate() {
        let mut spans = Vec::new();
        spans.push(Span::styled(
            if visual_index == 0 && is_selected {
                "▌"
            } else {
                " "
            },
            marker_style,
        ));
        spans.push(Span::styled(
            if visual_index == 0 { sign } else { " " },
            sign_style,
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            if visual_index == 0 {
                format!("{:>5} {:>5}  ", old, new)
            } else {
                " ".repeat(12)
            },
            Style::default().fg(colors.text_muted),
        ));
        spans.extend(content_line.spans.into_iter());

        let line = if is_selected && is_active {
            Line::from(spans).patch_style(
                Style::default()
                    .bg(colors.selected_line_bg)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Line::from(spans)
        };
        out.push(line);
    }
    out
}

fn build_side_by_side_row_lines(
    row: &DisplayRow,
    highlighted_segments: &[(Style, String)],
    is_selected: bool,
    is_active: bool,
    pane_inner_width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    let marker_style = if is_selected {
        Style::default()
            .fg(colors.selection_marker)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    };

    let old = row
        .old_line
        .map(|value| value.to_string())
        .unwrap_or_else(|| " ".to_string());
    let new = row
        .new_line
        .map(|value| value.to_string())
        .unwrap_or_else(|| " ".to_string());

    let fixed_cols = 20usize;
    let code_cols = pane_inner_width.saturating_sub(fixed_cols).max(2);
    let left_width = (code_cols / 2).max(1);
    let right_width = (code_cols - left_width).max(1);

    let left_sign_style = if matches!(row.kind, DiffLineKind::Removed) {
        Style::default()
            .fg(colors.removed_sign)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    };
    let right_sign_style = if matches!(row.kind, DiffLineKind::Added) {
        Style::default()
            .fg(colors.added_sign)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    };

    let left_segments: &[(Style, String)] =
        if matches!(row.kind, DiffLineKind::Removed | DiffLineKind::Context) {
            highlighted_segments
        } else {
            &[]
        };
    let right_segments: &[(Style, String)] =
        if matches!(row.kind, DiffLineKind::Added | DiffLineKind::Context) {
            highlighted_segments
        } else {
            &[]
        };

    let left_lines = wrapped_content_lines(
        left_segments,
        left_width,
        Style::default().fg(colors.text_primary),
    );
    let right_lines = wrapped_content_lines(
        right_segments,
        right_width,
        Style::default().fg(colors.text_primary),
    );

    let line_count = left_lines.len().max(right_lines.len());
    let mut out = Vec::with_capacity(line_count);
    for visual_index in 0..line_count {
        let mut spans = vec![
            Span::styled(
                if visual_index == 0 && is_selected {
                    "▌"
                } else {
                    " "
                },
                marker_style,
            ),
            Span::styled(
                if visual_index == 0 {
                    format!("{:>5} ", old)
                } else {
                    " ".repeat(6)
                },
                Style::default().fg(colors.text_muted),
            ),
            Span::styled(
                if visual_index == 0 && matches!(row.kind, DiffLineKind::Removed) {
                    "-"
                } else {
                    " "
                },
                left_sign_style,
            ),
            Span::raw(" "),
        ];

        spans.extend(
            left_lines
                .get(visual_index)
                .cloned()
                .unwrap_or_else(|| blank_line(left_width, Style::default().fg(colors.text_primary)))
                .spans
                .into_iter(),
        );
        spans.push(Span::styled(
            " │ ",
            Style::default().fg(colors.thread_border),
        ));
        spans.push(Span::styled(
            if visual_index == 0 {
                format!("{:>5} ", new)
            } else {
                " ".repeat(6)
            },
            Style::default().fg(colors.text_muted),
        ));
        spans.push(Span::styled(
            if visual_index == 0 && matches!(row.kind, DiffLineKind::Added) {
                "+"
            } else {
                " "
            },
            right_sign_style,
        ));
        spans.push(Span::raw(" "));
        spans.extend(
            right_lines
                .get(visual_index)
                .cloned()
                .unwrap_or_else(|| {
                    blank_line(right_width, Style::default().fg(colors.text_primary))
                })
                .spans
                .into_iter(),
        );

        let line = if is_selected && is_active {
            Line::from(spans).patch_style(
                Style::default()
                    .bg(colors.selected_line_bg)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Line::from(spans)
        };
        out.push(line);
    }
    out
}

fn wrapped_content_lines(
    segments: &[(Style, String)],
    width: usize,
    pad_style: Style,
) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }
    if segments.is_empty() {
        return vec![blank_line(width, pad_style)];
    }
    let raw_line = styled_segments_line(segments, pad_style);
    let wrapped = wrap_styled_line(&raw_line, width);
    if wrapped.is_empty() {
        return vec![blank_line(width, pad_style)];
    }
    wrapped
        .into_iter()
        .map(|line| pad_line_to_width(line, width, pad_style))
        .collect()
}

fn styled_segments_line(segments: &[(Style, String)], default_style: Style) -> Line<'static> {
    if segments.is_empty() {
        return Line::from(Span::styled("", default_style));
    }
    let spans: Vec<Span<'static>> = segments
        .iter()
        .map(|(style, text)| Span::styled(text.clone(), *style))
        .collect();
    Line::from(spans)
}

fn blank_line(width: usize, style: Style) -> Line<'static> {
    Line::from(Span::styled(" ".repeat(width), style))
}

fn pad_line_to_width(line: Line<'static>, width: usize, pad_style: Style) -> Line<'static> {
    let rendered_width: usize = line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum();
    if rendered_width >= width {
        return line;
    }

    let mut spans = line.spans;
    spans.push(Span::styled(" ".repeat(width - rendered_width), pad_style));
    Line::from(spans)
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

fn spinner_frame(started_at: Instant) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = ((started_at.elapsed().as_millis() / 100) as usize) % FRAMES.len();
    FRAMES[idx]
}

fn draw_status_panel(frame: &mut Frame<'_>, app: &TuiApp, area: ratatui::layout::Rect) {
    let colors = &app.theme().colors;
    let mode_label = if let Some(ai_task) = app.ai_task.as_ref() {
        format!(
            "AI_RUNNING {} {}:{}",
            spinner_frame(ai_task.started_at),
            ai_task.provider.as_str(),
            ai_task.mode.as_str()
        )
    } else if let Some(inline) = app.inline_comment.as_ref() {
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

    let help_line_1 = "keys: q quit | z fullscreen | V split | S side-by-side | Tab pane | </> files width | h/l file | j/k line | m/c comment | r reply";
    let help_line_2 = ":<line> goto | /<text> search | n/p search next/prev | N/P thread | [/] thread list | u name | t/T theme | a/o comment | s/w/d state | v provider | x/X/A AI | H stream | L logs";
    let line_1 = Line::from(vec![
        Span::styled(
            mode_label,
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::raw(format!(
            "review: {} ({:?}) | user: {} | ai: {}",
            app.review.name,
            app.review.state,
            app.config.user_name,
            app.ai_provider.as_str()
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
        Line::from(format!(
            "thread: #{} line {} | replies {} | {}",
            comment.id,
            format_line_reference(comment.old_line, comment.new_line),
            comment.replies.len(),
            format_timestamp_utc(comment.created_at_ms)
        ))
    } else {
        Line::from("thread: none")
    };

    let ai_detail_line = if let Some(detail) = app.last_ai_detail.as_deref() {
        Line::from(format!("ai detail: {detail}"))
    } else {
        Line::from("ai detail: none")
    };

    let mut panel_lines = vec![line_1, line_2, thread_line, ai_detail_line];
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

fn comment_status_label(status: &CommentStatus) -> &'static str {
    match status {
        CommentStatus::Open => "open",
        CommentStatus::Pending => "pending",
        CommentStatus::Addressed => "addressed",
    }
}

fn comment_status_style(status: &CommentStatus, colors: &ThemeColors) -> Style {
    let color = match status {
        CommentStatus::Open => colors.removed_sign,
        CommentStatus::Pending => colors.accent,
        CommentStatus::Addressed => colors.added_sign,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn review_state_label(state: &ReviewState) -> &'static str {
    match state {
        ReviewState::Draft => "draft",
        ReviewState::Pending => "pending",
        ReviewState::WaitingForResponse => "waiting_for_response",
        ReviewState::Done => "done",
    }
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
