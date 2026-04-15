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
        Ok(Self {
            name: value.name,
            colors: ThemeColors {
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
                markdown_quote_mark: parse_color(
                    "markdown_quote_mark",
                    &colors.markdown_quote_mark,
                )?,
                markdown_quote_text: parse_color(
                    "markdown_quote_text",
                    &colors.markdown_quote_text,
                )?,
                markdown_bullet: parse_color("markdown_bullet", &colors.markdown_bullet)?,
                markdown_fence: parse_color("markdown_fence", &colors.markdown_fence)?,
                markdown_code_fg: parse_color("markdown_code_fg", &colors.markdown_code_fg)?,
                markdown_code_bg: parse_color("markdown_code_bg", &colors.markdown_code_bg)?,
            },
        })
    }
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
