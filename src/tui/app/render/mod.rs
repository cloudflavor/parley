mod diff;
mod helpers;
mod markdown;
mod modals;
mod overlays;
mod sidebar;
mod status;
mod threads;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::Line,
};

use crate::domain::diff::DiffLineKind;

use super::ThreadDensityMode;
use super::{
    CommandPromptMode, DiffPane, InlineDraftMode, InlineFileMentionState,
    InlineFileReferencePickerState, SettingsEditorKind, TuiApp,
};

const INLINE_FILE_MENTION_MAX_VISIBLE_ROWS: usize = 6;

pub(crate) type HighlightParts = Vec<(Style, String)>;

#[derive(Debug, Clone)]
pub(crate) struct FileReferenceHit {
    pub(crate) rendered_row_index: usize,
    pub(crate) col_start: usize,
    pub(crate) col_end: usize,
    pub(crate) path: String,
    pub(crate) line: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct DisplayRow {
    pub(crate) kind: DiffLineKind,
    pub(crate) old_line: Option<u32>,
    pub(crate) new_line: Option<u32>,
    pub(crate) raw: String,
    pub(crate) code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DiffRenderCacheKey {
    pub(crate) file_index: usize,
    pub(crate) pane_inner_width: usize,
    pub(crate) side_by_side_diff: bool,
    pub(crate) search_query: Option<String>,
    pub(crate) thread_density_mode: ThreadDensityMode,
    pub(crate) selected_line: usize,
    pub(crate) selected_row_range: Option<(usize, usize)>,
    pub(crate) selected_comment_id: Option<u64>,
    pub(crate) expanded_thread_ids: Vec<u64>,
    pub(crate) review_state_code: u8,
    pub(crate) is_active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DiffRenderCacheEntry {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) row_map: Vec<usize>,
    pub(crate) link_hits: Vec<FileReferenceHit>,
}

use diff::draw_diff_view_for_pane;
use modals::{draw_commit_picker, draw_review_picker, draw_settings_editor, draw_theme_picker};
use overlays::{
    draw_ai_progress_popup, draw_command_palette, draw_command_prompt, draw_shortcuts_modal,
    draw_thread_navigator_overlay,
};
use sidebar::draw_file_sidebar;
use status::{compute_status_height, draw_status_panel, draw_status_toast};

pub(super) fn draw(frame: &mut Frame<'_>, app: &mut TuiApp) {
    app.refresh_status_toast();
    let root = frame.area();
    let blocking_overlay_visible = app.command_prompt.is_some()
        || app.command_palette.is_some()
        || app.theme_picker.is_some()
        || app.commit_picker.is_some()
        || app.review_picker.is_some()
        || app.settings_editor.is_some()
        || app.shortcuts_modal_visible;
    app.last_shortcuts_modal_area = None;
    app.last_thread_nav_area = None;
    app.last_thread_nav_scroll = 0;
    app.last_thread_nav_row_map.clear();
    app.last_ai_progress_area = None;
    app.last_file_search_area = None;
    app.last_diff_area_secondary = None;
    app.last_diff_scroll_secondary = 0;
    app.last_diff_row_map_secondary.clear();
    app.last_diff_link_hits.clear();
    app.last_diff_link_hits_secondary.clear();
    let status_height = compute_status_height(root.height);

    if app.content_fullscreen {
        app.last_file_area = None;
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(status_height)])
            .split(root);
        draw_diff_view_for_pane(frame, app, sections[0], app.active_diff_pane);
        draw_status_panel(frame, app, sections[1]);
        if !blocking_overlay_visible && !app.ai_progress_visible {
            draw_status_toast(frame, app, sections[1]);
        }
        if app.thread_nav_visible {
            draw_thread_navigator_overlay(frame, app);
        }
        if app.settings_editor.is_some() {
            draw_settings_editor(frame, app);
        }
        if app.theme_picker.is_some() {
            draw_theme_picker(frame, app);
        }
        if app.commit_picker.is_some() {
            draw_commit_picker(frame, app);
        }
        if app.review_picker.is_some() {
            draw_review_picker(frame, app);
        }
        if app.command_prompt.is_some() {
            draw_command_prompt(frame, app);
        }
        if app.command_palette.is_some() {
            draw_command_palette(frame, app);
        }
        if app.ai_progress_visible && !blocking_overlay_visible {
            draw_ai_progress_popup(frame, app);
        }
        if app.shortcuts_modal_visible {
            draw_shortcuts_modal(frame, app);
        }
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(status_height)])
        .split(root);

    let file_pane_width = app.computed_file_pane_width(sections[0].width);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(file_pane_width), Constraint::Min(0)])
        .split(sections[0]);

    draw_file_sidebar(frame, app, columns[0]);
    if app.split_diff_view {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(columns[1]);
        draw_diff_view_for_pane(frame, app, panes[0], DiffPane::Primary);
        draw_diff_view_for_pane(frame, app, panes[1], DiffPane::Secondary);
    } else {
        draw_diff_view_for_pane(frame, app, columns[1], app.active_diff_pane);
    }
    draw_status_panel(frame, app, sections[1]);
    if !blocking_overlay_visible && !app.ai_progress_visible {
        draw_status_toast(frame, app, sections[1]);
    }
    if app.thread_nav_visible {
        draw_thread_navigator_overlay(frame, app);
    }
    if app.settings_editor.is_some() {
        draw_settings_editor(frame, app);
    }
    if app.theme_picker.is_some() {
        draw_theme_picker(frame, app);
    }
    if app.commit_picker.is_some() {
        draw_commit_picker(frame, app);
    }
    if app.review_picker.is_some() {
        draw_review_picker(frame, app);
    }
    if app.command_prompt.is_some() {
        draw_command_prompt(frame, app);
    }
    if app.command_palette.is_some() {
        draw_command_palette(frame, app);
    }
    if app.ai_progress_visible && !blocking_overlay_visible {
        draw_ai_progress_popup(frame, app);
    }
    if app.shortcuts_modal_visible {
        draw_shortcuts_modal(frame, app);
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};

    use super::diff::{
        build_unified_row_lines, inline_comment_editor_area, keep_source_row_range_visible,
        source_row_visual_range,
    };
    use super::helpers::line_plain_text;
    use super::status::{build_status_field_line, truncate_with_ellipsis};
    use super::*;
    use crate::domain::diff::DiffLineKind;
    use crate::tui::theme::{default_theme_name, load_themes, resolve_theme_index};

    fn test_colors() -> Result<crate::tui::theme::ThemeColors> {
        let themes = load_themes()?;
        let index = resolve_theme_index(&themes, default_theme_name()).unwrap_or(0);
        Ok(themes[index].colors.clone())
    }

    #[test]
    fn unified_wrapped_rows_preserve_content_and_fit_width() -> Result<()> {
        let colors = test_colors()?;
        let pane_inner_width = 80usize;
        let content = "The default build uses a console renderer so the daemon core can compile and run in environments without GTK system packages.";
        let row = DisplayRow {
            kind: DiffLineKind::Context,
            old_line: Some(5),
            new_line: Some(5),
            raw: content.to_string(),
            code: content.to_string(),
        };
        let highlighted_segments = vec![(
            Style::default().fg(colors.text_primary),
            content.to_string(),
        )];

        let rendered = build_unified_row_lines(
            &row,
            &highlighted_segments,
            false,
            false,
            pane_inner_width,
            &colors,
        );

        assert!(rendered.len() > 1, "expected wrapped output");

        for line in &rendered {
            let width = line_plain_text(line).chars().count();
            assert_eq!(
                width, pane_inner_width,
                "rendered row overflowed pane width"
            );
        }

        let reassembled = rendered
            .iter()
            .map(line_plain_text)
            .map(|line| line.chars().skip(16).collect::<String>())
            .map(|line| line.trim_end().to_string())
            .collect::<String>();
        assert_eq!(reassembled, content);
        Ok(())
    }

    #[test]
    fn status_panel_height_grows_when_terminal_has_room() {
        assert_eq!(compute_status_height(11), 3);
        assert_eq!(compute_status_height(12), 4);
        assert_eq!(compute_status_height(16), 4);
    }

    #[test]
    fn status_field_line_respects_available_width() -> Result<()> {
        let colors = test_colors()?;
        let line = build_status_field_line(
            &[
                (
                    "Mode",
                    "AI running".to_string(),
                    Style::default().fg(colors.text_primary),
                ),
                (
                    "Review",
                    "demo:open".to_string(),
                    Style::default().fg(colors.accent),
                ),
                (
                    "Keys",
                    "Ctrl+k commands".to_string(),
                    Style::default().fg(colors.status_help),
                ),
            ],
            42,
            &colors,
        );

        let rendered = line_plain_text(&line);
        assert_eq!(rendered.chars().count(), 42);
        assert!(rendered.contains("Mode"));
        assert!(rendered.contains("Review"));
        Ok(())
    }

    #[test]
    fn truncate_with_ellipsis_shortens_without_overflow() {
        assert_eq!(truncate_with_ellipsis("abc", 5), "abc");
        assert_eq!(truncate_with_ellipsis("abcdef", 4), "abc…");
        assert_eq!(truncate_with_ellipsis("abcdef", 1), "…");
    }

    #[test]
    fn inline_comment_editor_area_is_fixed_width_and_left_anchored() -> Result<()> {
        use ratatui::layout::Rect;

        let area = Rect {
            x: 10,
            y: 5,
            width: 140,
            height: 28,
        };

        let editor =
            inline_comment_editor_area(area).ok_or_else(|| anyhow!("editor should fit"))?;

        assert_eq!(editor.x, 11);
        assert_eq!(editor.width, 68);
        assert_eq!(editor.height, 10);
        assert_eq!(editor.y, 23);
        Ok(())
    }

    #[test]
    fn selected_source_row_range_includes_thread_replies() {
        let row_map = vec![0, 1, 1, 1, 2];

        assert_eq!(source_row_visual_range(&row_map, 1), Some((1, 3)));
    }

    #[test]
    fn viewport_can_scroll_within_selected_thread_range() {
        let scroll = keep_source_row_range_visible(3, 2, (1, 5));

        assert_eq!(scroll, 3);
    }

    #[test]
    fn viewport_moves_to_selected_thread_range_when_hidden_above() {
        let scroll = keep_source_row_range_visible(5, 2, (0, 1));

        assert_eq!(scroll, 1);
    }

    #[test]
    fn last_line_comment_should_force_max_scroll() {
        let range = (49, 52);
        let lines_len = 53;
        let viewport_height = 20;
        let max_scroll = lines_len - viewport_height;
        let mut scroll = 30;
        let end_proximity_threshold = (lines_len as f64 * 0.8) as usize;
        if range.1 >= end_proximity_threshold || range.0 >= end_proximity_threshold {
            scroll = max_scroll;
        }

        assert_eq!(scroll, 33);
    }
}
