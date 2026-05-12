use super::theme::ThemeColors;
use ratatui::style::{Color, Modifier, Style};
use std::{path::Path, sync::LazyLock};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_DARK: LazyLock<Theme> = LazyLock::new(|| {
    ThemeSet::load_defaults()
        .themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_default()
});
static THEME_LIGHT: LazyLock<Theme> = LazyLock::new(|| {
    let themes = &ThemeSet::load_defaults().themes;
    themes
        .get("InspiredGitHub")
        .or_else(|| themes.get("base16-ocean.light"))
        .or_else(|| themes.get("Solarized (light)"))
        .cloned()
        .unwrap_or_default()
});

pub struct SyntaxPainter {
    highlighter: HighlightLines<'static>,
}

impl SyntaxPainter {
    pub fn for_path(path: &str, theme: &ThemeColors) -> Self {
        let syntax_theme = if theme_is_dark(theme) {
            &THEME_DARK
        } else {
            &THEME_LIGHT
        };
        Self {
            highlighter: HighlightLines::new(syntax_for_path(path), syntax_theme),
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
            .map_or_else(
                |_| vec![(Style::default(), line.to_string())],
                |parts| {
                    parts
                        .into_iter()
                        .map(|(style, text)| {
                            let text = text.strip_suffix('\n').unwrap_or(text);
                            (to_ratatui_style(style, text, theme), text.to_string())
                        })
                        .collect()
                },
            )
    }
}

fn theme_is_dark(theme: &ThemeColors) -> bool {
    color_to_rgb(theme.thread_background)
        .map(rgb_luminance)
        .is_none_or(|lum| lum < 0.5)
}

fn syntax_for_path(path: &str) -> &'static SyntaxReference {
    let path_obj = Path::new(path);

    if let Some(syntax) = SYNTAX_SET.find_syntax_for_file(path).ok().flatten() {
        return syntax;
    }

    let ext = path_obj.extension().and_then(|ext| ext.to_str());
    if let Some(ext) = ext {
        if let Some(syntax) = SYNTAX_SET.find_syntax_by_extension(ext) {
            return syntax;
        }
        let lower_ext = ext.to_ascii_lowercase();
        if let Some(syntax) = SYNTAX_SET.find_syntax_by_extension(&lower_ext) {
            return syntax;
        }
    }

    let filename = path_obj.file_name().and_then(|name| name.to_str());
    if let Some(name) = filename {
        if let Some(syntax) = SYNTAX_SET.find_syntax_by_token(name) {
            return syntax;
        }
        let lower_name = name.to_ascii_lowercase();
        if let Some(syntax) = SYNTAX_SET.find_syntax_by_token(&lower_name) {
            return syntax;
        }
    }

    if let Some(syntax) = fallback_known_syntax(ext, filename) {
        return syntax;
    }

    SYNTAX_SET.find_syntax_plain_text()
}

fn fallback_known_syntax(
    extension: Option<&str>,
    filename: Option<&str>,
) -> Option<&'static SyntaxReference> {
    let syntax = match extension {
        Some("py" | "pyw" | "py3") => syntax_by_name_or_extension(&["Python"], &["py"]),
        Some("js" | "mjs" | "cjs" | "jsx") => syntax_by_name_or_extension(&["JavaScript"], &["js"]),
        Some("ts" | "tsx") => {
            syntax_by_name_or_extension(&["TypeScript", "JavaScript"], &["ts", "js"])
        }
        Some("svelte" | "vue" | "astro") => {
            syntax_by_name_or_extension(&["HTML", "JavaScript"], &["html", "js"])
        }
        Some("go") => syntax_by_name_or_extension(&["Go"], &["go"]),
        Some("java") => syntax_by_name_or_extension(&["Java"], &["java"]),
        Some("kt" | "kts") => syntax_by_name_or_extension(&["Kotlin", "Java"], &["kt", "java"]),
        Some("c") => syntax_by_name_or_extension(&["C"], &["c"]),
        Some("h") => syntax_by_name_or_extension(&["C", "C++"], &["h", "c", "cpp"]),
        Some("cc" | "cpp" | "cxx" | "hpp" | "hxx") => {
            syntax_by_name_or_extension(&["C++", "C"], &["cpp", "c"])
        }
        Some("rb") => syntax_by_name_or_extension(&["Ruby"], &["rb"]),
        Some("php") => syntax_by_name_or_extension(&["PHP"], &["php"]),
        Some("swift") => syntax_by_name_or_extension(&["Swift", "C++"], &["swift", "cpp"]),
        Some("sh" | "bash" | "zsh") => syntax_by_name_or_extension(&["Bash"], &["sh"]),
        Some("sql") => syntax_by_name_or_extension(&["SQL"], &["sql"]),
        Some("css" | "scss" | "sass") => syntax_by_name_or_extension(&["CSS"], &["css"]),
        Some("html" | "htm") => syntax_by_name_or_extension(&["HTML"], &["html"]),
        Some("json") => syntax_by_name_or_extension(&["JSON"], &["json"]),
        Some("yaml" | "yml") => syntax_by_name_or_extension(&["YAML"], &["yaml", "yml"]),
        Some("md" | "markdown") => syntax_by_name_or_extension(&["Markdown"], &["md"]),
        _ => None,
    };
    if syntax.is_some() {
        return syntax;
    }

    if !matches!(extension, Some("toml")) && !matches!(filename, Some("cargo.toml")) {
        return None;
    }

    SYNTAX_SET
        .find_syntax_by_name("TOML")
        .or_else(|| SYNTAX_SET.find_syntax_by_token("Cargo.toml"))
        .or_else(|| SYNTAX_SET.find_syntax_by_extension("toml"))
        .or_else(|| SYNTAX_SET.find_syntax_by_extension("yaml"))
        .or_else(|| SYNTAX_SET.find_syntax_by_extension("ini"))
        .or_else(|| SYNTAX_SET.find_syntax_by_extension("rs"))
}

