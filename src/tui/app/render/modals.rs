use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use super::super::helpers::slice_chars;
use super::helpers::fit_to_width;
use super::status::{review_state_label, theme_family_label, theme_variant_label};
use super::{SettingsEditorKind, TuiApp};

pub(super) fn draw_settings_editor(frame: &mut Frame<'_>, app: &TuiApp) {
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
        SettingsEditorKind::CreateReview => "Create Review",
    };
    let colors = app.theme().colors.clone();
    let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let horizontal_scroll = editor
        .cursor_col
        .saturating_sub(inner_width.saturating_sub(1));
    let visible_value = slice_chars(&editor.value, horizontal_scroll, inner_width);

    let content = vec![
        Line::from(match editor.kind {
            SettingsEditorKind::UserName => "Type a display name for your comments/replies.",
            SettingsEditorKind::CreateReview => "Type a review name for the new comment context.",
        }),
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

pub(super) fn draw_theme_picker(frame: &mut Frame<'_>, app: &TuiApp) {
    let Some(picker) = app.theme_picker.as_ref() else {
        return;
    };

    let root = frame.area();
    let width = root.width.saturating_sub(2).clamp(60, 90);
    let height = root.height.saturating_sub(2).clamp(12, 22);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let colors = app.theme().colors.clone();
    let selected = picker
        .selected_index
        .min(app.themes.len().saturating_sub(1));
    let selected_theme = &app.themes[selected];
    let variant = theme_variant_label(&selected_theme.name);
    let family = theme_family_label(&selected_theme.name);

    frame.render_widget(Clear, area);
    let outer_block = Block::default()
        .title("Theme Picker")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors.thread_border))
        .title_style(
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        );
    let content = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(content);

    let visible_rows = usize::from(panels[0].height.saturating_sub(2)).max(1);
    let max_scroll = app.themes.len().saturating_sub(visible_rows);
    let scroll = picker.scroll.min(max_scroll);

    let mut items = Vec::new();
    for (idx, theme) in app
        .themes
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let marker = if idx == app.theme_index { "*" } else { " " };
        let variant = theme_variant_label(&theme.name);
        items.push(ListItem::new(format!(
            "{marker} {} ({variant})",
            theme.name
        )));
    }

    let mut state = ListState::default();
    state.select(Some(selected.saturating_sub(scroll)));

    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Themes").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        panels[0],
        &mut state,
    );

    let swatches = Line::from(vec![
        Span::styled(
            " accent ",
            Style::default().bg(selected_theme.colors.accent),
        ),
        Span::raw(" "),
        Span::styled(
            " text ",
            Style::default()
                .bg(selected_theme.colors.text_primary)
                .fg(selected_theme.colors.thread_background),
        ),
        Span::raw(" "),
        Span::styled(
            " bg ",
            Style::default()
                .bg(selected_theme.colors.thread_background)
                .fg(selected_theme.colors.text_primary),
        ),
        Span::raw(" "),
        Span::styled(" + ", Style::default().bg(selected_theme.colors.added_sign)),
        Span::raw(" "),
        Span::styled(
            " - ",
            Style::default().bg(selected_theme.colors.removed_sign),
        ),
        Span::raw(" "),
        Span::styled(
            " # ",
            Style::default().bg(selected_theme.colors.comment_title),
        ),
    ]);

    let preview_lines = vec![
        Line::from(vec![
            Span::styled(
                "Theme: ",
                Style::default()
                    .fg(colors.text_muted)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                selected_theme.name.clone(),
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Family: ", Style::default().fg(colors.text_muted)),
            Span::styled(family.to_string(), Style::default().fg(colors.text_primary)),
            Span::raw("  "),
            Span::styled("Variant: ", Style::default().fg(colors.text_muted)),
            Span::styled(
                variant.to_string(),
                Style::default().fg(colors.text_primary),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Swatches",
            Style::default()
                .fg(colors.text_muted)
                .add_modifier(Modifier::BOLD),
        )]),
        swatches,
        Line::from(""),
        Line::from(vec![
            Span::styled("Sample: ", Style::default().fg(colors.text_muted)),
            Span::styled("fn ", Style::default().fg(selected_theme.colors.accent)),
            Span::styled(
                "review_pass",
                Style::default()
                    .fg(selected_theme.colors.comment_title)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "() -> ",
                Style::default().fg(selected_theme.colors.text_primary),
            ),
            Span::styled(
                "bool",
                Style::default().fg(selected_theme.colors.reply_title),
            ),
            Span::styled(
                " { ",
                Style::default().fg(selected_theme.colors.text_primary),
            ),
            Span::styled(
                "true",
                Style::default().fg(selected_theme.colors.added_sign),
            ),
            Span::styled(
                " }",
                Style::default().fg(selected_theme.colors.text_primary),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Enter apply | Esc cancel | j/k move",
            Style::default().fg(colors.status_help),
        )]),
    ];

    frame.render_widget(
        Paragraph::new(preview_lines)
            .block(Block::default().title("Preview").borders(Borders::ALL)),
        panels[1],
    );
}

