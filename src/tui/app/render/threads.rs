use super::super::helpers::{
    format_comment_reference, format_line_range_reference, format_line_reference,
    format_timestamp_utc,
};
use super::helpers::{
    CompactThreadRowSpec, compact_preview, compute_compact_thread_content_width, fit_to_width,
    line_plain_text, push_compact_thread_row, wrap_markdown_lines, wrap_styled_line,
    wrap_styled_line_words,
};
use super::status::{comment_status_label, comment_status_style};
use super::{
    FileReferenceHit, ThreadBodyRenderCacheEntry, ThreadBodyRenderCacheKey,
    ThreadBodyRenderCacheKind, TuiApp,
};
use crate::domain::reference::parse_file_references;
use crate::domain::review::{CommentReply, DiffSide, LineComment, StoredAnchorSnapshot};
use crate::git::diff::DiffSource;
use crate::tui::syntax::SyntaxPainter;
use crate::tui::theme::ThemeColors;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub(super) struct RenderCommentThreadSpec<'a> {
    pub(super) app: &'a mut TuiApp,
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
    let colors = app.theme().colors.clone();
    let expanded = app.is_thread_expanded(comment.id, spec.selected_comment_id);
    let layout = comment_thread_layout(
        app.side_by_side_diff && !matches!(app.diff_source, DiffSource::RootDirectory),
        comment.side.clone(),
        spec.pane_inner_width,
    );

    if !expanded {
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
                colors: &colors,
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
                    colors: &colors,
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
    let comment_body_lines = cached_comment_body_lines(app, comment, layout.inner_width, &colors);
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
        let reply_body_lines =
            cached_reply_body_lines(app, comment.id, reply, layout.reply_inner_width, &colors);
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
                body_lines: &reply_body_lines,
                border_color: colors.thread_border,
                title_color: colors.reply_title,
                colors: &colors,
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
    pub(super) body_lines: &'a [Line<'static>],
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

    for wrapped in spec.body_lines {
        let rendered_row_index = lines.len();
        let wrapped_text = line_plain_text(wrapped);
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
        for span in &wrapped.spans {
            rendered_width += span.content.chars().count();
            let style = if span.style.bg.is_some() {
                span.style
            } else {
                span.style.bg(spec.colors.thread_background)
            };
            row_spans.push(Span::styled(span.content.clone(), style));
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

pub(super) fn cached_comment_body_lines(
    app: &mut TuiApp,
    comment: &LineComment,
    inner_width: usize,
    colors: &ThemeColors,
) -> Arc<[Line<'static>]> {
    let key = ThreadBodyRenderCacheKey {
        thread_id: comment.id,
        body_id: comment.id,
        kind: ThreadBodyRenderCacheKind::Comment,
        revision_ms: comment.updated_at_ms,
        body_hash: text_hash(&comment.body),
        inner_width,
        theme_index: app.theme_index,
    };
    cached_thread_body_lines(app, key, &comment.body, inner_width, colors)
}

pub(super) fn cached_reply_body_lines(
    app: &mut TuiApp,
    thread_id: u64,
    reply: &CommentReply,
    inner_width: usize,
    colors: &ThemeColors,
) -> Arc<[Line<'static>]> {
    let key = ThreadBodyRenderCacheKey {
        thread_id,
        body_id: reply.id,
        kind: ThreadBodyRenderCacheKind::Reply,
        revision_ms: reply.created_at_ms,
        body_hash: text_hash(&reply.body),
        inner_width,
        theme_index: app.theme_index,
    };
    cached_thread_body_lines(app, key, &reply.body, inner_width, colors)
}

pub(super) fn detached_thread_body_lines(
    comment: &LineComment,
    comment_body_lines: &[Line<'static>],
    inner_width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    let Some(anchor) = comment.original_anchor.as_ref() else {
        return comment_body_lines.to_vec();
    };

    let mut lines = original_anchor_context_lines(anchor, inner_width, colors);
    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.extend(comment_body_lines.iter().cloned());
    lines
}

fn cached_thread_body_lines(
    app: &mut TuiApp,
    key: ThreadBodyRenderCacheKey,
    body: &str,
    inner_width: usize,
    colors: &ThemeColors,
) -> Arc<[Line<'static>]> {
    if let Some(entry) = app.get_thread_body_render_cache(&key) {
        return entry.lines.clone();
    }

    let lines: Arc<[Line<'static>]> =
        Arc::from(wrap_markdown_lines(body, inner_width, colors).into_boxed_slice());
    app.insert_thread_body_render_cache(
        key,
        ThreadBodyRenderCacheEntry {
            lines: lines.clone(),
        },
    );
    lines
}

fn original_anchor_context_lines(
    anchor: &StoredAnchorSnapshot,
    inner_width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    push_context_line(
        &mut lines,
        "original anchor",
        &format!(
            "{} @ {} ({})",
            anchor.file_path,
            original_anchor_reference(anchor),
            anchor.side.as_str()
        ),
        inner_width,
        colors,
    );
    if let Some(diff) = anchor.diff.as_ref() {
        push_context_line(&mut lines, "hunk", &diff.hunk_header, inner_width, colors);
    }
    let mut syntax_painter = SyntaxPainter::for_path(&anchor.file_path, colors);
    for line in anchor.before_context.iter().rev() {
        push_context_code_line(
            &mut lines,
            "  ",
            line,
            inner_width,
            colors,
            &mut syntax_painter,
        );
    }
    for line in anchor.selected_text.lines() {
        push_context_code_line(
            &mut lines,
            "> ",
            line,
            inner_width,
            colors,
            &mut syntax_painter,
        );
    }
    for line in &anchor.after_context {
        push_context_code_line(
            &mut lines,
            "  ",
            line,
            inner_width,
            colors,
            &mut syntax_painter,
        );
    }
    lines
}

fn original_anchor_reference(anchor: &StoredAnchorSnapshot) -> String {
    anchor.line_range.as_ref().map_or_else(
        || format_line_reference(anchor.old_line, anchor.new_line),
        format_line_range_reference,
    )
}

fn push_context_line(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    inner_width: usize,
    colors: &ThemeColors,
) {
    let label_style = Style::default()
        .fg(colors.comment_title)
        .bg(colors.thread_background)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default()
        .fg(colors.text_muted)
        .bg(colors.thread_background);
    let line = Line::from(vec![
        Span::styled(format!("{label}: "), label_style),
        Span::styled(value.to_string(), value_style),
    ]);
    lines.extend(wrap_styled_line_words(&line, inner_width));
}

fn push_context_code_line(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    value: &str,
    inner_width: usize,
    colors: &ThemeColors,
    syntax_painter: &mut SyntaxPainter,
) {
    let prefix_style = Style::default()
        .fg(colors.markdown_quote_mark)
        .bg(colors.thread_background)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
    spans.extend(
        syntax_painter
            .highlight(value, colors)
            .into_iter()
            .map(|(style, text)| Span::styled(text, style.bg(colors.thread_background))),
    );
    lines.extend(wrap_styled_line(&Line::from(spans), inner_width));
}

fn text_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::review::{
        Author, CommentReply, CommentStatus, DiffSide, StoredAnchorSnapshot,
    };
    use crate::tui::app::state::tests::{make_comment_with_anchor, make_test_app};

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn collapsed_thread_renders_compact_row() -> anyhow::Result<()> {
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![make_comment_with_anchor(
                1,
                "src/a.rs",
                CommentStatus::Open,
                1,
                1,
            )],
        )?;
        app.collapsed_threads.insert(1);
        let comment = app.comments_for_file("src/a.rs")[0].clone();
        let mut lines = Vec::new();
        let mut row_map = Vec::new();
        let mut link_hits = Vec::new();

        render_comment_thread(
            &mut lines,
            &mut row_map,
            &mut link_hits,
            RenderCommentThreadSpec {
                app: &mut app,
                comment: &comment,
                review_state: "open",
                source_row_index: 0,
                pane_inner_width: 80,
                selected_comment_id: Some(1),
            },
        );

        let rendered_text = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(rendered_text.contains("▸ #1"));
        assert!(!rendered_text.contains("comment #1 ["));
        Ok(())
    }

    #[test]
    fn detached_thread_body_lines_prepend_original_anchor_context() -> anyhow::Result<()> {
        let app = make_test_app(
            vec!["src/a.rs"],
            vec![make_comment_with_anchor(
                1,
                "src/a.rs",
                CommentStatus::Open,
                2,
                2,
            )],
        )?;
        let colors = app.theme().colors.clone();
        let mut comment = make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 2, 2);
        comment.original_anchor = Some(StoredAnchorSnapshot {
            file_path: "src/a.rs".to_string(),
            side: DiffSide::Right,
            old_line: Some(2),
            new_line: Some(2),
            line_range: None,
            selected_text: "let target = make_call();\nlet other = target + 1;".to_string(),
            before_context: vec!["fn before() {}".to_string()],
            after_context: vec!["fn after() {}".to_string()],
            diff: None,
            source: None,
            base_rev: None,
            head_rev: None,
        });
        let body_lines = vec![Line::from("review body")];

        let rendered = detached_thread_body_lines(&comment, &body_lines, 80, &colors);
        let rendered_text = rendered
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered_text.contains("original anchor: src/a.rs @ 2:2 (right)"));
        assert!(rendered_text.contains("  fn before() {}"));
        assert!(rendered_text.contains("> let target = make_call();"));
        assert!(rendered_text.contains("> let other = target + 1;"));
        assert!(rendered_text.contains("  fn after() {}"));
        assert!(rendered_text.contains("review body"));
        let target_line = rendered
            .iter()
            .find(|line| line_text(line).contains("> let target = make_call();"))
            .ok_or_else(|| anyhow::anyhow!("rendered selected line should exist"))?;
        assert!(target_line.spans.len() > 2);
        Ok(())
    }

    #[test]
    fn expanded_thread_reuses_cached_body_lines_for_same_width_and_revision() -> anyhow::Result<()>
    {
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![make_comment_with_anchor(
                1,
                "src/a.rs",
                CommentStatus::Open,
                1,
                1,
            )],
        )?;
        let mut comment = app.comments_for_file("src/a.rs")[0].clone();
        comment.body = "comment body with enough text to wrap and render".to_string();
        comment.updated_at_ms = 10;
        comment.replies.push(CommentReply {
            id: 1,
            author: Author::Ai,
            body: "reply body with enough text to wrap and render".to_string(),
            created_at_ms: 11,
        });

        for _ in 0..2 {
            let mut lines = Vec::new();
            let mut row_map = Vec::new();
            let mut link_hits = Vec::new();
            render_comment_thread(
                &mut lines,
                &mut row_map,
                &mut link_hits,
                RenderCommentThreadSpec {
                    app: &mut app,
                    comment: &comment,
                    review_state: "open",
                    source_row_index: 0,
                    pane_inner_width: 80,
                    selected_comment_id: Some(1),
                },
            );
        }

        assert_eq!(app.thread_body_render_cache.len(), 2);
        Ok(())
    }
}
