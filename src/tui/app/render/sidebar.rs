use super::super::helpers::slice_chars;
use super::TuiApp;
use super::helpers::{compute_scroll, search_highlighted_text_spans};
use crate::tui::app::FileSortMode;
use crate::tui::theme::ThemeColors;
use crate::utils::cast::usize_to_u16_saturating;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::collections::BTreeMap;

pub(super) fn draw_file_sidebar(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    app.last_file_row_map.clear();
    app.last_file_group_map.clear();
    let colors = app.theme().colors.clone();
    let file_name_query = app.file_search_query().map(str::to_owned);
    let layout = file_sidebar_layout(area);

    app.last_file_area = Some(layout.list_area);
    app.last_file_search_area = layout.search_area;

    let items = build_file_sidebar_items(app, file_name_query.as_deref(), &colors);
    let mut state = file_sidebar_state(app, layout.list_area, items.len());
    if let Some(search_area) = layout.search_area {
        render_file_search(frame, app, search_area, &colors);
    }

    render_file_list(
        frame,
        app,
        layout.list_area,
        layout.search_area.is_some(),
        items,
        &colors,
        &mut state,
    );
    app.last_file_scroll = state.offset();
}

#[derive(Debug, Clone, Copy)]
struct FileSidebarLayout {
    search_area: Option<Rect>,
    list_area: Rect,
}

#[derive(Debug)]
struct FileSidebarGroup {
    name: String,
    file_indices: Vec<usize>,
    total_count: usize,
    open_count: usize,
    pending_count: usize,
}

fn file_sidebar_layout(area: Rect) -> FileSidebarLayout {
    if area.height <= 4 {
        return FileSidebarLayout {
            search_area: None,
            list_area: area,
        };
    }

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    FileSidebarLayout {
        search_area: Some(parts[0]),
        list_area: parts[1],
    }
}

fn build_file_sidebar_items(
    app: &mut TuiApp,
    file_name_query: Option<&str>,
    colors: &ThemeColors,
) -> Vec<ListItem<'static>> {
    let visible_files = app.visible_file_indices();
    if visible_files.is_empty() {
        return empty_file_sidebar_items(app);
    }

    let groups = sorted_visible_file_groups(app, visible_files);
    let mut items = Vec::new();
    for group in groups {
        push_file_group_item(app, &mut items, &group, colors);
        if app.collapsed_file_groups.contains(&group.name) {
            continue;
        }

        for file_index in sorted_file_indices_for_group(app, group.file_indices) {
            push_file_item(
                app,
                &mut items,
                file_index,
                &group.name,
                file_name_query,
                colors,
            );
        }
    }
    items
}

fn empty_file_sidebar_items(app: &mut TuiApp) -> Vec<ListItem<'static>> {
    let message = if app.diff.files.is_empty() {
        "(no files in git diff HEAD)"
    } else {
        "(no files match current filter)"
    };
    app.last_file_row_map.push(None);
    app.last_file_group_map.push(None);
    vec![ListItem::new(message)]
}

fn sorted_visible_file_groups(app: &TuiApp, visible_files: Vec<usize>) -> Vec<FileSidebarGroup> {
    let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for file_index in visible_files {
        let group = app.file_group_name_for_index(file_index);
        grouped.entry(group).or_default().push(file_index);
    }

    let mut groups = grouped
        .into_iter()
        .map(|(name, file_indices)| file_sidebar_group(app, name, file_indices))
        .collect::<Vec<_>>();
    groups.sort_by(|left, right| match app.file_sort_mode {
        FileSortMode::Path => left.name.cmp(&right.name),
        FileSortMode::OpenCountDesc => right
            .open_count
            .cmp(&left.open_count)
            .then_with(|| right.total_count.cmp(&left.total_count))
            .then_with(|| left.name.cmp(&right.name)),
        FileSortMode::TotalCountDesc => right
            .total_count
            .cmp(&left.total_count)
            .then_with(|| right.open_count.cmp(&left.open_count))
            .then_with(|| left.name.cmp(&right.name)),
    });
    groups
}

fn file_sidebar_group(app: &TuiApp, name: String, file_indices: Vec<usize>) -> FileSidebarGroup {
    let mut total_count = 0usize;
    let mut open_count = 0usize;
    let mut pending_count = 0usize;
    for file_index in &file_indices {
        let file = &app.diff.files[*file_index];
        let stats = app.comment_stats_for_file(&file.path);
        total_count += stats.total;
        open_count += stats.open;
        pending_count += stats.pending;
    }

    FileSidebarGroup {
        name,
        file_indices,
        total_count,
        open_count,
        pending_count,
    }
}

