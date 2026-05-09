use std::{path::Path, sync::LazyLock};

use ratatui::style::{Color, Modifier, Style};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::{SyntaxReference, SyntaxSet},
};

use super::theme::ThemeColors;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_DARK: LazyLock<Theme> = LazyLock::new(|| {
    let set = ThemeSet::load_defaults();
    set.themes
        .get("base16-ocean.dark")
        .cloned()
        .unwrap_or_default()
});
static THEME_LIGHT: LazyLock<Theme> = LazyLock::new(|| {
    let set = ThemeSet::load_defaults();
    set.themes
        .get("InspiredGitHub")
        .or_else(|| set.themes.get("base16-ocean.light"))
        .or_else(|| set.themes.get("Solarized (light)"))
        .cloned()
        .unwrap_or_default()
});

pub struct SyntaxPainter {
    highlighter: HighlightLines<'static>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticTokenKind {
    Plain,
    Keyword,
    String,
    Number,
    Type,
    Constant,
    Comment,
    Operator,
    Function,
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
        .or_else(|| fallback_known_syntax(lower_extension.as_deref(), lower_filename.as_deref()))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text())
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

    if matches!(extension, Some("toml")) || matches!(filename, Some("cargo.toml")) {
        return SYNTAX_SET
            .find_syntax_by_name("TOML")
            .or_else(|| SYNTAX_SET.find_syntax_by_token("Cargo.toml"))
            .or_else(|| SYNTAX_SET.find_syntax_by_extension("toml"))
            .or_else(|| SYNTAX_SET.find_syntax_by_extension("yaml"))
            .or_else(|| SYNTAX_SET.find_syntax_by_extension("ini"))
            .or_else(|| SYNTAX_SET.find_syntax_by_extension("rs"));
    }

    None
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
    token_text: &str,
    theme: &ThemeColors,
) -> Style {
    let base_fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let semantic = classify_semantic_token(token_text, style.font_style);
    let fg = semantic_fg_for_kind(semantic, base_fg, theme);
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

fn classify_semantic_token(token_text: &str, font_style: FontStyle) -> SemanticTokenKind {
    let trimmed = token_text.trim();
    if trimmed.is_empty() {
        return SemanticTokenKind::Plain;
    }
    if is_comment_like(trimmed, font_style) {
        return SemanticTokenKind::Comment;
    }
    if is_keyword_token(trimmed) {
        return SemanticTokenKind::Keyword;
    }
    if is_string_like(trimmed) {
        return SemanticTokenKind::String;
    }
    if is_number_like(trimmed) {
        return SemanticTokenKind::Number;
    }
    if is_constant_like(trimmed) {
        return SemanticTokenKind::Constant;
    }
    if is_function_like(trimmed) {
        return SemanticTokenKind::Function;
    }
    if is_type_like(trimmed) {
        return SemanticTokenKind::Type;
    }
    if is_operator_like(trimmed) {
        return SemanticTokenKind::Operator;
    }
    SemanticTokenKind::Plain
}

fn semantic_fg_for_kind(kind: SemanticTokenKind, base_fg: Color, theme: &ThemeColors) -> Color {
    let base = normalize_diff_fg(base_fg, theme);
    let target = match kind {
        SemanticTokenKind::Plain => return base,
        SemanticTokenKind::Keyword => theme.accent,
        SemanticTokenKind::String => theme.reply_title,
        SemanticTokenKind::Number => theme.hunk_header,
        SemanticTokenKind::Type => theme.comment_title,
        SemanticTokenKind::Constant => theme.added_sign,
        SemanticTokenKind::Comment => theme.text_muted,
        SemanticTokenKind::Operator => theme.context_sign,
        SemanticTokenKind::Function => theme.markdown_heading,
    };
    let Some(base_rgb) = color_to_rgb(base) else {
        return base;
    };
    let Some(target_rgb) = color_to_rgb(target) else {
        return base;
    };
    let blend_amount = match kind {
        SemanticTokenKind::Comment => 0.75,
        SemanticTokenKind::Keyword => 0.58,
        SemanticTokenKind::String => 0.46,
        SemanticTokenKind::Number => 0.42,
        SemanticTokenKind::Type => 0.40,
        SemanticTokenKind::Constant => 0.50,
        SemanticTokenKind::Operator => 0.35,
        SemanticTokenKind::Function => 0.45,
        SemanticTokenKind::Plain => 0.0,
    };
    let blended = blend_rgb(base_rgb, target_rgb, blend_amount);
    normalize_diff_fg(Color::Rgb(blended.0, blended.1, blended.2), theme)
}

fn is_comment_like(token: &str, font_style: FontStyle) -> bool {
    token.starts_with("//")
        || token.starts_with("/*")
        || token.starts_with('*')
        || token.starts_with("--")
        || (font_style.contains(FontStyle::ITALIC) && token.len() > 1)
}

fn is_string_like(token: &str) -> bool {
    token.starts_with('"')
        || token.ends_with('"')
        || token.starts_with('\'')
        || token.ends_with('\'')
        || token.starts_with('`')
        || token.ends_with('`')
        || token.starts_with("r\"")
        || token.starts_with("r#\"")
        || token.starts_with("b\"")
}

fn is_number_like(token: &str) -> bool {
    let mut has_digit = false;
    for ch in token.chars() {
        if ch.is_ascii_digit() {
            has_digit = true;
            continue;
        }
        if matches!(
            ch,
            '_' | '.' | '-' | '+' | 'x' | 'X' | 'o' | 'O' | 'b' | 'B' | 'e' | 'E'
        ) {
            continue;
        }
        if matches!(ch, 'a'..='f' | 'A'..='F') {
            continue;
        }
        return false;
    }
    has_digit
}

fn is_constant_like(token: &str) -> bool {
    let mut has_alpha = false;
    let mut has_lower = false;
    for ch in token.chars() {
        if ch.is_ascii_alphabetic() {
            has_alpha = true;
            if ch.is_ascii_lowercase() {
                has_lower = true;
            }
        } else if !(ch.is_ascii_digit() || ch == '_') {
            return false;
        }
    }
    has_alpha && !has_lower && token.chars().count() >= 2
}

fn is_type_like(token: &str) -> bool {
    let first = token.chars().next();
    first.is_some_and(|ch| ch.is_ascii_uppercase())
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':'))
}