fn syntax_by_name_or_extension(
    names: &[&str],
    extensions: &[&str],
) -> Option<&'static SyntaxReference> {
    names
        .iter()
        .find_map(|name| SYNTAX_SET.find_syntax_by_name(name))
        .or_else(|| {
            extensions
                .iter()
                .find_map(|extension| SYNTAX_SET.find_syntax_by_extension(extension))
        })
}

fn to_ratatui_style(
    style: syntect::highlighting::Style,
    _token_text: &str,
    theme: &ThemeColors,
) -> Style {
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
    let Some(fg_rgb) = color_to_rgb(color) else {
        return color;
    };
    let Some(bg_rgb) = color_to_rgb(theme.thread_background) else {
        return color;
    };

    let fg_lum = rgb_luminance(fg_rgb);
    let bg_lum = rgb_luminance(bg_rgb);
    let delta = (fg_lum - bg_lum).abs();
    if delta >= 0.18 {
        return color;
    }

    // Keep token hue, only nudge brightness when contrast is too low.
    let adjusted = if bg_lum < 0.5 {
        blend_rgb(fg_rgb, (255, 255, 255), 0.35)
    } else {
        blend_rgb(fg_rgb, (0, 0, 0), 0.35)
    };
    Color::Rgb(adjusted.0, adjusted.1, adjusted.2)
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
    use super::{SyntaxPainter, syntax_for_path};
    use anyhow::{Result, anyhow};
    use std::collections::HashSet;

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

    #[test]
    fn syntax_for_path_detects_major_language_extensions() {
        for path in [
            "main.py",
            "app.js",
            "component.jsx",
            "lib.ts",
            "component.tsx",
            "page.svelte",
            "component.vue",
            "page.astro",
            "main.go",
            "Main.java",
            "main.cpp",
            "main.c",
            "main.h",
            "main.rb",
            "main.php",
            "main.swift",
            "main.kt",
            "main.rs",
            "main.sh",
            "query.sql",
            "styles.css",
            "index.html",
            "config.yaml",
            "data.json",
        ] {
            let syntax = syntax_for_path(path);
            assert_ne!(syntax.name, "Plain Text", "{path} should have syntax");
        }
    }

    #[test]
    fn syntax_painter_uses_syntect_colors_for_typescript() -> Result<()> {
        let themes = crate::tui::theme::load_themes()?;
        let colors = &themes
            .first()
            .ok_or_else(|| anyhow!("expected at least one theme"))?
            .colors;
        let mut painter = SyntaxPainter::for_path("api/src/lib/encryption.ts", colors);
        let parts = painter.highlight(
            "const rawKey = await crypto.subtle.exportKey(\"raw\", key as CryptoKey);",
            colors,
        );
        let foregrounds = parts
            .iter()
            .filter_map(|(style, _)| style.fg)
            .collect::<HashSet<_>>();

        assert!(
            foregrounds.len() > 1,
            "TypeScript should receive multiple syntect foreground colors"
        );
        Ok(())
    }

    #[test]
    fn syntax_painter_highlights_svelte_and_jsx_family_files() -> Result<()> {
        let themes = crate::tui::theme::load_themes()?;
        let colors = &themes
            .first()
            .ok_or_else(|| anyhow!("expected at least one theme"))?
            .colors;

        for (path, line) in [
            ("vindue/src/routes/sql/+page.svelte", "<script lang=\"ts\">"),
            (
                "src/components/button.jsx",
                "export function Button() { return <button>Save</button>; }",
            ),
            (
                "src/components/button.tsx",
                "export function Button(): JSX.Element { return <button>Save</button>; }",
            ),
        ] {
            let mut painter = SyntaxPainter::for_path(path, colors);
            let parts = painter.highlight(line, colors);
            let foregrounds = parts
                .iter()
                .filter_map(|(style, _)| style.fg)
                .collect::<HashSet<_>>();

            assert!(
                foregrounds.len() > 1,
                "{path} should receive multiple syntax foreground colors"
            );
        }
        Ok(())
    }
}
