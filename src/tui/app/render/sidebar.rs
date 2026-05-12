use super::super::helpers::slice_chars;
use super::TuiApp;
use super::helpers::{compute_scroll, search_highlighted_text_spans};
use crate::utils::cast::usize_to_u16_saturating;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub(super) fn draw_file_sidebar(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    app.last_file_row_map.clear();
    app.last_file_group_map.clear();
    let colors = app.theme().colors.clone();
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
        for (group, sorted_indices, group_stats) in app.ordered_visible_file_groups() {
            let collapsed = app.collapsed_file_groups.contains(&group);
            let twisty = if collapsed { "▸" } else { "▾" };
            let group_line = format!(
                "{twisty} {group}  o:{} p:{} t:{}",
                group_stats.open, group_stats.pending, group_stats.total
            );
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
                let stats = app.comment_stats_for_file(&file.path);

                let marker_style = if stats.open > 0 {
                    Style::default()
                        .fg(colors.comment_title)
                        .add_modifier(Modifier::BOLD)
                } else if stats.pending > 0 {
                    Style::default()
                        .fg(colors.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors.text_muted)
                };
                let marker_text = if stats.open > 0 {
                    format!("  ●{} ", stats.open)
                } else if stats.pending > 0 {
                    format!("  ◍{} ", stats.pending)
                } else if stats.total > 0 {
                    format!("  ◌{} ", stats.total)
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
        let viewport_height = usize::from(list_area.height.saturating_sub(2)).max(1);
        app.last_file_scroll = resolve_file_sidebar_scroll(
            selected_row,
            app.last_file_scroll,
            viewport_height,
            items.len(),
            app.file_sidebar_manual_scroll,
        );
        if !app.file_sidebar_manual_scroll
            || (selected_row >= app.last_file_scroll
                && selected_row < app.last_file_scroll.saturating_add(viewport_height))
        {
            state.select(Some(selected_row));
        }
        *state.offset_mut() = app.last_file_scroll;
    } else {
        app.last_file_scroll = 0;
        app.file_sidebar_manual_scroll = false;
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
    app.last_file_scroll = state.offset();
}

fn resolve_file_sidebar_scroll(
    selected_row: usize,
    current_scroll: usize,
    viewport_height: usize,
    total_rows: usize,
    manual_scroll: bool,
) -> usize {
    let viewport_height = viewport_height.max(1);
    let max_scroll = total_rows.saturating_sub(viewport_height);
    let current_scroll = current_scroll.min(max_scroll);
    if manual_scroll {
        return current_scroll;
    }
    let visible_end = current_scroll.saturating_add(viewport_height);
    if selected_row >= current_scroll && selected_row < visible_end {
        current_scroll
    } else {
        compute_scroll(selected_row, viewport_height).min(max_scroll)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_file_sidebar_scroll;

    #[test]
    fn sidebar_scroll_stays_when_selected_row_is_visible() {
        assert_eq!(resolve_file_sidebar_scroll(24, 20, 10, 50, false), 20);
    }

    #[test]
    fn sidebar_scroll_moves_only_when_selected_row_is_not_visible() {
        assert_eq!(resolve_file_sidebar_scroll(35, 20, 10, 50, false), 26);
        assert_eq!(resolve_file_sidebar_scroll(8, 20, 10, 50, false), 0);
    }

    #[test]
    fn sidebar_manual_scroll_does_not_snap_to_selection() {
        assert_eq!(resolve_file_sidebar_scroll(8, 20, 10, 50, true), 20);
        assert_eq!(resolve_file_sidebar_scroll(8, 99, 10, 50, true), 40);
    }
}
