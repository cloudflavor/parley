use std::{
    collections::{BTreeMap, HashSet},
    time::Instant,
};

use pulldown_cmark::{
    CodeBlockKind, Event as MdEvent, HeadingLevel, Options as MdOptions, Parser as MdParser,
    Tag as MdTag, TagEnd as MdTagEnd,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::domain::{
    diff::DiffLineKind,
    reference::parse_file_references,
    review::{CommentStatus, LineComment, ReviewState},
};

use super::super::theme::ThemeColors;
use super::help_docs::HELP_DOCS;
use super::helpers::{
    comment_matches_display_row, format_line_reference, format_timestamp_utc, slice_chars,
};
use super::{
    CommandPromptMode, DiffPane, DiffRenderCacheEntry, DiffRenderCacheKey, DisplayRow,
    INLINE_FILE_MENTION_MAX_VISIBLE_ROWS, InlineDraftMode, InlineFileMentionState,
    InlineFileReferencePickerState, SettingsEditorKind, ThreadDensityMode, TuiApp,
};

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let root = frame.area();
    let blocking_overlay_visible = app.command_prompt.is_some()
        || app.command_palette.is_some()
        || app.theme_picker.is_some()
        || app.settings_editor.is_some()
        || app.shortcuts_modal_visible;
    app.last_shortcuts_modal_area = None;
    app.last_thread_nav_area = None;
    app.last_thread_nav_scroll = 0;
    app.last_thread_nav_row_map.clear();
    app.last_ai_progress_area = None;
    app.last_file_search_area = None;
    app.last_diff_area_secondary = None;
    app.last_diff_scroll_secondary = 0;
    app.last_diff_row_map_secondary.clear();
    app.last_diff_link_hits.clear();
    app.last_diff_link_hits_secondary.clear();
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
        if app.theme_picker.is_some() {
            draw_theme_picker(frame, app);
        }
        if app.command_prompt.is_some() {
            draw_command_prompt(frame, app);
        }
        if app.command_palette.is_some() {
            draw_command_palette(frame, app);
        }
        if app.ai_progress_visible && !blocking_overlay_visible {
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
    if app.theme_picker.is_some() {
        draw_theme_picker(frame, app);
    }
    if app.command_prompt.is_some() {
        draw_command_prompt(frame, app);
    }
    if app.command_palette.is_some() {
        draw_command_palette(frame, app);
    }
    if app.ai_progress_visible && !blocking_overlay_visible {
        draw_ai_progress_popup(frame, app);
    }
    if app.shortcuts_modal_visible {
        draw_shortcuts_modal(frame, app);
    }
}

fn compute_status_height(total_height: u16) -> u16 {
    if total_height >= 12 { 4 } else { 3 }
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
                format_line_reference(comment.old_line, comment.new_line),
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
            .scroll((scroll as u16, 0)),
        area,
    );
}

