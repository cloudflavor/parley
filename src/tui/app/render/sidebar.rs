use std::collections::BTreeMap;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use super::super::helpers::slice_chars;
use super::TuiApp;
use super::helpers::{compute_scroll, search_highlighted_text_spans};
use crate::tui::app::FileSortMode;
use crate::utils::cast::usize_to_u16_saturating;

pub(super) fn draw_file_sidebar(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    app.last_file_row_map.clear();
    app.last_file_group_map.clear();
    let colors = app.theme().colors.clone();
    let comment_stats = app.file_comment_stats();
    let file_name_query = app.file_search_query().map(str::to_owned);

    let (search_area, list_area) = if area.height > 4 {
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);
        (Some(parts[0]), parts[1])
    } else {
        (None, area)
    };
    app.last_file_area = Some(list_area);
    app.last_file_search_area = search_area;

    let visible_files = app.visible_file_indices();
    let mut items: Vec<ListItem<'_>> = Vec::new();

    if visible_files.is_empty() {
        let message = if app.diff.files.is_empty() {
            "(no files in git diff HEAD)"
        } else {
            "(no files match current filter)"
        };
        items.push(ListItem::new(message));
        app.last_file_row_map.push(None);
        app.last_file_group_map.push(None);
    } else {
        let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for file_index in visible_files {
            let group = app.file_group_name_for_index(file_index);
            grouped.entry(group).or_default().push(file_index);
        }

        let mut grouped_entries: Vec<(String, Vec<usize>, usize, usize, usize)> = grouped
            .into_iter()
            .map(|(group, file_indices)| {
                let mut group_total = 0usize;
                let mut group_open = 0usize;
                let mut group_pending = 0usize;
                for file_index in &file_indices {
                    let file = &app.diff.files[*file_index];
                    let (total, open, pending) =
                        comment_stats.get(&file.path).copied().unwrap_or((0, 0, 0));
                    group_total += total;
                    group_open += open;
                    group_pending += pending;
                }
                (group, file_indices, group_total, group_open, group_pending)
            })
            .collect();

        grouped_entries.sort_by(|left, right| match app.file_sort_mode {
            FileSortMode::Path => left.0.cmp(&right.0),
            FileSortMode::OpenCountDesc => right
                .3
                .cmp(&left.3)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.cmp(&right.0)),
            FileSortMode::TotalCountDesc => right
                .2
                .cmp(&left.2)
                .then_with(|| right.3.cmp(&left.3))
                .then_with(|| left.0.cmp(&right.0)),
        });

        for (group, file_indices, group_total, group_open, group_pending) in grouped_entries {
            let mut sorted_indices = file_indices;
            sorted_indices.sort_by(|left, right| {
                let left_file = &app.diff.files[*left];
                let right_file = &app.diff.files[*right];
                let left_stats = comment_stats
                    .get(&left_file.path)
                    .copied()
                    .unwrap_or((0, 0, 0));
                let right_stats = comment_stats
                    .get(&right_file.path)
                    .copied()
                    .unwrap_or((0, 0, 0));
                match app.file_sort_mode {
                    FileSortMode::Path => left_file.path.cmp(&right_file.path),
                    FileSortMode::OpenCountDesc => right_stats
                        .1
                        .cmp(&left_stats.1)
                        .then_with(|| left_file.path.cmp(&right_file.path)),
                    FileSortMode::TotalCountDesc => right_stats
                        .0
                        .cmp(&left_stats.0)
                        .then_with(|| left_file.path.cmp(&right_file.path)),
                }
            });

            let collapsed = app.collapsed_file_groups.contains(&group);
            let twisty = if collapsed { "▸" } else { "▾" };
            let group_line =
                format!("{twisty} {group}  o:{group_open} p:{group_pending} t:{group_total}");
            items.push(ListItem::new(Line::from(Span::styled(
                group_line,
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ))));
            app.last_file_row_map.push(None);
            app.last_file_group_map.push(Some(group.clone()));

            if collapsed {
                continue;
            }

            for file_index in sorted_indices {
                let file = &app.diff.files[file_index];
                let (total_comments, open_comments, pending_comments) =
                    comment_stats.get(&file.path).copied().unwrap_or((0, 0, 0));

                let marker_style = if open_comments > 0 {
                    Style::default()
                        .fg(colors.comment_title)
                        .add_modifier(Modifier::BOLD)
                } else if pending_comments > 0 {
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors.text_muted)
                };
                let marker_text = if open_comments > 0 {
                    format!("  ●{open_comments} ")
                } else if pending_comments > 0 {
                    format!("  ◍{pending_comments} ")
                } else if total_comments > 0 {
                    format!("  ◌{total_comments} ")
                } else {
                    "    ".to_string()
                };
                let display_name = if group == "." {
                    file.path.clone()
                } else {
                    file.path
                        .strip_prefix(&format!("{group}/"))
                        .unwrap_or(&file.path)
                        .to_string()
                };
                let mut spans = vec![Span::styled(marker_text, marker_style)];
                spans.extend(search_highlighted_text_spans(
                    &display_name,
                    file_name_query.as_deref(),
                    &colors,
                ));
                items.push(ListItem::new(Line::from(spans)));
                app.last_file_row_map.push(Some(file_index));
                app.last_file_group_map.push(None);
            }
        }
    }

    let mut state = ListState::default();
    let selected_row = app
        .last_file_row_map
        .iter()
        .position(|entry| *entry == Some(app.active_file_index()))
        .or_else(|| {
            app.last_file_row_map
                .iter()
                .position(|entry| entry.is_some())
        });
    if let Some(selected_row) = selected_row {
        state.select(Some(selected_row));
        app.last_file_scroll = compute_scroll(
            selected_row,
            usize::from(list_area.height.saturating_sub(2)),
        );
    } else {
        app.last_file_scroll = 0;
    }

    if let Some(search_area) = search_area {
        let active = app.file_search.focused;
        let title_style = if active {
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors.text_muted)
        };
        let query_style = if active {
            Style::default()
                .bg(colors.sidebar_highlight_bg)
                .fg(colors.sidebar_highlight_fg)
        } else {
            Style::default().fg(colors.text_primary)
        };
        let prefix = "search> ";
        let inner_width = usize::from(search_area.width.saturating_sub(2)).max(1);
        let content_width = inner_width.saturating_sub(prefix.chars().count());
        let horizontal_scroll = app
            .file_search
            .cursor_col
            .saturating_sub(content_width.saturating_sub(1));
        let query_slice = slice_chars(&app.file_search.query, horizontal_scroll, content_width);
        let line = Line::from(vec![
            Span::styled(prefix, Style::default().fg(colors.status_help)),
            Span::styled(query_slice, query_style),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(
                Block::default()
                    .title("Files Filter (Ctrl+f)")
                    .borders(Borders::TOP | Borders::LEFT)
                    .border_style(Style::default().fg(colors.thread_border))
                    .title_style(title_style),
            ),
            search_area,
        );
        if active {
            let cursor_x = search_area
                .x
                .saturating_add(1)
                .saturating_add(usize_to_u16_saturating(prefix.chars().count()))
                .saturating_add(usize_to_u16_saturating(
                    app.file_search
                        .cursor_col
                        .saturating_sub(horizontal_scroll)
                        .min(content_width),
                ));
            let cursor_y = search_area.y.saturating_add(1);
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(
                    app.file_search_query()
                        .map_or_else(|| "Files".to_string(), |query| format!("Files [q:{query}]")),
                )
                .borders(if search_area.is_some() {
                    Borders::LEFT
                } else {
                    Borders::TOP | Borders::LEFT
                })
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

    frame.render_stateful_widget(list, list_area, &mut state);
}
