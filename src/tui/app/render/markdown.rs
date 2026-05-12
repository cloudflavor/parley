use super::super::super::theme::ThemeColors;
use super::super::helpers::slice_chars;
use crate::domain::reference::parse_file_references;
use pulldown_cmark::{
    CodeBlockKind, Event as MdEvent, HeadingLevel, Options as MdOptions, Parser as MdParser,
    Tag as MdTag, TagEnd as MdTagEnd,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

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

pub(super) fn render_markdown(buffer: &str, colors: &ThemeColors) -> Vec<Line<'static>> {
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
        .is_some_and(|line| line.spans.iter().all(|span| span.content.is_empty()))
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

pub(super) fn push_text_with_file_references(
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
