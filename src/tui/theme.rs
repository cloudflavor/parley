use anyhow::{Context, Result, anyhow};
use include_dir::{Dir, include_dir};
use ratatui::style::Color;
use serde::Deserialize;

const DEFAULT_THEME_NAME: &str = "default";
static THEMES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/tui/themes");

#[derive(Debug, Clone)]
pub struct UiTheme {
    pub name: String,
    pub colors: ThemeColors,
}

#[derive(Debug, Clone)]
pub struct ThemeColors {
    pub accent: Color,
    pub text_primary: Color,
    pub text_muted: Color,
    pub sidebar_highlight_bg: Color,
    pub sidebar_highlight_fg: Color,
    pub selected_line_bg: Color,
    pub selection_marker: Color,
    pub added_sign: Color,
    pub removed_sign: Color,
    pub context_sign: Color,
    pub hunk_header: Color,
    pub meta: Color,
    pub thread_border: Color,
    pub thread_background: Color,
    pub comment_title: Color,
    pub reply_title: Color,
    pub status_help: Color,
    pub markdown_heading: Color,
    pub markdown_quote_mark: Color,
    pub markdown_quote_text: Color,
    pub markdown_bullet: Color,
    pub markdown_fence: Color,
    pub markdown_code_fg: Color,
    pub markdown_code_bg: Color,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeDefinition {
    name: String,
    colors: ThemeColorDefinition,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeColorDefinition {
    accent: String,
    text_primary: String,
    text_muted: String,
    sidebar_highlight_bg: String,
    sidebar_highlight_fg: String,
    selected_line_bg: String,
    selection_marker: String,
    added_sign: String,
    removed_sign: String,
    context_sign: String,
    hunk_header: String,
    meta: String,
    thread_border: String,
    thread_background: String,
    comment_title: String,
    reply_title: String,
    status_help: String,
    markdown_heading: String,
    markdown_quote_mark: String,
    markdown_quote_text: String,
    markdown_bullet: String,
    markdown_fence: String,
    markdown_code_fg: String,
    markdown_code_bg: String,
}

pub fn load_themes() -> Result<Vec<UiTheme>> {
    let mut themes = Vec::new();

    for file in THEMES_DIR.files() {
        if file.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let content = file
            .contents_utf8()
            .ok_or_else(|| anyhow!("theme file is not utf-8: {}", file.path().display()))?;
        let parsed: ThemeDefinition = serde_json::from_str(content)
            .with_context(|| format!("failed to parse theme file {}", file.path().display()))?;
        themes.push(UiTheme::from_definition(parsed)?);
    }

    themes.sort_by(|a, b| a.name.cmp(&b.name));

    if themes.is_empty() {
        return Err(anyhow!("no embedded themes found"));
    }

    Ok(themes)
}

pub fn resolve_theme_index(themes: &[UiTheme], name: &str) -> Option<usize> {
    themes
        .iter()
        .position(|theme| theme.name.eq_ignore_ascii_case(name))
}

pub fn default_theme_name() -> &'static str {
    DEFAULT_THEME_NAME
}

impl UiTheme {
    fn from_definition(value: ThemeDefinition) -> Result<Self> {
        let colors = value.colors;
        let mut parsed_colors = ThemeColors {
            accent: parse_color("accent", &colors.accent)?,
            text_primary: parse_color("text_primary", &colors.text_primary)?,
            text_muted: parse_color("text_muted", &colors.text_muted)?,
            sidebar_highlight_bg: parse_color(
                "sidebar_highlight_bg",
                &colors.sidebar_highlight_bg,
            )?,
            sidebar_highlight_fg: parse_color(
                "sidebar_highlight_fg",
                &colors.sidebar_highlight_fg,
            )?,
            selected_line_bg: parse_color("selected_line_bg", &colors.selected_line_bg)?,
            selection_marker: parse_color("selection_marker", &colors.selection_marker)?,
            added_sign: parse_color("added_sign", &colors.added_sign)?,
            removed_sign: parse_color("removed_sign", &colors.removed_sign)?,
            context_sign: parse_color("context_sign", &colors.context_sign)?,
            hunk_header: parse_color("hunk_header", &colors.hunk_header)?,
            meta: parse_color("meta", &colors.meta)?,
            thread_border: parse_color("thread_border", &colors.thread_border)?,
            thread_background: parse_color("thread_background", &colors.thread_background)?,
            comment_title: parse_color("comment_title", &colors.comment_title)?,
            reply_title: parse_color("reply_title", &colors.reply_title)?,
            status_help: parse_color("status_help", &colors.status_help)?,
            markdown_heading: parse_color("markdown_heading", &colors.markdown_heading)?,
            markdown_quote_mark: parse_color("markdown_quote_mark", &colors.markdown_quote_mark)?,
            markdown_quote_text: parse_color("markdown_quote_text", &colors.markdown_quote_text)?,
            markdown_bullet: parse_color("markdown_bullet", &colors.markdown_bullet)?,
            markdown_fence: parse_color("markdown_fence", &colors.markdown_fence)?,
            markdown_code_fg: parse_color("markdown_code_fg", &colors.markdown_code_fg)?,
            markdown_code_bg: parse_color("markdown_code_bg", &colors.markdown_code_bg)?,
        };
        normalize_text_contrast(&mut parsed_colors);

        Ok(Self {
            name: value.name,
            colors: parsed_colors,
        })
    }
}

fn normalize_text_contrast(colors: &mut ThemeColors) {
    let is_dark = perceived_luminance(colors.thread_background)
        .map(|lum| lum < 0.5)
        .unwrap_or(true);
    if is_dark {
        colors.text_primary = ensure_min_luminance(colors.text_primary, 0.87);
        colors.text_muted = ensure_min_luminance(colors.text_muted, 0.70);
        colors.status_help = ensure_min_luminance(colors.status_help, 0.66);
        colors.context_sign = ensure_min_luminance(colors.context_sign, 0.66);
        colors.meta = ensure_min_luminance(colors.meta, 0.76);
        colors.sidebar_highlight_fg = ensure_min_luminance(colors.sidebar_highlight_fg, 0.90);
        colors.markdown_quote_text = ensure_min_luminance(colors.markdown_quote_text, 0.74);
        colors.markdown_quote_mark = ensure_min_luminance(colors.markdown_quote_mark, 0.67);
    } else {
        colors.text_primary = ensure_max_luminance(colors.text_primary, 0.13);
        colors.text_muted = ensure_max_luminance(colors.text_muted, 0.30);
        colors.status_help = ensure_max_luminance(colors.status_help, 0.34);
        colors.context_sign = ensure_max_luminance(colors.context_sign, 0.34);
        colors.meta = ensure_max_luminance(colors.meta, 0.24);
        colors.sidebar_highlight_fg = ensure_max_luminance(colors.sidebar_highlight_fg, 0.12);
        colors.markdown_quote_text = ensure_max_luminance(colors.markdown_quote_text, 0.26);
        colors.markdown_quote_mark = ensure_max_luminance(colors.markdown_quote_mark, 0.34);
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
            for _ in 0..12 {
                out = blend_rgb(out, (255, 255, 255), 0.22);
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
            for _ in 0..12 {
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

fn parse_color(field: &str, value: &str) -> Result<Color> {
    if let Some(hex) = value.strip_prefix('#')
        && hex.len() == 6
    {
        let red = u8::from_str_radix(&hex[0..2], 16)
            .with_context(|| format!("invalid red channel for {field}: {value}"))?;
        let green = u8::from_str_radix(&hex[2..4], 16)
            .with_context(|| format!("invalid green channel for {field}: {value}"))?;
        let blue = u8::from_str_radix(&hex[4..6], 16)
            .with_context(|| format!("invalid blue channel for {field}: {value}"))?;
        return Ok(Color::Rgb(red, green, blue));
    }

    Err(anyhow!(
        "invalid color for {field}: {value} (expected #RRGGBB)"
    ))
}
