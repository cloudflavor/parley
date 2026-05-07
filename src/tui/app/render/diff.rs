use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::domain::diff::DiffLineKind;
use crate::tui::theme::ThemeColors;

use super::helpers::{
    apply_search_highlighting, blank_line, pad_line_to_width, styled_segments_line,
    wrap_styled_line, wrapped_content_lines,
};
use super::{
    DiffPane, DiffRenderCacheEntry, DiffRenderCacheKey, DisplayRow,
    INLINE_FILE_MENTION_MAX_VISIBLE_ROWS, InlineDraftMode, InlineFileMentionState,
    InlineFileReferencePickerState, TuiApp,
};

use super::super::helpers::{
    comment_matches_display_row, format_line_reference, format_timestamp_utc,
};
use super::helpers::{
    compact_preview, compute_compact_thread_content_width, compute_thread_inner_width,
};
use super::status::{comment_status_label, comment_status_style, review_state_label};
use super::threads::{RenderCommentThreadSpec, render_comment_thread};

pub(super) fn draw_diff_view_for_pane(
    frame: &mut Frame<'_>,
    app: &mut TuiApp,
    area: Rect,
    pane: DiffPane,
) {
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
        let mut rendered_comment_ids = std::collections::HashSet::new();

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
            let anchor_state = if comment.detached {
                "detached"
            } else {
                "anchor not in current diff"
            };
            let comment_header = format!(
                "{} | {} | {} @ {}",
                app.author_label(&comment.author),
                format_timestamp_utc(comment.created_at_ms),
                anchor_state,
                format_line_reference(comment.old_line, comment.new_line)
            );
            if matches!(app.thread_density_mode, super::ThreadDensityMode::Compact)
                && !app.is_thread_expanded(comment.id, selected_comment_id)
            {
                super::helpers::push_compact_thread_row(
                    &mut lines,
                    &mut row_map,
                    &mut link_hits,
                    super::helpers::CompactThreadRowSpec {
                        source_row_index: fallback_source_row_index,
                        indent: 8,
                        width: compute_compact_thread_content_width(pane_inner_width, 8),
                        text: &format!(
                            "▸ #{} [{}] {} {} @ {} - {}",
                            comment.id,
                            comment_status_label(&comment.status),
                            app.author_label(&comment.author),
                            anchor_state,
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
                super::threads::push_thread_box(
                    &mut lines,
                    &mut row_map,
                    &mut link_hits,
                    super::threads::ThreadBoxSpec {
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
                    super::threads::push_thread_box(
                        &mut lines,
                        &mut row_map,
                        &mut link_hits,
                        super::threads::ThreadBoxSpec {
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

pub(super) fn diff_pane_borders(split: bool, pane: DiffPane) -> Borders {
    if !split {
        return Borders::TOP | Borders::LEFT | Borders::RIGHT;
    }
    match pane {
        DiffPane::Primary => Borders::TOP | Borders::LEFT | Borders::RIGHT,
        DiffPane::Secondary => Borders::TOP | Borders::RIGHT,
    }
}

pub(super) fn build_unified_row_lines(
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

pub(super) fn draw_inline_comment_editor(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(inline) = app.inline_comment.as_ref() else {
        return;
    };
    let colors = &app.theme().colors;
    let Some(editor_area) = inline_comment_editor_area(area) else {
        return;
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
        Some(InlineFileReferencePickerState { path, .. }) => format!(" | Select Line for {path}"),
        None => String::new(),
    };
    let help_line = match line_picker {
        Some(InlineFileReferencePickerState { path, .. }) => format!(
            "Select a diff line for {path} | ↑/↓/PgUp/PgDn move | Enter/Tab confirm | click line insert | Esc cancel"
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
        let mut content = super::markdown::render_markdown(&inline.buffer.to_text(), colors);
        if content.is_empty() {
            content.push(Line::from(""));
        }
        if let Some(InlineFileReferencePickerState { path, .. }) = line_picker {
            content.push(Line::from(Span::styled(
                format!("Select a diff line for {path} before confirming the file reference."),
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
            content.push(Line::from(super::super::helpers::slice_chars(
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
            format!("Select a diff line for {path} before confirming the file reference."),
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

pub(super) fn inline_comment_editor_area(area: Rect) -> Option<Rect> {
    if area.height < 8 || area.width < 32 {
        return None;
    }

    let available_width = area.width.saturating_sub(2);
    let available_height = area.height.saturating_sub(1);
    if available_width < 30 || available_height < 6 {
        return None;
    }

    let editor_width = available_width.min(68);
    let editor_height = available_height.min(10);

    Some(Rect {
        x: area.x.saturating_add(1),
        y: area.y + area.height.saturating_sub(editor_height),
        width: editor_width,
        height: editor_height,
    })
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