fn is_function_like(token: &str) -> bool {
    token.ends_with('(') || token.ends_with("()")
}

fn is_operator_like(token: &str) -> bool {
    token.chars().all(|ch| {
        matches!(
            ch,
            '+' | '-'
                | '*'
                | '/'
                | '%'
                | '='
                | '!'
                | '<'
                | '>'
                | '&'
                | '|'
                | '^'
                | '~'
                | '?'
                | ':'
                | ','
                | ';'
                | '.'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
        )
    })
}

fn is_keyword_token(token: &str) -> bool {
    matches!(
        token,
        "fn" | "let"
            | "mut"
            | "const"
            | "static"
            | "pub"
            | "impl"
            | "trait"
            | "enum"
            | "struct"
            | "type"
            | "match"
            | "if"
            | "else"
            | "for"
            | "while"
            | "loop"
            | "return"
            | "break"
            | "continue"
            | "mod"
            | "use"
            | "crate"
            | "super"
            | "self"
            | "Self"
            | "where"
            | "as"
            | "in"
            | "async"
            | "await"
            | "move"
            | "unsafe"
            | "extern"
            | "true"
            | "false"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "class"
            | "interface"
            | "function"
            | "var"
            | "new"
            | "import"
            | "from"
            | "export"
            | "default"
            | "def"
            | "elif"
            | "try"
            | "except"
            | "finally"
            | "with"
            | "yield"
            | "lambda"
            | "pass"
            | "raise"
            | "global"
            | "nonlocal"
            | "del"
            | "is"
            | "not"
            | "and"
            | "or"
            | "null"
            | "undefined"
            | "extends"
            | "typeof"
            | "instanceof"
            | "switch"
            | "case"
            | "catch"
            | "throw"
            | "package"
            | "func"
            | "defer"
            | "go"
            | "select"
            | "chan"
            | "map"
            | "public"
            | "private"
            | "protected"
            | "final"
            | "void"
            | "int"
            | "long"
            | "float"
            | "double"
            | "char"
            | "boolean"
            | "namespace"
            | "template"
            | "typename"
            | "include"
            | "define"
    )
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
    use syntect::highlighting::FontStyle;

    use super::{SemanticTokenKind, classify_semantic_token, syntax_for_path};

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
    fn semantic_classifier_detects_keywords() {
        assert_eq!(
            classify_semantic_token("fn", FontStyle::empty()),
            SemanticTokenKind::Keyword
        );
        assert_eq!(
            classify_semantic_token("return", FontStyle::empty()),
            SemanticTokenKind::Keyword
        );
        assert_eq!(
            classify_semantic_token("def", FontStyle::empty()),
            SemanticTokenKind::Keyword
        );
        assert_eq!(
            classify_semantic_token("const", FontStyle::empty()),
            SemanticTokenKind::Keyword
        );
    }

    #[test]
    fn semantic_classifier_detects_done_comment_tokens() {
        assert_eq!(
            classify_semantic_token("// done", FontStyle::ITALIC),
            SemanticTokenKind::Comment
        );
    }

    #[test]
    fn semantic_classifier_detects_types_and_numbers() {
        assert_eq!(
            classify_semantic_token("ReviewSession", FontStyle::empty()),
            SemanticTokenKind::Type
        );
        assert_eq!(
            classify_semantic_token("42", FontStyle::empty()),
            SemanticTokenKind::Number
        );
    }
}
