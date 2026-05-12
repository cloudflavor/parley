use super::FileReferenceHit;
use crate::domain::reference::parse_file_references;
use crate::tui::theme::ThemeColors;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const TAB_WIDTH: usize = 4;

pub(super) fn compute_scroll(selected_index: usize, viewport_height: usize) -> usize {
    if viewport_height == 0 {
        return 0;
    }
    if selected_index >= viewport_height {
        selected_index - viewport_height + 1
    } else {
        0
    }
}

pub(super) fn fit_to_width(input: &str, width: usize) -> String {
    let mut out: String = input.chars().take(width).collect();
    let missing = width.saturating_sub(out.chars().count());
    if missing > 0 {
        out.push_str(&" ".repeat(missing));
    }
    out
}

pub(super) fn fit_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Span<'static>> {
    let mut styled_chars: Vec<(Style, char)> = Vec::new();
    let mut column = 0usize;
    for span in spans {
        for ch in span.content.chars() {
            push_render_char(&mut styled_chars, span.style, ch, &mut column);
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

pub(super) fn line_plain_text(line: &Line<'_>) -> String {
    let mut text = String::new();
    for span in &line.spans {
        text.push_str(span.content.as_ref());
    }
    text
}

pub(super) fn styled_segments_line(
    segments: &[(Style, String)],
    default_style: Style,
) -> Line<'static> {
    if segments.is_empty() {
        return Line::from(Span::styled("", default_style));
    }
    let spans: Vec<Span<'static>> = segments
        .iter()
        .flat_map(|(style, text)| spans_with_expanded_tabs(*style, text))
        .collect();
    Line::from(spans)
}

pub(super) fn blank_line(width: usize, style: Style) -> Line<'static> {
    Line::from(Span::styled(" ".repeat(width), style))
}

pub(super) fn pad_line_to_width(
    line: Line<'static>,
    width: usize,
    pad_style: Style,
) -> Line<'static> {
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

pub(super) fn wrap_styled_line(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut styled_chars: Vec<(Style, char)> = Vec::new();
    let mut column = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            push_render_char(&mut styled_chars, span.style, ch, &mut column);
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

pub(super) fn wrap_styled_line_words(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut styled_chars: Vec<(Style, char)> = Vec::new();
    let mut column = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            push_render_char(&mut styled_chars, span.style, ch, &mut column);
        }
    }

    if styled_chars.is_empty() {
        return vec![Line::from("")];
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    while start < styled_chars.len() {
        let hard_end = (start + width).min(styled_chars.len());
        if hard_end == styled_chars.len() {
            out.push(line_from_styled_chars(&styled_chars[start..hard_end]));
            break;
        }

        let break_after = styled_chars[start + 1..hard_end]
            .iter()
            .rposition(|(_, ch)| ch.is_whitespace())
            .map(|position| start + 1 + position + 1);

        let (end, next_start) = break_after.map_or((hard_end, hard_end), |end| {
            let mut next = end;
            while next < styled_chars.len() && styled_chars[next].1.is_whitespace() {
                next += 1;
            }
            (end, next)
        });
        out.push(line_from_styled_chars(&styled_chars[start..end]));
        start = next_start;
    }
    out
}

pub(super) fn line_from_styled_chars(chars: &[(Style, char)]) -> Line<'static> {
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

pub(super) fn apply_search_highlighting(
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
    let mut column = 0usize;
    for (style, text) in segments {
        for ch in text.chars() {
            push_render_char(&mut styled_chars, *style, ch, &mut column);
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

fn spans_with_expanded_tabs(style: Style, text: &str) -> Vec<Span<'static>> {
    let mut styled_chars = Vec::new();
    let mut column = 0usize;
    for ch in text.chars() {
        push_render_char(&mut styled_chars, style, ch, &mut column);
    }
    line_from_styled_chars(&styled_chars).spans
}

fn push_render_char(out: &mut Vec<(Style, char)>, style: Style, ch: char, column: &mut usize) {
    if ch == '\t' {
        let spaces = TAB_WIDTH - (*column % TAB_WIDTH);
        out.extend(std::iter::repeat_n((style, ' '), spaces));
        *column += spaces;
        return;
    }

    out.push((style, ch));
    *column += 1;
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

pub(super) fn search_highlighted_text_spans(
    text: &str,
    query: Option<&str>,
    colors: &ThemeColors,
) -> Vec<Span<'static>> {
    apply_search_highlighting(&[(Style::default(), text.to_string())], query, colors)
        .into_iter()
        .map(|(style, text)| Span::styled(text, style))
        .collect()
}

pub(super) fn wrapped_content_lines(
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

pub(super) fn wrap_markdown_lines(
    input: &str,
    width: usize,
    colors: &ThemeColors,
) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut out = Vec::new();
    for line in super::markdown::render_markdown(input, colors) {
        out.extend(wrap_styled_line_words(&line, width));
    }
    if out.is_empty() {
        out.push(Line::from(""));
    }
    out
}

pub(super) struct CompactThreadRowSpec<'a> {
    pub(super) source_row_index: usize,
    pub(super) indent: usize,
    pub(super) width: usize,
    pub(super) text: &'a str,
    pub(super) style: Style,
    pub(super) colors: &'a ThemeColors,
}

pub(super) fn push_compact_thread_row(
    lines: &mut Vec<Line<'static>>,
    row_map: &mut Vec<usize>,
    link_hits: &mut Vec<FileReferenceHit>,
    spec: CompactThreadRowSpec<'_>,
) {
    let text_style = spec.style.bg(spec.colors.thread_background);
    let mut text_spans = Vec::new();
    super::markdown::push_text_with_file_references(
        &mut text_spans,
        spec.text,
        text_style,
        spec.colors,
    );
    let wrapped = wrap_styled_line_words(&Line::from(text_spans), spec.width.max(1));

    for wrapped_line in wrapped {
        let rendered_row_index = lines.len();
        let wrapped_text = line_plain_text(&wrapped_line);
        for reference in parse_file_references(&wrapped_text) {
            link_hits.push(FileReferenceHit {
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

pub(super) fn compact_preview(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("(empty)")
        .to_string()
}

pub(super) fn compute_thread_inner_width(pane_inner_width: usize, indent: usize) -> usize {
    let available = pane_inner_width.saturating_sub(indent + 4);
    available.clamp(1, THREAD_BOX_MAX_CONTENT_WIDTH)
}

const THREAD_BOX_MAX_CONTENT_WIDTH: usize = 79;

pub(super) fn compute_compact_thread_content_width(
    pane_inner_width: usize,
    indent: usize,
) -> usize {
    pane_inner_width.saturating_sub(indent).max(1)
}
