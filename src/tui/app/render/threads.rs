use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::domain::reference::parse_file_references;
use crate::domain::review::DiffSide;
use crate::git::diff::DiffSource;
use crate::tui::theme::ThemeColors;

use super::super::helpers::{format_comment_reference, format_timestamp_utc};
use super::helpers::{
    CompactThreadRowSpec, compact_preview, compute_compact_thread_content_width, fit_to_width,
    line_plain_text, push_compact_thread_row, wrap_markdown_lines,
};
use super::status::{comment_status_label, comment_status_style};
use super::{FileReferenceHit, TuiApp};

pub(super) struct RenderCommentThreadSpec<'a> {
    pub(super) app: &'a TuiApp,
    pub(super) comment: &'a crate::domain::review::LineComment,
    pub(super) review_state: &'a str,
    pub(super) source_row_index: usize,
    pub(super) pane_inner_width: usize,
    pub(super) selected_comment_id: Option<u64>,
}

pub(super) fn render_comment_thread(
    lines: &mut Vec<Line<'static>>,
    row_map: &mut Vec<usize>,
    link_hits: &mut Vec<FileReferenceHit>,
    spec: RenderCommentThreadSpec<'_>,
) {
    let app = spec.app;
    let comment = spec.comment;
    let colors = &app.theme().colors;
    let expanded = app.is_thread_expanded(comment.id, spec.selected_comment_id);
    let layout = comment_thread_layout(
        app.side_by_side_diff && !matches!(app.diff_source, DiffSource::RootDirectory),
        comment.side.clone(),
        spec.pane_inner_width,
    );

    if matches!(app.thread_density_mode, super::ThreadDensityMode::Compact) && !expanded {
        let comment_preview = format!(
            "▸ #{} [{}] {} @ {} - {}",
            comment.id,
            comment_status_label(&comment.status),
            app.author_label(&comment.author),
            format_comment_reference(comment),
            compact_preview(&comment.body)
        );
        push_compact_thread_row(
            lines,
            row_map,
            link_hits,
            CompactThreadRowSpec {
                source_row_index: spec.source_row_index,
                indent: layout.indent,
                width: compute_compact_thread_content_width(spec.pane_inner_width, layout.indent)
                    .min(layout.outer_width),
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
                    indent: layout.reply_indent,
                    width: compute_compact_thread_content_width(
                        spec.pane_inner_width,
                        layout.reply_indent,
                    )
                    .min(layout.reply_outer_width),
                    text: &reply_preview,
                    style: Style::default().fg(colors.reply_title),
                    colors,
                },
            );
        }
        return;
    }

    let status = comment_status_label(&comment.status);
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
            indent: layout.indent,
            inner_width: layout.inner_width,
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
                indent: layout.reply_indent,
                inner_width: layout.reply_inner_width,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CommentThreadLayout {
    pub(super) indent: usize,
    pub(super) inner_width: usize,
    pub(super) outer_width: usize,
    pub(super) reply_indent: usize,
    pub(super) reply_inner_width: usize,
    pub(super) reply_outer_width: usize,
}

pub(super) fn comment_thread_layout(
    side_by_side_diff: bool,
    side: DiffSide,
    pane_inner_width: usize,
) -> CommentThreadLayout {
    let (indent, outer_width) = if side_by_side_diff {
        let fixed_cols = 20usize;
        let code_cols = pane_inner_width.saturating_sub(fixed_cols).max(2);
        let left_width = (code_cols / 2).max(1);
        let right_width = (code_cols - left_width).max(1);
        match side {
            DiffSide::Left => (1, left_width.saturating_add(8).min(pane_inner_width)),
            DiffSide::Right => {
                let right_start = left_width.saturating_add(11);
                (
                    right_start.min(pane_inner_width.saturating_sub(8)),
                    right_width.saturating_add(8),
                )
            }
        }
    } else {
        (12, pane_inner_width.saturating_sub(12))
    };

    let outer_width = outer_width
        .min(pane_inner_width.saturating_sub(indent))
        .max(12);
    let inner_width = outer_width.saturating_sub(4).max(8);
    let reply_indent = indent
        .saturating_add(2)
        .min(pane_inner_width.saturating_sub(8));
    let reply_outer_width = outer_width
        .saturating_sub(2)
        .min(pane_inner_width.saturating_sub(reply_indent))
        .max(10);
    let reply_inner_width = reply_outer_width.saturating_sub(4).max(8);

    CommentThreadLayout {
        indent,
        inner_width,
        outer_width,
        reply_indent,
        reply_inner_width,
        reply_outer_width,
    }
}

pub(super) struct ThreadBoxSpec<'a> {
    pub(super) source_row_index: usize,
    pub(super) indent: usize,
    pub(super) inner_width: usize,
    pub(super) header: &'a str,
    pub(super) title_prefix: &'a str,
    pub(super) title_status: Option<&'a str>,
    pub(super) title_suffix: &'a str,
    pub(super) title_status_style: Option<Style>,
    pub(super) body: &'a str,
    pub(super) border_color: Color,
    pub(super) title_color: Color,
    pub(super) colors: &'a ThemeColors,
}

pub(super) fn push_thread_box(
    lines: &mut Vec<Line<'static>>,
    row_map: &mut Vec<usize>,
    link_hits: &mut Vec<FileReferenceHit>,
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
    let title_spans =
        super::helpers::fit_spans_to_width(title_spans, spec.inner_width, title_style);
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
            link_hits.push(FileReferenceHit {
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
