use super::super::helpers::{
    comment_line_range_contains_display_row, comment_matches_display_row,
    comment_reference_matches_display_row, format_comment_reference, format_line_range_reference,
    format_timestamp_utc,
};
use super::helpers::{
    apply_search_highlighting, blank_line, fit_spans_to_width, pad_line_to_width,
    styled_segments_line, wrap_styled_line, wrapped_content_lines,
};
use super::helpers::{
    compact_preview, compute_compact_thread_content_width, compute_thread_inner_width,
};
use super::status::{
    comment_status_label, comment_status_style, review_state_label, spinner_frame,
};
use super::threads::{
    RenderCommentThreadSpec, cached_comment_body_lines, cached_reply_body_lines,
    render_comment_thread,
};
use super::{
    DiffPane, DiffRenderCacheEntry, DiffRenderCacheKey, DisplayRow,
    INLINE_FILE_MENTION_MAX_VISIBLE_ROWS, InlineDraftMode, InlineFileMentionState,
    InlineFileReferencePickerState, TuiApp,
};
use crate::domain::diff::DiffLineKind;
use crate::domain::review::LineComment;
use crate::git::diff::DiffSource;
use crate::tui::theme::ThemeColors;
use crate::utils::cast::usize_to_u16_saturating;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::collections::{HashMap, HashSet};

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
    frame.render_widget(Clear, area);

    let Some(file_path) = app.diff.files.get(file_index).map(|file| file.path.clone()) else {
        let title = if pane == DiffPane::Primary {
            "Diff A"
        } else {
            "Diff B"
        };
        let borders = diff_pane_borders(app.split_diff_view, pane);
        let message = if matches!(app.diff_source, DiffSource::RootDirectory) {
            if let Some(started_at) = app.root_diff_load_started_at {
                format!(
                    "{} Loading reviewable files in root directory",
                    spinner_frame(started_at)
                )
            } else {
                "No reviewable files found in the current root directory.".to_string()
            }
        } else {
            "No git changes against HEAD.".to_string()
        };
        frame.render_widget(
            Paragraph::new(message).block(
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
    let selected_row_range = app.comment_selection_row_range_for_pane(pane);
    let effective_side_by_side_diff =
        app.side_by_side_diff && !matches!(app.diff_source, DiffSource::RootDirectory);
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
        side_by_side_diff: effective_side_by_side_diff,
        search_query: app.search_query.clone(),
        thread_density_mode: app.thread_density_mode,
        selected_line,
        selected_row_range,
        selected_comment_id,
        expanded_thread_ids: app.expanded_thread_ids_for_file(&file_path),
        collapsed_thread_ids: app.collapsed_thread_ids_for_file(&file_path),
        review_state_code: app.review_state_code(),
        is_active,
    };

    let (lines, row_map, link_hits) = if let Some(cached) = app.get_diff_render_cache(&cache_key) {
        (
            cached.lines.clone(),
            cached.row_map.clone(),
            cached.link_hits.clone(),
        )
    } else {
        app.ensure_row_cache_for_file(file_index);
        let Some(row_count) = app.row_count_for_file(file_index) else {
            return;
        };
        let file_comments = app
            .comments_for_file(&file_path)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let Some(mut syntax_painter) = app.syntax_painter_for_file(file_index, &colors) else {
            return;
        };

        let mut lines = Vec::new();
        let mut row_map = Vec::new();
        let mut link_hits = Vec::new();
        let root_fallback_rows = if matches!(app.diff_source, DiffSource::RootDirectory) {
            build_root_comment_fallback_rows(app, file_index, row_count, &file_comments)
        } else {
            HashMap::new()
        };
        let mut rendered_comment_ids = HashSet::new();

        for index in 0..row_count {
            let highlighted_parts = app.highlighted_segments_for_file_row_with_painter(
                file_index,
                index,
                &mut syntax_painter,
                &colors,
            );
            let highlighted_segments =
                apply_search_highlighting(&highlighted_parts, app.search_query.as_deref(), &colors);
            let Some(row) = app.row_for_file(file_index, index).cloned() else {
                continue;
            };
            let is_current_line = index == selected_line;
            let is_range_selected = selected_row_range
                .is_some_and(|(start, end)| is_active && index >= start && index <= end)
                || file_comments
                    .iter()
                    .any(|comment| comment_line_range_contains_display_row(comment, &row));
            let selection = if is_current_line {
                RowSelectionKind::Current
            } else if is_range_selected {
                RowSelectionKind::Range
            } else {
                RowSelectionKind::None
            };
            let rendered = if effective_side_by_side_diff
                && matches!(
                    row.kind,
                    DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
                ) {
                build_side_by_side_row_lines(
                    &row,
                    &highlighted_segments,
                    selection,
                    is_active,
                    pane_inner_width,
                    &colors,
                )
            } else {
                build_unified_row_lines(
                    &row,
                    &highlighted_segments,
                    selection,
                    is_active,
                    pane_inner_width,
                    &colors,
                )
            };
            for line in rendered {
                lines.push(line);
                row_map.push(index);
            }

            for comment in file_comments.iter().filter(|comment| {
                comment_matches_display_row(comment, &row)
                    || root_fallback_rows
                        .get(&comment.id)
                        .is_some_and(|row_index| *row_index == index)
            }) {
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

        let fallback_source_row_index = selected_line.min(row_count.saturating_sub(1));
        for comment in file_comments
            .iter()
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
                format_comment_reference(comment)
            );
            if !app.is_thread_expanded(comment.id, selected_comment_id) {
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
                            format_comment_reference(comment),
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
                let comment_body_lines =
                    cached_comment_body_lines(app, comment, inner_width, &colors);
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
                        body_lines: &comment_body_lines,
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
                    let reply_body_lines = cached_reply_body_lines(
                        app,
                        comment.id,
                        reply,
                        compute_thread_inner_width(pane_inner_width, 14),
                        &colors,
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
                            body_lines: &reply_body_lines,
                            border_color: colors.thread_border,
                            title_color: colors.reply_title,
                            colors: &colors,
                        },
                    );
                }
            }
        }

        let entry = DiffRenderCacheEntry::new(lines, row_map, link_hits);
        let lines = entry.lines.clone();
        let row_map = entry.row_map.clone();
        let link_hits = entry.link_hits.clone();
        app.insert_diff_render_cache(cache_key, entry);
        (lines, row_map, link_hits)
    };

    let selected_visual_range =
        selected_visual_range(&row_map, selected_line, app.visual_row_for_pane(pane))
            .unwrap_or((0, 0));

    let viewport_height = app.effective_viewport_height_for_pane(pane);
    let scroll = resolve_diff_scroll(
        app.viewport_top_for_pane(pane),
        lines.len(),
        viewport_height,
        selected_visual_range,
        app.take_pending_scroll_anchor(pane),
    );
    app.set_viewport_top_for_pane(pane, scroll);

    if pane == DiffPane::Primary {
        app.last_diff_scroll = scroll;
        app.last_diff_row_map = row_map.to_vec();
        app.last_diff_link_hits = link_hits.to_vec();
    } else {
        app.last_diff_scroll_secondary = scroll;
        app.last_diff_row_map_secondary = row_map.to_vec();
        app.last_diff_link_hits_secondary = link_hits.to_vec();
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
    let visible_lines = lines
        .iter()
        .skip(scroll)
        .take(viewport_height)
        .cloned()
        .collect::<Vec<_>>();
    let widget = Paragraph::new(visible_lines).block(
        Block::default()
            .title(title)
            .borders(borders)
            .border_style(Style::default().fg(border_color))
            .title_style(
                Style::default()
                    .fg(colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
    );
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

pub(super) fn source_row_visual_range(
    row_map: &[usize],
    source_row: usize,
) -> Option<(usize, usize)> {
    let start = row_map
        .iter()
        .position(|row_index| *row_index == source_row)?;
    let end = row_map
        .iter()
        .enumerate()
        .filter_map(|(visual_row, row_index)| (*row_index == source_row).then_some(visual_row))
        .next_back()
        .unwrap_or(start);
    Some((start, end))
}

pub(super) fn selected_visual_range(
    row_map: &[usize],
    source_row: usize,
    visual_row: Option<usize>,
) -> Option<(usize, usize)> {
    if let Some(visual_row) = visual_row
        && row_map
            .get(visual_row)
            .is_some_and(|row_index| *row_index == source_row)
    {
        return Some((visual_row, visual_row));
    }

    source_row_visual_range(row_map, source_row)
}

pub(super) fn keep_source_row_range_visible(
    scroll: usize,
    viewport_height: usize,
    selected_range: (usize, usize),
) -> usize {
    let (selected_start, selected_end) = selected_range;
    let viewport_end = scroll.saturating_add(viewport_height);

    if selected_end < scroll {
        selected_end
    } else if selected_start >= viewport_end {
        selected_start
            .saturating_add(1)
            .saturating_sub(viewport_height)
    } else {
        scroll
    }
}

pub(super) fn resolve_diff_scroll(
    current_scroll: usize,
    total_lines: usize,
    viewport_height: usize,
    selected_range: (usize, usize),
    pending_anchor_row: Option<usize>,
) -> usize {
    let viewport_height = viewport_height.max(1);
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let mut scroll = current_scroll.min(max_scroll);

    if let Some(anchor_row) = pending_anchor_row {
        let clamped_anchor = anchor_row.min(total_lines.saturating_sub(1));
        scroll = clamped_anchor.saturating_sub(viewport_height.saturating_sub(1));
    }

    keep_source_row_range_visible(scroll, viewport_height, selected_range).min(max_scroll)
}

pub(super) fn build_unified_row_lines(
    row: &DisplayRow,
    highlighted_segments: &[(Style, String)],
    selection: RowSelectionKind,
    is_active: bool,
    pane_inner_width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    const UNIFIED_PREFIX_WIDTH: usize = 16;
    const UNIFIED_CONTINUATION_GUTTER_WIDTH: usize = 13;

    let is_selected = !matches!(selection, RowSelectionKind::None);
    let marker_style = if is_selected {
        Style::default()
            .fg(colors.selection_marker)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    };
    let old = row
        .old_line
        .map_or_else(|| " ".to_string(), |value| value.to_string());
    let new = row
        .new_line
        .map_or_else(|| " ".to_string(), |value| value.to_string());

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
    let row_background = diff_line_background(&row.kind, colors);
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
            selection_marker(selection, visual_index),
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
        spans.extend(content_line.spans);

        let line_style = if is_active && matches!(selection, RowSelectionKind::Current) {
            Some(Style::default().bg(colors.selected_line_bg))
        } else if is_active && matches!(selection, RowSelectionKind::Range) {
            Some(Style::default().bg(range_selection_background(colors)))
        } else {
            row_background.map(|background| Style::default().bg(background))
        };
        let line = if let Some(style) = line_style {
            Line::from(spans).patch_style(style)
        } else {
            Line::from(spans)
        };
        out.push(line);
    }
    out
}

fn build_root_comment_fallback_rows(
    app: &TuiApp,
    file_index: usize,
    row_count: usize,
    comments: &[LineComment],
) -> HashMap<u64, usize> {
    let mut fallback_rows = HashMap::new();
    for comment in comments {
        let mut exact_match = false;
        let mut reference_match = None;
        for index in 0..row_count {
            let Some(row) = app.row_for_file(file_index, index) else {
                continue;
            };
            if comment_matches_display_row(comment, row) {
                exact_match = true;
                break;
            }
            if reference_match.is_none() && comment_reference_matches_display_row(comment, row) {
                reference_match = Some(index);
            }
        }
        if !exact_match && let Some(index) = reference_match {
            fallback_rows.insert(comment.id, index);
        }
    }
    fallback_rows
}

pub(super) fn build_side_by_side_row_lines(
    row: &DisplayRow,
    highlighted_segments: &[(Style, String)],
    selection: RowSelectionKind,
    is_active: bool,
    pane_inner_width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    let is_selected = !matches!(selection, RowSelectionKind::None);
    let marker_style = if is_selected {
        Style::default()
            .fg(colors.selection_marker)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors.text_muted)
    };

    let old = row
        .old_line
        .map_or_else(|| " ".to_string(), |value| value.to_string());
    let new = row
        .new_line
        .map_or_else(|| " ".to_string(), |value| value.to_string());

    let fixed_cols = 20usize;
    let code_cols = pane_inner_width.saturating_sub(fixed_cols).max(2);
    let left_width = (code_cols / 2).max(1);
    let right_width = (code_cols - left_width).max(1);
    let left_background = side_by_side_line_background(
        &row.kind,
        DiffSideColumn::Left,
        selection,
        is_active,
        colors,
    );
    let right_background = side_by_side_line_background(
        &row.kind,
        DiffSideColumn::Right,
        selection,
        is_active,
        colors,
    );
    let separator_background =
        (is_active && is_selected && matches!(row.kind, DiffLineKind::Context))
            .then_some(selection_background(selection, colors));

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
            Span::styled(selection_marker(selection, visual_index), marker_style),
            Span::styled(
                if visual_index == 0 {
                    format!("{old:>5} ")
                } else {
                    " ".repeat(6)
                },
                apply_background(Style::default().fg(colors.text_muted), left_background),
            ),
            Span::styled(
                if visual_index == 0 && matches!(row.kind, DiffLineKind::Removed) {
                    "-"
                } else {
                    " "
                },
                apply_background(left_sign_style, left_background),
            ),
            Span::styled(" ", apply_background(Style::default(), left_background)),
        ];

        spans.extend(
            left_lines
                .get(visual_index)
                .cloned()
                .unwrap_or_else(|| blank_line(left_width, Style::default().fg(colors.text_primary)))
                .spans
                .into_iter()
                .map(|span| apply_span_background(span, left_background)),
        );
        spans.push(Span::styled(
            " │ ",
            apply_background(
                Style::default().fg(colors.thread_border),
                separator_background,
            ),
        ));
        spans.push(Span::styled(
            if visual_index == 0 {
                format!("{new:>5} ")
            } else {
                " ".repeat(6)
            },
            apply_background(Style::default().fg(colors.text_muted), right_background),
        ));
        spans.push(Span::styled(
            if visual_index == 0 && matches!(row.kind, DiffLineKind::Added) {
                "+"
            } else {
                " "
            },
            apply_background(right_sign_style, right_background),
        ));
        spans.push(Span::styled(
            " ",
            apply_background(Style::default(), right_background),
        ));
        spans.extend(
            right_lines
                .get(visual_index)
                .cloned()
                .unwrap_or_else(|| {
                    blank_line(right_width, Style::default().fg(colors.text_primary))
                })
                .spans
                .into_iter()
                .map(|span| apply_span_background(span, right_background)),
        );

        let fitted_spans = fit_spans_to_width(spans, pane_inner_width, Style::default());
        let line = if is_active && matches!(selection, RowSelectionKind::Current) {
            Line::from(fitted_spans).patch_style(Style::default().add_modifier(Modifier::BOLD))
        } else {
            Line::from(fitted_spans)
        };
        out.push(line);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSideColumn {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowSelectionKind {
    None,
    Current,
    Range,
}

fn side_by_side_line_background(
    kind: &DiffLineKind,
    column: DiffSideColumn,
    selection: RowSelectionKind,
    is_active: bool,
    colors: &ThemeColors,
) -> Option<Color> {
    if !matches!(selection, RowSelectionKind::None) && is_active {
        return match (kind, column) {
            (DiffLineKind::Removed | DiffLineKind::Context, DiffSideColumn::Left)
            | (DiffLineKind::Added | DiffLineKind::Context, DiffSideColumn::Right) => {
                Some(selection_background(selection, colors))
            }
            _ => None,
        };
    }

    match (kind, column) {
        (DiffLineKind::Removed, DiffSideColumn::Left) => diff_line_background(kind, colors),
        (DiffLineKind::Added, DiffSideColumn::Right) => diff_line_background(kind, colors),
        _ => None,
    }
}

fn selection_marker(selection: RowSelectionKind, visual_index: usize) -> &'static str {
    if visual_index != 0 {
        return " ";
    }
    match selection {
        RowSelectionKind::Current => "▌",
        RowSelectionKind::Range => "▏",
        RowSelectionKind::None => " ",
    }
}

fn selection_background(selection: RowSelectionKind, colors: &ThemeColors) -> Color {
    match selection {
        RowSelectionKind::Current => colors.selected_line_bg,
        RowSelectionKind::Range => range_selection_background(colors),
        RowSelectionKind::None => colors.thread_background,
    }
}

fn range_selection_background(colors: &ThemeColors) -> Color {
    blend_color(colors.thread_background, colors.accent, 0.12)
}

fn apply_span_background(mut span: Span<'static>, background: Option<Color>) -> Span<'static> {
    if let Some(background) = background {
        span.style = span.style.bg(background);
    }
    span
}

fn apply_background(style: Style, background: Option<Color>) -> Style {
    background.map_or(style, |background| style.bg(background))
}

fn diff_line_background(kind: &DiffLineKind, colors: &ThemeColors) -> Option<Color> {
    match kind {
        DiffLineKind::Added => Some(blend_color(
            colors.thread_background,
            colors.added_sign,
            0.18,
        )),
        DiffLineKind::Removed => Some(blend_color(
            colors.thread_background,
            colors.removed_sign,
            0.16,
        )),
        DiffLineKind::Context | DiffLineKind::HunkHeader | DiffLineKind::Meta => None,
    }
}

fn blend_color(base: Color, overlay: Color, alpha: f32) -> Color {
    let Some((base_r, base_g, base_b)) = color_to_rgb(base) else {
        return overlay;
    };
    let Some((overlay_r, overlay_g, overlay_b)) = color_to_rgb(overlay) else {
        return overlay;
    };

    let blend_channel = |base: u8, overlay: u8| -> u8 {
        (f32::from(base) + (f32::from(overlay) - f32::from(base)) * alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    };

    Color::Rgb(
        blend_channel(base_r, overlay_r),
        blend_channel(base_g, overlay_g),
        blend_channel(base_b, overlay_b),
    )
}

fn color_to_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((205, 49, 49)),
        Color::Green => Some((13, 188, 121)),
        Color::Yellow => Some((229, 229, 16)),
        Color::Blue => Some((36, 114, 200)),
        Color::Magenta => Some((188, 63, 188)),
        Color::Cyan => Some((17, 168, 205)),
        Color::Gray => Some((170, 170, 170)),
        Color::DarkGray => Some((85, 85, 85)),
        Color::LightRed => Some((241, 76, 76)),
        Color::LightGreen => Some((35, 209, 139)),
        Color::LightYellow => Some((245, 245, 67)),
        Color::LightBlue => Some((59, 142, 234)),
        Color::LightMagenta => Some((214, 112, 214)),
        Color::LightCyan => Some((41, 184, 219)),
        Color::White => Some((255, 255, 255)),
        Color::Reset | Color::Indexed(_) => None,
    }
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
            target.line_range.as_ref().map_or_else(
                || format_editor_line_reference(target.old_line, target.new_line),
                format_line_range_reference,
            ),
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
    let footer_rows = if line_picker.is_some() { 2 } else { 1 };
    let text_height = inner_height.saturating_sub(footer_rows).max(1);

    let wrapped_lines = wrap_editor_buffer_lines(&inline.buffer.lines, inner_width);
    let (cursor_visual_line, cursor_visual_col) = editor_cursor_visual_position(
        &inline.buffer.lines,
        inline.buffer.cursor_line,
        inline.buffer.cursor_col,
        inner_width,
    );
    let vertical_scroll = cursor_visual_line.saturating_sub(text_height.saturating_sub(1));

    let mut content = Vec::new();
    for offset in 0..text_height {
        let visual_idx = vertical_scroll + offset;
        if let Some(line) = wrapped_lines.get(visual_idx) {
            content.push(Line::from(line.text.clone()));
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
        .saturating_add(usize_to_u16_saturating(cursor_visual_col));
    let cursor_y = editor_area
        .y
        .saturating_add(1)
        .saturating_add(usize_to_u16_saturating(
            cursor_visual_line.saturating_sub(vertical_scroll),
        ));

    if let Some(mention) = inline.file_mention.as_ref() {
        draw_inline_file_mention_picker(frame, editor_area, mention, cursor_x, cursor_y, colors);
    }

    frame.set_cursor_position((cursor_x, cursor_y));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WrappedEditorLine {
    pub(super) text: String,
    start_col: usize,
    end_col: usize,
}

pub(super) fn wrap_editor_buffer_lines(lines: &[String], width: usize) -> Vec<WrappedEditorLine> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in lines {
        out.extend(wrap_editor_line(line, width));
    }
    if out.is_empty() {
        out.push(WrappedEditorLine {
            text: String::new(),
            start_col: 0,
            end_col: 0,
        });
    }
    out
}

fn wrap_editor_line(line: &str, width: usize) -> Vec<WrappedEditorLine> {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![WrappedEditorLine {
            text: String::new(),
            start_col: 0,
            end_col: 0,
        }];
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let hard_end = (start + width).min(chars.len());
        if hard_end == chars.len() {
            out.push(WrappedEditorLine {
                text: chars[start..hard_end].iter().collect(),
                start_col: start,
                end_col: hard_end,
            });
            break;
        }

        let break_after = chars[start + 1..hard_end]
            .iter()
            .rposition(|ch| ch.is_whitespace())
            .map(|position| start + 1 + position + 1);
        let (end, next_start) = break_after.map_or((hard_end, hard_end), |end| {
            let mut next = end;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            (end, next)
        });

        out.push(WrappedEditorLine {
            text: chars[start..end].iter().collect(),
            start_col: start,
            end_col: next_start,
        });
        start = next_start;
    }
    out
}

pub(super) fn editor_cursor_visual_position(
    lines: &[String],
    cursor_line: usize,
    cursor_col: usize,
    width: usize,
) -> (usize, usize) {
    let width = width.max(1);
    let mut visual_line = 0usize;
    for (line_index, line) in lines.iter().enumerate() {
        if line_index == cursor_line {
            let wrapped = wrap_editor_line(line, width);
            let char_count = line.chars().count();
            let cursor_col = cursor_col.min(char_count);
            for (offset, wrapped_line) in wrapped.iter().enumerate() {
                let is_last = offset == wrapped.len().saturating_sub(1);
                if cursor_col < wrapped_line.end_col || is_last {
                    return (
                        visual_line.saturating_add(offset),
                        cursor_col.saturating_sub(wrapped_line.start_col),
                    );
                }
            }
            return (visual_line, 0);
        }
        visual_line = visual_line.saturating_add(wrap_editor_line(line, width).len());
    }
    (visual_line, 0)
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

    let editor_width = available_width.min(88);
    let editor_height = available_height.min(12);

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

    let mut popup_width = usize_to_u16_saturating(max_row_width.saturating_add(6));
    popup_width = popup_width.clamp(24, inner_width);
    let mut popup_height = usize_to_u16_saturating(visible_rows).saturating_add(2);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_visual_range_prefers_exact_visual_row_inside_thread() {
        let row_map = vec![0, 1, 1, 1, 1, 2];

        let range = selected_visual_range(&row_map, 1, Some(3));

        assert_eq!(range, Some((3, 3)));
    }
}
