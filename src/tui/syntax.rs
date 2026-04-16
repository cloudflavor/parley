use std::path::Path;

use once_cell::sync::Lazy;
use ratatui::style::{Color, Modifier, Style};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
};

use super::theme::ThemeColors;

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
        Self {
            highlighter: HighlightLines::new(syntax_for_path(path), &THEME),
        }
    }

    pub fn highlight(&mut self, line: &str, theme: &ThemeColors) -> Vec<(Style, String)> {
        let line_with_newline = if line.ends_with('\n') {
            line.to_string()
        } else {
            format!("{line}\n")
        };
        self.highlighter
            .highlight_line(&line_with_newline, &SYNTAX_SET)
            .map(|parts| {
                parts
                    .into_iter()
                    .map(|(style, text)| {
                        let text = text.strip_suffix('\n').unwrap_or(text);
                        (to_ratatui_style(style, theme), text.to_string())
                    })
                    .collect()
            })
            .unwrap_or_else(|_| vec![(Style::default(), line.to_string())])
    }
}

fn syntax_for_path(path: &str) -> &'static SyntaxReference {
    let path_obj = Path::new(path);
    let lower_extension = path_obj
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    let lower_filename = path_obj
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase());
    SYNTAX_SET
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
            lower_extension
                .as_deref()
                .and_then(|ext| SYNTAX_SET.find_syntax_by_extension(ext))
        })
        .or_else(|| {
            path_obj
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| SYNTAX_SET.find_syntax_by_token(name))
        })
        .or_else(|| {
            lower_filename
                .as_deref()
                .and_then(|name| SYNTAX_SET.find_syntax_by_token(name))
        })
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text())
}

fn to_ratatui_style(style: syntect::highlighting::Style, theme: &ThemeColors) -> Style {
    let fg = normalize_diff_fg(
        Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
        theme,
    );
    let mut ratatui = Style::default().fg(fg);

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

fn normalize_diff_fg(color: Color, theme: &ThemeColors) -> Color {
    let is_dark_bg = perceived_luminance(theme.thread_background)
        .map(|lum| lum < 0.5)
        .unwrap_or(true);
    if is_dark_bg {
        ensure_min_luminance(color, 0.67)
    } else {
        ensure_max_luminance(color, 0.34)
    }
}

fn ensure_min_luminance(color: Color, target: f32) -> Color {
    match color_to_rgb(color) {
        Some((r, g, b)) => {
            let mut out = (r, g, b);
            let mut lum = rgb_luminance(out);
            if lum >= target {
                return color;
            }
            for _ in 0..8 {
                out = blend_rgb(out, (255, 255, 255), 0.20);
                lum = rgb_luminance(out);
                if lum >= target {
                    break;
                }
            }
            Color::Rgb(out.0, out.1, out.2)
        }
        None => color,
    }
}

fn ensure_max_luminance(color: Color, target: f32) -> Color {
    match color_to_rgb(color) {
        Some((r, g, b)) => {
            let mut out = (r, g, b);
            let mut lum = rgb_luminance(out);
            if lum <= target {
                return color;
            }
            for _ in 0..8 {
                out = blend_rgb(out, (0, 0, 0), 0.20);
                lum = rgb_luminance(out);
                if lum <= target {
                    break;
                }
            }
            Color::Rgb(out.0, out.1, out.2)
        }
        None => color,
    }
}

fn perceived_luminance(color: Color) -> Option<f32> {
    color_to_rgb(color).map(rgb_luminance)
}

fn color_to_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

fn rgb_luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn blend_rgb(from: (u8, u8, u8), to: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let clamped = t.clamp(0.0, 1.0);
    let blend = |a: u8, b: u8| -> u8 {
        ((f32::from(a) + (f32::from(b) - f32::from(a)) * clamped).round()).clamp(0.0, 255.0) as u8
    };
    (
        blend(from.0, to.0),
        blend(from.1, to.1),
        blend(from.2, to.2),
    )
}

#[cfg(test)]
mod tests {
    use super::syntax_for_path;

    #[test]
    fn syntax_for_path_detects_toml_files() {
        let syntax = syntax_for_path("Cargo.toml");
        assert_ne!(syntax.name, "Plain Text");
    }

    #[test]
    fn syntax_for_path_handles_uppercase_extensions() {
        let syntax = syntax_for_path("EXAMPLE.RS");
        assert_ne!(syntax.name, "Plain Text");
    }
}