fn sorted_file_indices_for_group(app: &TuiApp, mut file_indices: Vec<usize>) -> Vec<usize> {
    file_indices.sort_by(|left, right| {
        let left_file = &app.diff.files[*left];
        let right_file = &app.diff.files[*right];
        let left_stats = app.comment_stats_for_file(&left_file.path);
        let right_stats = app.comment_stats_for_file(&right_file.path);
        match app.file_sort_mode {
            FileSortMode::Path => left_file.path.cmp(&right_file.path),
            FileSortMode::OpenCountDesc => right_stats
                .open
                .cmp(&left_stats.open)
                .then_with(|| left_file.path.cmp(&right_file.path)),
            FileSortMode::TotalCountDesc => right_stats
                .total
                .cmp(&left_stats.total)
                .then_with(|| left_file.path.cmp(&right_file.path)),
        }
    });
    file_indices
}

fn push_file_group_item(
    app: &mut TuiApp,
    items: &mut Vec<ListItem<'static>>,
    group: &FileSidebarGroup,
    colors: &ThemeColors,
) {
    let collapsed = app.collapsed_file_groups.contains(&group.name);
    let twisty = if collapsed { "▸" } else { "▾" };
    let group_line = format!(
        "{twisty} {}  o:{} p:{} t:{}",
        group.name, group.open_count, group.pending_count, group.total_count
    );
    items.push(ListItem::new(Line::from(Span::styled(
        group_line,
        Style::default()
            .fg(colors.accent)
            .add_modifier(Modifier::BOLD),
    ))));
    app.last_file_row_map.push(None);
    app.last_file_group_map.push(Some(group.name.clone()));
}

fn push_file_item(
    app: &mut TuiApp,
    items: &mut Vec<ListItem<'static>>,
    file_index: usize,
    group_name: &str,
    file_name_query: Option<&str>,
    colors: &ThemeColors,
) {
    let file = &app.diff.files[file_index];
    let stats = app.comment_stats_for_file(&file.path);
    let marker_style = file_marker_style(stats.open, stats.pending, colors);
    let marker_text = file_marker_text(stats.open, stats.pending, stats.total);
    let display_name = sidebar_file_display_name(&file.path, group_name);
    let mut spans = vec![Span::styled(marker_text, marker_style)];
    spans.extend(search_highlighted_text_spans(
        &display_name,
        file_name_query,
        colors,
    ));
    items.push(ListItem::new(Line::from(spans)));
    app.last_file_row_map.push(Some(file_index));
    app.last_file_group_map.push(None);
}

fn file_marker_style(open_count: usize, pending_count: usize, colors: &ThemeColors) -> Style {
    if open_count > 0 {
        Style::default()
            .fg(colors.comment_title)
            .add_modifier(Modifier::BOLD)
    } else if pending_count > 0 {
        Style::default()
            .fg(colors.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    }
}

fn file_marker_text(open_count: usize, pending_count: usize, total_count: usize) -> String {
    if open_count > 0 {
        format!("  ●{open_count} ")
    } else if pending_count > 0 {
        format!("  ◍{pending_count} ")
    } else if total_count > 0 {
        format!("  ◌{total_count} ")
    } else {
        "    ".to_string()
    }
}

fn sidebar_file_display_name(path: &str, group_name: &str) -> String {
    if group_name == "." {
        return path.to_string();
    }
    path.strip_prefix(&format!("{group_name}/"))
        .unwrap_or(path)
        .to_string()
}

fn file_sidebar_state(app: &mut TuiApp, list_area: Rect, item_count: usize) -> ListState {
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
            item_count,
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
    state
}

fn render_file_search(
    frame: &mut Frame<'_>,
    app: &TuiApp,
    search_area: Rect,
    colors: &ThemeColors,
) {
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
        set_file_search_cursor(frame, app, search_area, horizontal_scroll, content_width);
    }
}

fn set_file_search_cursor(
    frame: &mut Frame<'_>,
    app: &TuiApp,
    search_area: Rect,
    horizontal_scroll: usize,
    content_width: usize,
) {
    let prefix = "search> ";
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

fn render_file_list(
    frame: &mut Frame<'_>,
    app: &TuiApp,
    list_area: Rect,
    search_visible: bool,
    items: Vec<ListItem<'static>>,
    colors: &ThemeColors,
    state: &mut ListState,
) {
    let list = List::new(items)
        .block(
            Block::default()
                .title(
                    app.file_search_query()
                        .map_or_else(|| "Files".to_string(), |query| format!("Files [q:{query}]")),
                )
                .borders(if search_visible {
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

    frame.render_stateful_widget(list, list_area, state);
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