fn draw_ai_progress_popup(frame: &mut Frame<'_>, app: &mut TuiApp) {
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
        "H hide/show | K cancel run | L open full logs | PgUp/PgDn/Home/End scroll",
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
            .scroll((scroll as u16, 0)),
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
    proposed.clamp(i32::from(min_value), i32::from(max_value)) as u16
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
    let colors = app.theme().colors.clone();
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

fn draw_theme_picker(frame: &mut Frame<'_>, app: &TuiApp) {
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

fn draw_command_prompt(frame: &mut Frame<'_>, app: &TuiApp) {
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

    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(1)
        .saturating_add((prompt.cursor_col.saturating_sub(horizontal_scroll)) as u16);
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_command_palette(frame: &mut Frame<'_>, app: &mut TuiApp) {
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
        .saturating_add(query_prefix.len() as u16)
        .saturating_add((palette_cursor_col.saturating_sub(query_scroll)) as u16);
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_file_sidebar(frame: &mut Frame<'_>, app: &mut TuiApp, area: ratatui::layout::Rect) {
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
            super::FileSortMode::Path => left.0.cmp(&right.0),
            super::FileSortMode::OpenCountDesc => right
                .3
                .cmp(&left.3)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.cmp(&right.0)),
            super::FileSortMode::TotalCountDesc => right
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
                    super::FileSortMode::Path => left_file.path.cmp(&right_file.path),
                    super::FileSortMode::OpenCountDesc => right_stats
                        .1
                        .cmp(&left_stats.1)
                        .then_with(|| left_file.path.cmp(&right_file.path)),
                    super::FileSortMode::TotalCountDesc => right_stats
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
                .saturating_add(prefix.chars().count() as u16)
                .saturating_add(
                    (app.file_search
                        .cursor_col
                        .saturating_sub(horizontal_scroll)
                        .min(content_width)) as u16,
                );
            let cursor_y = search_area.y.saturating_add(1);
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    let list = List::new(items)
        .block(
            Block::default()
                .title(
                    app.file_search_query()
                        .map(|query| format!("Files [q:{query}]"))
                        .unwrap_or_else(|| "Files".to_string()),
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
            app.set_viewport_top_for_pane(DiffPane::Primary, 0);
            app.last_diff_row_map.clear();
            app.last_diff_link_hits.clear();
            app.pending_scroll_anchor_row = None;
        } else {
            app.last_diff_scroll_secondary = 0;
            app.set_viewport_top_for_pane(DiffPane::Secondary, 0);
            app.last_diff_row_map_secondary.clear();
            app.last_diff_link_hits_secondary.clear();
            app.pending_scroll_anchor_row_secondary = None;
        }
        return;
    };

    let selected_line = app.line_for_pane(pane);
    let selected_comment_id = if app.active_file_index() == file_index {
        app.selected_comment_details().map(|comment| comment.id)
    } else {
        None
    };
    let review_state = review_state_label(&app.review.state);
    let pane_inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let cache_key = DiffRenderCacheKey {
        file_index,
        pane_inner_width,
        side_by_side_diff: app.side_by_side_diff,
        search_query: app.search_query.clone(),
        thread_density_mode: app.thread_density_mode,
        selected_line,
        selected_comment_id,
        expanded_thread_ids: app.expanded_thread_ids_for_file(&file_path),
        review_state_code: app.review_state_code(),
        is_active,
    };

    let (lines, row_map, link_hits) = if let Some(cached) = app.get_diff_render_cache(&cache_key) {
        (cached.lines, cached.row_map, cached.link_hits)
    } else {
        app.ensure_row_cache_for_file(file_index);
        let Some((rows, highlights)) = app.rows_and_highlights_for_file(file_index) else {
            return;
        };
        let file_comments = app.comments_for_file(&file_path);

        let mut lines = Vec::new();
        let mut row_map = Vec::new();
        let mut link_hits = Vec::new();
        let mut rendered_comment_ids = HashSet::new();

        for (index, row) in rows.iter().enumerate() {
            let is_selected = index == selected_line;
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
                render_comment_thread(
                    &mut lines,
                    &mut row_map,
                    &mut link_hits,
                    RenderCommentThreadSpec {
                        app,
                        comment,
                        review_state,
                        source_row_index: index,
                        pane_inner_width,
                        selected_comment_id,
                    },
                );
            }
        }

        let fallback_source_row_index = selected_line.min(rows.len().saturating_sub(1));
        for comment in file_comments
            .iter()
            .copied()
            .filter(|comment| !rendered_comment_ids.contains(&comment.id))
        {
            let inner_width = compute_thread_inner_width(pane_inner_width, 12);
            let comment_header = format!(
                "{} | {} | anchor {} not in current diff",
                app.author_label(&comment.author),
                format_timestamp_utc(comment.created_at_ms),
                format_line_reference(comment.old_line, comment.new_line)
            );
            if matches!(app.thread_density_mode, ThreadDensityMode::Compact)
                && !app.is_thread_expanded(comment.id, selected_comment_id)
            {
                push_compact_thread_row(
                    &mut lines,
                    &mut row_map,
                    &mut link_hits,
                    CompactThreadRowSpec {
                        source_row_index: fallback_source_row_index,
                        indent: 8,
                        width: compute_compact_thread_content_width(pane_inner_width, 8),
                        text: &format!(
                            "▸ #{} [{}] {} @ {} - {}",
                            comment.id,
                            comment_status_label(&comment.status),
                            app.author_label(&comment.author),
                            format_line_reference(comment.old_line, comment.new_line),
                            compact_preview(&comment.body)
                        ),
                        style: Style::default()
                            .fg(colors.comment_title)
                            .bg(colors.thread_background),
                        colors: &colors,
                    },
                );
            } else {
                let comment_title_prefix = format!("comment #{} [", comment.id);
                push_thread_box(
                    &mut lines,
                    &mut row_map,
                    &mut link_hits,
                    ThreadBoxSpec {
                        source_row_index: fallback_source_row_index,
                        indent: 12,
                        inner_width,
                        header: &comment_header,
                        title_prefix: &comment_title_prefix,
                        title_status: Some(comment_status_label(&comment.status)),
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
                        &mut link_hits,
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
        }

        app.insert_diff_render_cache(
            cache_key,
            DiffRenderCacheEntry {
                lines: lines.clone(),
                row_map: row_map.clone(),
                link_hits: link_hits.clone(),
            },
        );
        (lines, row_map, link_hits)
    };

    let selected_visual_index = row_map
        .iter()
        .position(|row_index| *row_index == selected_line)
        .unwrap_or(0);

    let viewport_height = usize::from(area.height.saturating_sub(2)).max(1);
    let max_scroll = lines.len().saturating_sub(viewport_height);
    let mut scroll = app.viewport_top_for_pane(pane).min(max_scroll);

    if let Some(anchor_row) = app.take_pending_scroll_anchor(pane) {
        let clamped_anchor = anchor_row.min(lines.len().saturating_sub(1));
        scroll = clamped_anchor.saturating_sub(viewport_height.saturating_sub(1));
    }

    if selected_visual_index < scroll {
        scroll = selected_visual_index;
    } else if selected_visual_index >= scroll.saturating_add(viewport_height) {
        scroll = selected_visual_index
            .saturating_add(1)
            .saturating_sub(viewport_height);
    }
    scroll = scroll.min(max_scroll);
    app.set_viewport_top_for_pane(pane, scroll);

    if pane == DiffPane::Primary {
        app.last_diff_scroll = scroll;
        app.last_diff_row_map = row_map;
        app.last_diff_link_hits = link_hits;
    } else {
        app.last_diff_scroll_secondary = scroll;
        app.last_diff_row_map_secondary = row_map;
        app.last_diff_link_hits_secondary = link_hits;
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
    let line_picker = inline.file_reference_picker.as_ref();
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
            format!("Reply Box #{comment_id}"),
            format_editor_line_reference(*old_line, *new_line),
        ),
    };
    let title_suffix = match line_picker {
        Some(InlineFileReferencePickerState { path, .. }) => {
            format!(" | Select Line for {}", path)
        }
        None => String::new(),
    };
    let help_line = match line_picker {
        Some(InlineFileReferencePickerState { path, .. }) => format!(
            "Select a diff line for {} | ↑/↓/PgUp/PgDn move | Enter/Tab confirm | click line insert | Esc cancel",
            path
        ),
        None => "Ctrl+S save | Ctrl+P preview | @path:line ref | ↑/↓ lines | Enter/Tab accept ref | Esc close"
            .to_string(),
    };

    frame.render_widget(Clear, editor_area);
    let block = Block::default()
        .title(format!(
            "{title_kind} [{mode}] line {line_ref}{title_suffix}"
        ))
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
        if let Some(InlineFileReferencePickerState { path, .. }) = line_picker {
            content.push(Line::from(Span::styled(
                format!(
                    "Select a diff line for {} before confirming the file reference.",
                    path
                ),
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            content.push(Line::from(""));
        }
        content.push(Line::from(""));
        content.push(Line::from(help_line.clone()));
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
    if let Some(InlineFileReferencePickerState { path, .. }) = line_picker {
        content.push(Line::from(Span::styled(
            format!(
                "Select a diff line for {} before confirming the file reference.",
                path
            ),
            Style::default()
                .fg(colors.accent)
                .add_modifier(Modifier::BOLD),
        )));
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

    if let Some(mention) = inline.file_mention.as_ref() {
        draw_inline_file_mention_picker(frame, editor_area, mention, cursor_x, cursor_y, colors);
    }

    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_inline_file_mention_picker(
    frame: &mut Frame<'_>,
    editor_area: Rect,
    mention: &InlineFileMentionState,
    cursor_x: u16,
    cursor_y: u16,
    colors: &ThemeColors,
) {
    let inner_left = editor_area.x.saturating_add(1);
    let inner_top = editor_area.y.saturating_add(1);
    let inner_right = editor_area
        .x
        .saturating_add(editor_area.width.saturating_sub(1));
    let inner_bottom = editor_area
        .y
        .saturating_add(editor_area.height.saturating_sub(1));
    if inner_right <= inner_left || inner_bottom <= inner_top {
        return;
    }

    let inner_width = inner_right.saturating_sub(inner_left);
    let inner_height = inner_bottom.saturating_sub(inner_top);
    if inner_width < 20 || inner_height < 4 {
        return;
    }

    let total_rows = mention.candidates.len().max(1);
    let visible_rows = total_rows.min(INLINE_FILE_MENTION_MAX_VISIBLE_ROWS);
    let max_row_width = mention
        .candidates
        .iter()
        .skip(mention.scroll)
        .take(visible_rows)
        .map(|path| path.chars().count())
        .max()
        .unwrap_or(18);

    let mut popup_width = (max_row_width + 6) as u16;
    popup_width = popup_width.clamp(24, inner_width);
    let mut popup_height = (visible_rows as u16).saturating_add(2);
    popup_height = popup_height.min(inner_height);

    let mut popup_x = cursor_x.saturating_add(1);
    let max_x = inner_right.saturating_sub(popup_width);
    if popup_x > max_x {
        popup_x = max_x;
    }
    popup_x = popup_x.max(inner_left);

    let below_y = cursor_y.saturating_add(1);
    let mut popup_y = below_y;
    if below_y.saturating_add(popup_height) > inner_bottom {
        popup_y = cursor_y.saturating_sub(popup_height.saturating_sub(1));
    }
    popup_y = popup_y.max(inner_top);
    if popup_y.saturating_add(popup_height) > inner_bottom {
        popup_y = inner_bottom.saturating_sub(popup_height);
    }

    let area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    let selected_index = mention
        .selected_index
        .min(mention.candidates.len().saturating_sub(1));
    let mut lines = Vec::new();
    if mention.candidates.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matching files)",
            Style::default().fg(colors.text_muted),
        )));
    } else {
        for (idx, path) in mention
            .candidates
            .iter()
            .enumerate()
            .skip(mention.scroll)
            .take(visible_rows)
        {
            let is_selected = idx == selected_index;
            let marker = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .bg(colors.sidebar_highlight_bg)
                    .fg(colors.sidebar_highlight_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors.text_primary)
            };
            lines.push(Line::from(Span::styled(format!("{marker}{path}"), style)));
        }
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(format!("@ file  {}", mention.path_query))
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
}

fn format_editor_line_reference(old_line: Option<u32>, new_line: Option<u32>) -> String {
    match (old_line, new_line) {
        (Some(old), Some(new)) => format!("{old}:{new}"),
        (Some(old), None) => format!("{old} (left)"),
        (None, Some(new)) => format!("{new} (right)"),
        (None, None) => "-".to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
enum MdListKind {
    Unordered,
    Ordered { next: u64 },
}

fn md_flush_line(rendered: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>, force: bool) {
    if !current.is_empty() || force {
        rendered.push(Line::from(std::mem::take(current)));
    }
}

fn md_ensure_quote_prefix(
    current: &mut Vec<Span<'static>>,
    line_started: &mut bool,
    quote_depth: usize,
    colors: &ThemeColors,
) {
    if *line_started {
        return;
    }
    for _ in 0..quote_depth {
        current.push(Span::styled(
            "> ",
            Style::default().fg(colors.markdown_quote_mark),
        ));
    }
    *line_started = true;
}

#[derive(Debug, Clone, Copy)]
struct MdTextStyleState {
    heading: Option<HeadingLevel>,
    bold_depth: usize,
    italic_depth: usize,
    in_code_block: bool,
}

#[derive(Debug, Clone, Copy)]
struct MdTextRenderOptions {
    quote_depth: usize,
    inline_code: bool,
    quote_text_style: bool,
}

fn md_push_text(
    current: &mut Vec<Span<'static>>,
    line_started: &mut bool,
    text: &str,
    colors: &ThemeColors,
    state: MdTextStyleState,
    options: MdTextRenderOptions,
) {
    if text.is_empty() {
        return;
    }
    md_ensure_quote_prefix(current, line_started, options.quote_depth, colors);
    let mut style = if options.quote_text_style {
        Style::default().fg(colors.markdown_quote_text)
    } else {
        Style::default().fg(colors.text_primary)
    };
    if state.heading.is_some() || state.bold_depth > 0 {
        style = style.add_modifier(Modifier::BOLD);
    }
    if state.italic_depth > 0 {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if state.in_code_block || options.inline_code {
        style = style
            .fg(colors.markdown_code_fg)
            .bg(colors.markdown_code_bg);
    }
    if state.heading.is_some() {
        style = style.fg(colors.markdown_heading);
    }
    push_text_with_file_references(current, text, style, colors);
}

fn render_markdown(buffer: &str, colors: &ThemeColors) -> Vec<Line<'static>> {
    let mut options = MdOptions::empty();
    options.insert(MdOptions::ENABLE_TABLES);
    options.insert(MdOptions::ENABLE_TASKLISTS);
    options.insert(MdOptions::ENABLE_STRIKETHROUGH);

    let mut rendered: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut quote_depth = 0usize;
    let mut list_stack: Vec<MdListKind> = Vec::new();
    let mut bold_depth = 0usize;
    let mut italic_depth = 0usize;
    let mut heading: Option<HeadingLevel> = None;
    let mut in_code_block = false;
    let mut table_cell_index = 0usize;
    let mut line_started = false;

    for event in MdParser::new_ext(buffer, options) {
        match event {
            MdEvent::Start(tag) => match tag {
                MdTag::Paragraph => {}
                MdTag::Heading { level, .. } => {
                    md_flush_line(&mut rendered, &mut current, false);
                    heading = Some(level);
                    line_started = false;
                }
                MdTag::BlockQuote(_) => {
                    md_flush_line(&mut rendered, &mut current, false);
                    quote_depth += 1;
                    line_started = false;
                }
                MdTag::List(Some(start)) => list_stack.push(MdListKind::Ordered { next: start }),
                MdTag::List(None) => list_stack.push(MdListKind::Unordered),
                MdTag::Item => {
                    md_flush_line(&mut rendered, &mut current, false);
                    md_ensure_quote_prefix(&mut current, &mut line_started, quote_depth, colors);
                    let prefix = match list_stack.last_mut() {
                        Some(MdListKind::Ordered { next }) => {
                            let value = *next;
                            *next += 1;
                            format!("{value}. ")
                        }
                        _ => "• ".to_string(),
                    };
                    current.push(Span::styled(
                        prefix,
                        Style::default().fg(colors.markdown_bullet),
                    ));
                }
                MdTag::CodeBlock(kind) => {
                    md_flush_line(&mut rendered, &mut current, false);
                    in_code_block = true;
                    if let CodeBlockKind::Fenced(language) = kind {
                        let trimmed = language.trim();
                        if !trimmed.is_empty() {
                            rendered.push(Line::from(Span::styled(
                                trimmed.to_string(),
                                Style::default()
                                    .fg(colors.markdown_fence)
                                    .add_modifier(Modifier::BOLD),
                            )));
                        }
                    }
                    line_started = false;
                }
                MdTag::Emphasis => italic_depth += 1,
                MdTag::Strong => bold_depth += 1,
                MdTag::Table(_) => {
                    md_flush_line(&mut rendered, &mut current, false);
                    line_started = false;
                }
                MdTag::TableHead => {
                    md_flush_line(&mut rendered, &mut current, false);
                    line_started = false;
                }
                MdTag::TableRow => {
                    md_flush_line(&mut rendered, &mut current, false);
                    table_cell_index = 0;
                    line_started = true;
                }
                MdTag::TableCell => {
                    if table_cell_index == 0 {
                        current.push(Span::styled(
                            "│ ",
                            Style::default().fg(colors.markdown_quote_mark),
                        ));
                    } else {
                        current.push(Span::styled(
                            " │ ",
                            Style::default().fg(colors.markdown_quote_mark),
                        ));
                    }
                }
                _ => {}
            },
            MdEvent::End(tag_end) => match tag_end {
                MdTagEnd::Paragraph => {
                    md_flush_line(&mut rendered, &mut current, false);
                    rendered.push(Line::from(""));
                    line_started = false;
                }
                MdTagEnd::Heading(_) => {
                    md_flush_line(&mut rendered, &mut current, false);
                    heading = None;
                    rendered.push(Line::from(""));
                    line_started = false;
                }
                MdTagEnd::BlockQuote(_) => {
                    md_flush_line(&mut rendered, &mut current, false);
                    quote_depth = quote_depth.saturating_sub(1);
                    rendered.push(Line::from(""));
                    line_started = false;
                }
                MdTagEnd::List(_) => {
                    md_flush_line(&mut rendered, &mut current, false);
                    list_stack.pop();
                    rendered.push(Line::from(""));
                    line_started = false;
                }
                MdTagEnd::Item => {
                    md_flush_line(&mut rendered, &mut current, false);
                    line_started = false;
                }
                MdTagEnd::CodeBlock => {
                    md_flush_line(&mut rendered, &mut current, false);
                    rendered.push(Line::from(""));
                    in_code_block = false;
                    line_started = false;
                }
                MdTagEnd::Emphasis => italic_depth = italic_depth.saturating_sub(1),
                MdTagEnd::Strong => bold_depth = bold_depth.saturating_sub(1),
                MdTagEnd::TableHead => {
                    md_flush_line(&mut rendered, &mut current, false);
                    rendered.push(Line::from(Span::styled(
                        "├────────────────────────┤",
                        Style::default().fg(colors.text_muted),
                    )));
                    line_started = false;
                }
                MdTagEnd::TableRow => {
                    current.push(Span::styled(
                        " │",
                        Style::default().fg(colors.markdown_quote_mark),
                    ));
                    md_flush_line(&mut rendered, &mut current, false);
                    line_started = false;
                }
                MdTagEnd::TableCell => {
                    table_cell_index += 1;
                }
                MdTagEnd::Table => {
                    md_flush_line(&mut rendered, &mut current, false);
                    rendered.push(Line::from(""));
                    line_started = false;
                }
                _ => {}
            },
            MdEvent::Text(text) => {
                let state = MdTextStyleState {
                    heading,
                    bold_depth,
                    italic_depth,
                    in_code_block,
                };
                let options = MdTextRenderOptions {
                    quote_depth,
                    inline_code: false,
                    quote_text_style: quote_depth > 0,
                };
                for chunk in text.split_inclusive('\n') {
                    let has_newline = chunk.ends_with('\n');
                    let segment = if has_newline {
                        &chunk[..chunk.len().saturating_sub(1)]
                    } else {
                        chunk
                    };
                    if !segment.is_empty() {
                        md_push_text(
                            &mut current,
                            &mut line_started,
                            segment,
                            colors,
                            MdTextStyleState {
                                heading: state.heading,
                                bold_depth: state.bold_depth,
                                italic_depth: state.italic_depth,
                                in_code_block: state.in_code_block,
                            },
                            MdTextRenderOptions {
                                quote_depth: options.quote_depth,
                                inline_code: options.inline_code,
                                quote_text_style: options.quote_text_style,
                            },
                        );
                    }
                    if has_newline {
                        md_flush_line(&mut rendered, &mut current, true);
                        line_started = false;
                    }
                }
            }
            MdEvent::Code(text) => {
                md_push_text(
                    &mut current,
                    &mut line_started,
                    &text,
                    colors,
                    MdTextStyleState {
                        heading,
                        bold_depth,
                        italic_depth,
                        in_code_block,
                    },
                    MdTextRenderOptions {
                        quote_depth,
                        inline_code: true,
                        quote_text_style: quote_depth > 0,
                    },
                );
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                md_flush_line(&mut rendered, &mut current, false);
                line_started = false;
            }
            MdEvent::Rule => {
                md_flush_line(&mut rendered, &mut current, false);
                rendered.push(Line::from(Span::styled(
                    "─".repeat(32),
                    Style::default().fg(colors.text_muted),
                )));
                line_started = false;
            }
            MdEvent::TaskListMarker(checked) => {
                md_ensure_quote_prefix(&mut current, &mut line_started, quote_depth, colors);
                let marker = if checked { "[x] " } else { "[ ] " };
                current.push(Span::styled(
                    marker,
                    Style::default().fg(colors.markdown_bullet),
                ));
            }
            MdEvent::Html(text) => {
                md_push_text(
                    &mut current,
                    &mut line_started,
                    &text,
                    colors,
                    MdTextStyleState {
                        heading,
                        bold_depth,
                        italic_depth,
                        in_code_block,
                    },
                    MdTextRenderOptions {
                        quote_depth,
                        inline_code: false,
                        quote_text_style: false,
                    },
                );
            }
            _ => {}
        }
    }

    md_flush_line(&mut rendered, &mut current, false);
    while rendered
        .last()
        .map(|line| line.spans.iter().all(|span| span.content.is_empty()))
        .unwrap_or(false)
    {
        rendered.pop();
    }
    if rendered.is_empty() {
        rendered.push(Line::from(Span::styled(
            "(empty markdown)",
            Style::default().fg(colors.text_primary),
        )));
    }
    rendered
}

fn push_text_with_file_references(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    base_style: Style,
    colors: &ThemeColors,
) {
    let references = parse_file_references(text);
    if references.is_empty() {
        spans.push(Span::styled(text.to_string(), base_style));
        return;
    }

    let mut cursor = 0usize;
    for reference in references {
        if reference.start_char > cursor {
            spans.push(Span::styled(
                slice_chars(text, cursor, reference.start_char - cursor),
                base_style,
            ));
        }
        let mut link_style = base_style
            .fg(colors.accent)
            .add_modifier(Modifier::UNDERLINED);
        if base_style.bg.is_some() {
            link_style = link_style.bg(base_style.bg.unwrap_or(colors.markdown_code_bg));
        }
        spans.push(Span::styled(reference.raw, link_style));
        cursor = reference.end_char;
    }

    let total_chars = text.chars().count();
    if cursor < total_chars {
        spans.push(Span::styled(
            slice_chars(text, cursor, total_chars - cursor),
            base_style,
        ));
    }
}

const THREAD_BOX_MAX_CONTENT_WIDTH: usize = 79;
fn compute_thread_inner_width(pane_inner_width: usize, indent: usize) -> usize {
    // Each thread line uses: indent + "│ " + content + " │"
    let available = pane_inner_width.saturating_sub(indent + 4);
    available.clamp(1, THREAD_BOX_MAX_CONTENT_WIDTH)
}

fn compute_compact_thread_content_width(pane_inner_width: usize, indent: usize) -> usize {
    pane_inner_width.saturating_sub(indent).max(1)
}

fn compact_preview(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(empty)")
        .to_string()
}

struct CompactThreadRowSpec<'a> {
    source_row_index: usize,
    indent: usize,
    width: usize,
    text: &'a str,
    style: Style,
    colors: &'a ThemeColors,
}

fn push_compact_thread_row(
    lines: &mut Vec<Line<'static>>,
    row_map: &mut Vec<usize>,
    link_hits: &mut Vec<super::FileReferenceHit>,
    spec: CompactThreadRowSpec<'_>,
) {
    let text_style = spec.style.bg(spec.colors.thread_background);
    let mut text_spans = Vec::new();
    push_text_with_file_references(&mut text_spans, spec.text, text_style, spec.colors);
    let wrapped = wrap_styled_line(&Line::from(text_spans), spec.width.max(1));

    for wrapped_line in wrapped {
        let rendered_row_index = lines.len();
        let wrapped_text = line_plain_text(&wrapped_line);
        for reference in parse_file_references(&wrapped_text) {
            link_hits.push(super::FileReferenceHit {
                rendered_row_index,
                col_start: spec.indent + reference.start_char,
                col_end: spec.indent + reference.end_char,
                path: reference.path,
                line: reference.line,
            });
        }

        let mut spans = vec![Span::styled(" ".repeat(spec.indent), Style::default())];
        spans.extend(pad_line_to_width(wrapped_line, spec.width.max(1), text_style).spans);
        lines.push(Line::from(spans));
        row_map.push(spec.source_row_index);
    }
}

fn render_comment_thread(
    lines: &mut Vec<Line<'static>>,
    row_map: &mut Vec<usize>,
    link_hits: &mut Vec<super::FileReferenceHit>,
    spec: RenderCommentThreadSpec<'_>,
) {
    let app = spec.app;
    let comment = spec.comment;
    let colors = &app.theme().colors;
    let expanded = app.is_thread_expanded(comment.id, spec.selected_comment_id);

    if matches!(app.thread_density_mode, ThreadDensityMode::Compact) && !expanded {
        let comment_preview = format!(
            "▸ #{} [{}] {} @ {} - {}",
            comment.id,
            comment_status_label(&comment.status),
            app.author_label(&comment.author),
            format_line_reference(comment.old_line, comment.new_line),
            compact_preview(&comment.body)
        );
        push_compact_thread_row(
            lines,
            row_map,
            link_hits,
            CompactThreadRowSpec {
                source_row_index: spec.source_row_index,
                indent: 8,
                width: compute_compact_thread_content_width(spec.pane_inner_width, 8),
                text: &comment_preview,
                style: Style::default().fg(colors.comment_title),
                colors,
            },
        );

        for reply in &comment.replies {
            let reply_preview = format!(
                "↳ #{} {} - {}",
                reply.id,
                app.author_label(&reply.author),
                compact_preview(&reply.body)
            );
            push_compact_thread_row(
                lines,
                row_map,
                link_hits,
                CompactThreadRowSpec {
                    source_row_index: spec.source_row_index,
                    indent: 10,
                    width: compute_compact_thread_content_width(spec.pane_inner_width, 10),
                    text: &reply_preview,
                    style: Style::default().fg(colors.reply_title),
                    colors,
                },
            );
        }
        return;
    }

    let status = comment_status_label(&comment.status);
    let inner_width = compute_thread_inner_width(spec.pane_inner_width, 12);
    let comment_title_prefix = format!("comment #{} [", comment.id);
    let comment_header = format!(
        "{} | {}",
        app.author_label(&comment.author),
        format_timestamp_utc(comment.created_at_ms)
    );
    push_thread_box(
        lines,
        row_map,
        link_hits,
        ThreadBoxSpec {
            source_row_index: spec.source_row_index,
            indent: 12,
            inner_width,
            header: &comment_header,
            title_prefix: &comment_title_prefix,
            title_status: Some(status),
            title_suffix: &format!(" | review: {}]", spec.review_state),
            title_status_style: Some(comment_status_style(&comment.status, colors)),
            body: &comment.body,
            border_color: colors.thread_border,
            title_color: colors.comment_title,
            colors,
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
            lines,
            row_map,
            link_hits,
            ThreadBoxSpec {
                source_row_index: spec.source_row_index,
                indent: 14,
                inner_width: compute_thread_inner_width(spec.pane_inner_width, 14),
                header: &reply_header,
                title_prefix: &reply_title,
                title_status: None,
                title_suffix: "",
                title_status_style: None,
                body: &reply.body,
                border_color: colors.thread_border,
                title_color: colors.reply_title,
                colors,
            },
        );
    }
}

struct RenderCommentThreadSpec<'a> {
    app: &'a TuiApp,
    comment: &'a LineComment,
    review_state: &'a str,
    source_row_index: usize,
    pane_inner_width: usize,
    selected_comment_id: Option<u64>,
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
    link_hits: &mut Vec<super::FileReferenceHit>,
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
        let rendered_row_index = lines.len();
        let wrapped_text = line_plain_text(&wrapped);
        for reference in parse_file_references(&wrapped_text) {
            link_hits.push(super::FileReferenceHit {
                rendered_row_index,
                col_start: spec.indent + 2 + reference.start_char,
                col_end: spec.indent + 2 + reference.end_char,
                path: reference.path,
                line: reference.line,
            });
        }

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

fn line_plain_text(line: &Line<'_>) -> String {
    let mut text = String::new();
    for span in &line.spans {
        text.push_str(span.content.as_ref());
    }
    text
}

fn fit_to_width(input: &str, width: usize) -> String {
    let mut out: String = input.chars().take(width).collect();
    let missing = width.saturating_sub(out.chars().count());
    if missing > 0 {
        out.push_str(&" ".repeat(missing));
    }
    out
}

fn fit_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Span<'static>> {
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
    let rendered_width: usize = line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum();
    if rendered_width < width {
        line.spans
            .push(Span::styled(" ".repeat(width - rendered_width), pad_style));
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
    const UNIFIED_PREFIX_WIDTH: usize = 16;
    const UNIFIED_CONTINUATION_GUTTER_WIDTH: usize = 13;

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

    let content_width = pane_inner_width.saturating_sub(UNIFIED_PREFIX_WIDTH).max(1);
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
                format!("{old:>5} {new:>5}  ")
            } else {
                " ".repeat(UNIFIED_CONTINUATION_GUTTER_WIDTH)
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
                    format!("{old:>5} ")
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
                format!("{new:>5} ")
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

    let review_state = review_state_label(&app.review.state);
    let file_label = app
        .current_file()
        .map(|file| file.path.as_str())
        .unwrap_or("-");
    let file_position = if app.diff.files.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", app.active_file_index() + 1, app.diff.files.len())
    };

    let selected_thread = app
        .selected_comment_details()
        .map(|comment| {
            format!(
                "#{} {} {}",
                comment.id,
                format_line_reference(comment.old_line, comment.new_line),
                comment_status_label(&comment.status)
            )
        })
        .unwrap_or_else(|| "none".to_string());
    let open_threads = app
        .review
        .comments
        .iter()
        .filter(|comment| matches!(comment.status, CommentStatus::Open))
        .count();
    let pending_human_count = app
        .review
        .comments
        .iter()
        .filter(|comment| matches!(comment.status, CommentStatus::Pending))
        .count();

    let inner_width = usize::from(area.width.saturating_sub(2)).max(1);
    let line_1_left = format!(
        "{} | review {}:{} | file {file_position} {} | thread {selected_thread} | open {open_threads} pending {pending_human_count} | density {}",
        mode_label,
        app.review.name,
        review_state,
        file_label,
        app.thread_density_mode_label()
    );
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let line_1 = build_right_tag_line(
        &line_1_left,
        &version,
        inner_width,
        Style::default().fg(colors.status_help),
    );

    let hint = status_hint(app);
    let thread_left = format!(
        "{} | user {} | ai {} | {hint}",
        truncate_tag_right(&app.status_line, inner_width.saturating_sub(16)),
        app.config.user_name,
        app.ai_provider.as_str()
    );
    let line_2 = build_right_tag_line(
        &thread_left,
        "? help",
        inner_width,
        Style::default().fg(colors.status_help),
    );

    let inner_height = usize::from(area.height.saturating_sub(2));
    let panel_lines = if inner_height >= 2 {
        vec![line_1, line_2]
    } else {
        vec![line_1]
    };

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

fn status_hint(app: &TuiApp) -> &'static str {
    if app.shortcuts_modal_visible {
        "Esc close help | Tab switch doc | </> zoom | j/k scroll"
    } else if app.command_palette.is_some() {
        "Enter run command | Esc close"
    } else if app.theme_picker.is_some() {
        "j/k move | Enter apply | Esc cancel"
    } else if app.settings_editor.is_some() {
        "Enter save | Esc cancel"
    } else if app.command_prompt.is_some() {
        "Enter run | Esc cancel | ←/→ edit"
    } else if app.file_search.focused {
        "Type to filter files | Enter/Esc close | Backspace/Delete edit"
    } else if let Some(inline) = app.inline_comment.as_ref() {
        if inline.preview_mode {
            "Ctrl+P edit | Ctrl+S save | Esc collapse"
        } else {
            "Ctrl+S save | Ctrl+P preview | Esc collapse"
        }
    } else if app.ai_task.is_some() {
        "K cancel AI | H stream | L logs"
    } else {
        "Ctrl+k commands | Ctrl+f files | j/k line | PgUp/PgDn page | zz center | e toggle thread | Shift+E density | / search | : goto"
    }
}

fn theme_variant_label(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("light") {
        "light"
    } else if lower.contains("dark") {
        "dark"
    } else {
        "mixed"
    }
}

fn theme_family_label(name: &str) -> &str {
    name.split(['_', '-'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(name)
}

fn comment_status_label(status: &CommentStatus) -> &'static str {
    match status {
        CommentStatus::Open => "open",
        CommentStatus::Pending => "pending_human",
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
        ReviewState::Open => "open",
        ReviewState::UnderReview => "under_review",
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

fn truncate_tag_right(input: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let input_len = input.chars().count();
    if input_len <= max_len {
        return input.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let mut out: String = input.chars().take(max_len - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::{default_theme_name, load_themes, resolve_theme_index};

    fn test_colors() -> ThemeColors {
        let themes = load_themes().expect("embedded themes should load");
        let index = resolve_theme_index(&themes, default_theme_name()).unwrap_or(0);
        themes[index].colors.clone()
    }

    #[test]
    fn unified_wrapped_rows_preserve_content_and_fit_width() {
        let colors = test_colors();
        let pane_inner_width = 80usize;
        let content = "The default build uses a console renderer so the daemon core can compile and run in environments without GTK system packages.";
        let row = DisplayRow {
            kind: DiffLineKind::Context,
            old_line: Some(5),
            new_line: Some(5),
            raw: content.to_string(),
            code: content.to_string(),
        };
        let highlighted_segments = vec![(
            Style::default().fg(colors.text_primary),
            content.to_string(),
        )];

        let rendered = build_unified_row_lines(
            &row,
            &highlighted_segments,
            false,
            false,
            pane_inner_width,
            &colors,
        );

        assert!(rendered.len() > 1, "expected wrapped output");

        for line in &rendered {
            let width = line_plain_text(line).chars().count();
            assert_eq!(
                width, pane_inner_width,
                "rendered row overflowed pane width"
            );
        }

        let reassembled = rendered
            .iter()
            .map(line_plain_text)
            .map(|line| line.chars().skip(16).collect::<String>())
            .map(|line| line.trim_end().to_string())
            .collect::<String>();
        assert_eq!(reassembled, content);
    }
}