pub(super) fn draw_commit_picker(frame: &mut Frame<'_>, app: &TuiApp) {
    let Some(picker) = app.commit_picker.as_ref() else {
        return;
    };

    let root = frame.area();
    let width = root.width.saturating_sub(2).clamp(72, 120);
    let height = root.height.saturating_sub(2).clamp(14, 24);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let colors = app.theme().colors.clone();
    let filtered = app.commit_picker_filtered_indices();
    let selected = picker.selected_index.min(filtered.len().saturating_sub(1));

    frame.render_widget(Clear, area);
    let outer_block = Block::default()
        .title("Commit Picker")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors.thread_border))
        .title_style(
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        );
    let content = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(content);

    let filter_line = Line::from(vec![
        Span::styled(
            "Search ",
            Style::default()
                .fg(colors.status_help)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            picker.query.clone(),
            Style::default().fg(colors.text_primary),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(vec![
            filter_line,
            Line::from(Span::styled(
                "Filter by commit message or SHA",
                Style::default().fg(colors.text_muted),
            )),
        ])
        .block(Block::default().borders(Borders::ALL)),
        rows[0],
    );

    let visible_rows = usize::from(rows[1].height.saturating_sub(2)).max(1);
    let max_scroll = filtered.len().saturating_sub(visible_rows);
    let scroll = picker.scroll.min(max_scroll);
    let mut items = Vec::new();
    for &commit_index in filtered.iter().skip(scroll).take(visible_rows) {
        if let Some(commit) = picker.commits.get(commit_index) {
            let label = format!("{} {}", commit.short_oid, commit.summary);
            items.push(ListItem::new(fit_to_width(
                &label,
                usize::from(rows[1].width.saturating_sub(6)).max(8),
            )));
        }
    }
    if items.is_empty() {
        items.push(ListItem::new(Span::styled(
            "(no matching commits)",
            Style::default().fg(colors.text_muted),
        )));
    }

    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(selected.saturating_sub(scroll)));
    }
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Commits").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        rows[1],
        &mut state,
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Enter apply · Esc close · ↑↓ select · type to filter",
            Style::default().fg(colors.status_help),
        ))),
        rows[2],
    );

    let filter_area = Block::default().borders(Borders::ALL).inner(rows[0]);
    let cursor_x = filter_area
        .x
        .saturating_add("Search ".chars().count() as u16)
        .saturating_add(picker.cursor_col as u16);
    let max_cursor_x = filter_area
        .x
        .saturating_add(filter_area.width.saturating_sub(1));
    frame.set_cursor_position((cursor_x.min(max_cursor_x), filter_area.y));
}

pub(super) fn draw_review_picker(frame: &mut Frame<'_>, app: &TuiApp) {
    let Some(picker) = app.review_picker.as_ref() else {
        return;
    };

    let root = frame.area();
    let width = root.width.saturating_sub(2).clamp(72, 120);
    let height = root.height.saturating_sub(2).clamp(14, 24);
    let area = Rect {
        x: root.x + root.width.saturating_sub(width) / 2,
        y: root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let colors = app.theme().colors.clone();
    let filtered = app.review_picker_filtered_indices();
    let selected = picker.selected_index.min(filtered.len().saturating_sub(1));

    frame.render_widget(Clear, area);
    let outer_block = Block::default()
        .title("Review Picker")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors.thread_border))
        .title_style(
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        );
    let content = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(content);

    let filter_line = Line::from(vec![
        Span::styled(
            "Search ",
            Style::default()
                .fg(colors.status_help)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            picker.query.clone(),
            Style::default().fg(colors.text_primary),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(vec![
            filter_line,
            Line::from(Span::styled(
                "Filter by review name or state",
                Style::default().fg(colors.text_muted),
            )),
        ])
        .block(Block::default().borders(Borders::ALL)),
        rows[0],
    );

    let visible_rows = usize::from(rows[1].height.saturating_sub(2)).max(1);
    let max_scroll = filtered.len().saturating_sub(visible_rows);
    let scroll = picker.scroll.min(max_scroll);
    let mut items = Vec::new();
    for &review_index in filtered.iter().skip(scroll).take(visible_rows) {
        if let Some(review) = picker.reviews.get(review_index) {
            let current_marker = if review.name == app.review_name {
                "* "
            } else {
                "  "
            };
            let total_count = review.open_count + review.pending_count + review.addressed_count;
            let label = format!(
                "{current_marker}{} [{}] open:{} pending:{} addressed:{} total:{}",
                review.name,
                review_state_label(&review.state),
                review.open_count,
                review.pending_count,
                review.addressed_count,
                total_count
            );
            items.push(ListItem::new(fit_to_width(
                &label,
                usize::from(rows[1].width.saturating_sub(6)).max(8),
            )));
        }
    }
    if items.is_empty() {
        items.push(ListItem::new(Span::styled(
            "(no matching reviews)",
            Style::default().fg(colors.text_muted),
        )));
    }

    let mut state = ListState::default();
    if !filtered.is_empty() {
        state.select(Some(selected.saturating_sub(scroll)));
    }
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().title("Reviews").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> "),
        rows[1],
        &mut state,
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Enter apply · Esc close · ↑↓ select · type to filter",
            Style::default().fg(colors.status_help),
        ))),
        rows[2],
    );

    let filter_area = Block::default().borders(Borders::ALL).inner(rows[0]);
    let cursor_x = filter_area
        .x
        .saturating_add("Search ".chars().count() as u16)
        .saturating_add(picker.cursor_col as u16);
    let max_cursor_x = filter_area
        .x
        .saturating_add(filter_area.width.saturating_sub(1));
    frame.set_cursor_position((cursor_x.min(max_cursor_x), filter_area.y));
}
