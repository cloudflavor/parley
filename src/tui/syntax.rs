use std::path::Path;

use once_cell::sync::Lazy;
use ratatui::style::{Color, Modifier, Style};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
};

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
static THEME: Lazy<Theme> = Lazy::new(|| {
    let set = ThemeSet::load_defaults();
    set.themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_default()
});

pub struct SyntaxPainter {
    highlighter: HighlightLines<'static>,
}

impl SyntaxPainter {
    pub fn for_path(path: &str) -> Self {
        let path_obj = Path::new(path);
        let syntax = SYNTAX_SET
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .or_else(|| {
                path_obj
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .and_then(|ext| SYNTAX_SET.find_syntax_by_extension(ext))
            })
            .or_else(|| {
                path_obj
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| SYNTAX_SET.find_syntax_by_token(name))
            })
            .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
        Self {
            highlighter: HighlightLines::new(syntax, &THEME),
        }
    }

    pub fn highlight(&mut self, line: &str) -> Vec<(Style, String)> {
        self.highlighter
            .highlight_line(line, &SYNTAX_SET)
            .map(|parts| {
                parts
                    .into_iter()
                    .map(|(style, text)| (to_ratatui_style(style), text.to_string()))
                    .collect()
            })
            .unwrap_or_else(|_| vec![(Style::default(), line.to_string())])
    }
}

fn to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let mut ratatui = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));

    if style.font_style.contains(FontStyle::BOLD) {
        ratatui = ratatui.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        ratatui = ratatui.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        ratatui = ratatui.add_modifier(Modifier::UNDERLINED);
    }

    ratatui
}
