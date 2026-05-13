use super::{
    AiLogEvent, AiLogSessionStatus, CommandPromptMode, DiffPane, FileHeatmapSortMode,
    InlineDraftMode, InlineFileMentionState, InlineFileReferencePickerState, SettingsEditorKind,
    ThreadSelectorEntry, TuiApp,
};
use crate::domain::diff::DiffLineKind;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::text::Line;
use std::sync::Arc;

mod diff;
mod helpers;
mod markdown;
mod modals;
mod overlays;
mod sidebar;
mod status;
mod threads;

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
    pub(crate) rendered: Option<Line<'static>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DiffRenderCacheKey {
    pub(crate) file_index: usize,
    pub(crate) pane_inner_width: usize,
    pub(crate) side_by_side_diff: bool,
    pub(crate) search_query: Option<String>,
    pub(crate) selected_line: usize,
    pub(crate) selected_row_range: Option<(usize, usize)>,
    pub(crate) selected_comment_id: Option<u64>,
    pub(crate) expanded_thread_ids: Vec<u64>,
    pub(crate) collapsed_thread_ids: Vec<u64>,
    pub(crate) review_state_code: u8,
    pub(crate) is_active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DiffRenderCacheEntry {
    pub(crate) lines: Arc<[Line<'static>]>,
    pub(crate) row_map: Arc<[usize]>,
    pub(crate) link_hits: Arc<[FileReferenceHit]>,
}

impl DiffRenderCacheEntry {
    pub(crate) fn new(
        lines: Vec<Line<'static>>,
        row_map: Vec<usize>,
        link_hits: Vec<FileReferenceHit>,
    ) -> Self {
        Self {
            lines: Arc::from(lines.into_boxed_slice()),
            row_map: Arc::from(row_map.into_boxed_slice()),
            link_hits: Arc::from(link_hits.into_boxed_slice()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ThreadBodyRenderCacheKind {
    Comment,
    Reply,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ThreadBodyRenderCacheKey {
    pub(crate) thread_id: u64,
    pub(crate) body_id: u64,
    pub(crate) kind: ThreadBodyRenderCacheKind,
    pub(crate) revision_ms: u64,
    pub(crate) body_hash: u64,
    pub(crate) inner_width: usize,
    pub(crate) theme_index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ThreadBodyRenderCacheEntry {
    pub(crate) lines: Arc<[Line<'static>]>,
}

use diff::draw_diff_view_for_pane;
use modals::{draw_commit_picker, draw_review_picker, draw_settings_editor, draw_theme_picker};
use overlays::{
    draw_ai_activity_overlay, draw_ai_progress_popup, draw_code_search, draw_command_palette,
    draw_command_prompt, draw_file_heatmap_overlay, draw_shortcuts_modal,
    draw_thread_navigator_overlay, draw_thread_selector,
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
        || app.thread_selector.is_some()
        || app.code_search.is_some()
        || app.settings_editor.is_some()
        || app.shortcuts_modal_visible
        || app.ai_activity_visible
        || app.file_heatmap.is_some()
        || app.file_heatmap_started_at.is_some();
    app.last_shortcuts_modal_area = None;
    app.last_file_heatmap_area = None;
    app.last_thread_nav_area = None;
    app.last_thread_nav_scroll = 0;
    app.last_thread_nav_row_map.clear();
    app.last_ai_progress_area = None;
    app.last_file_search_area = None;
    app.last_code_search_area = None;
    app.last_ai_activity_area = None;
    app.last_thread_selector_area = None;
    app.last_thread_selector_scroll = 0;
    app.last_thread_selector_visible_rows = 0;
    app.last_code_search_scroll = 0;
    app.last_code_search_visible_rows = 0;
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
        if app.thread_selector.is_some() {
            draw_thread_selector(frame, app);
        }
        if app.code_search.is_some() {
            draw_code_search(frame, app);
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
        if app.ai_activity_visible {
            draw_ai_activity_overlay(frame, app);
        }
        if app.shortcuts_modal_visible {
            draw_shortcuts_modal(frame, app);
        }
        if app.file_heatmap.is_some() || app.file_heatmap_started_at.is_some() {
            draw_file_heatmap_overlay(frame, app);
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
    if app.thread_selector.is_some() {
        draw_thread_selector(frame, app);
    }
    if app.code_search.is_some() {
        draw_code_search(frame, app);
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
    if app.ai_activity_visible {
        draw_ai_activity_overlay(frame, app);
    }
    if app.shortcuts_modal_visible {
        draw_shortcuts_modal(frame, app);
    }
    if app.file_heatmap.is_some() || app.file_heatmap_started_at.is_some() {
        draw_file_heatmap_overlay(frame, app);
    }
}

#[cfg(test)]
mod tests {
    use super::diff::{
        RowSelectionKind, build_side_by_side_row_lines, build_unified_row_lines,
        editor_cursor_visual_position, inline_comment_editor_area, keep_source_row_range_visible,
        resolve_diff_scroll, source_row_visual_range, wrap_editor_buffer_lines,
    };
    use super::helpers::line_plain_text;
    use super::status::{build_status_field_line, truncate_with_ellipsis};
    use super::threads::comment_thread_layout;
    use super::*;
    use crate::domain::diff::DiffLineKind;
    use crate::domain::review::DiffSide;
    use crate::tui::app::state::tests::{
        diff_file_with_context_lines, make_test_app_with_files_and_comments,
    };
    use crate::tui::theme::{default_theme_name, load_themes, resolve_theme_index};
    use anyhow::{Result, anyhow};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::widgets::Paragraph;

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
            rendered: None,
        };
        let highlighted_segments = vec![(
            Style::default().fg(colors.text_primary),
            content.to_string(),
        )];

        let rendered = build_unified_row_lines(
            &row,
            &highlighted_segments,
            RowSelectionKind::None,
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
    fn unified_rows_expand_tabs_before_rendering() -> Result<()> {
        let colors = test_colors()?;
        let pane_inner_width = 40usize;
        let content = "\treturn\tvalue";
        let row = DisplayRow {
            kind: DiffLineKind::Context,
            old_line: Some(7),
            new_line: Some(7),
            raw: format!(" {content}"),
            code: content.to_string(),
            rendered: None,
        };
        let highlighted_segments = vec![(
            Style::default().fg(colors.text_primary),
            content.to_string(),
        )];

        let rendered = build_unified_row_lines(
            &row,
            &highlighted_segments,
            RowSelectionKind::None,
            false,
            pane_inner_width,
            &colors,
        );
        let rendered_text = rendered.iter().map(line_plain_text).collect::<String>();

        assert!(!rendered_text.contains('\t'));
        assert!(rendered_text.contains("    return  value"));
        Ok(())
    }

    #[test]
    fn unified_added_and_removed_rows_use_muted_backgrounds() -> Result<()> {
        let colors = test_colors()?;
        let pane_inner_width = 50usize;
        let highlighted_segments =
            vec![(Style::default().fg(colors.text_primary), "changed".into())];
        let added = DisplayRow {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(8),
            raw: "+changed".to_string(),
            code: "changed".to_string(),
            rendered: None,
        };
        let removed = DisplayRow {
            kind: DiffLineKind::Removed,
            old_line: Some(8),
            new_line: None,
            raw: "-changed".to_string(),
            code: "changed".to_string(),
            rendered: None,
        };
        let context = DisplayRow {
            kind: DiffLineKind::Context,
            old_line: Some(8),
            new_line: Some(8),
            raw: " changed".to_string(),
            code: "changed".to_string(),
            rendered: None,
        };

        let added_lines = build_unified_row_lines(
            &added,
            &highlighted_segments,
            RowSelectionKind::None,
            false,
            pane_inner_width,
            &colors,
        );
        let removed_lines = build_unified_row_lines(
            &removed,
            &highlighted_segments,
            RowSelectionKind::None,
            false,
            pane_inner_width,
            &colors,
        );
        let context_lines = build_unified_row_lines(
            &context,
            &highlighted_segments,
            RowSelectionKind::None,
            false,
            pane_inner_width,
            &colors,
        );

        assert!(added_lines[0].style.bg.is_some());
        assert!(removed_lines[0].style.bg.is_some());
        assert_ne!(added_lines[0].style.bg, removed_lines[0].style.bg);
        assert!(context_lines[0].style.bg.is_none());
        Ok(())
    }

    #[test]
    fn selected_active_diff_row_keeps_selection_background() -> Result<()> {
        let colors = test_colors()?;
        let row = DisplayRow {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(8),
            raw: "+changed".to_string(),
            code: "changed".to_string(),
            rendered: None,
        };
        let highlighted_segments =
            vec![(Style::default().fg(colors.text_primary), "changed".into())];

        let rendered = build_unified_row_lines(
            &row,
            &highlighted_segments,
            RowSelectionKind::Current,
            true,
            50,
            &colors,
        );

        assert_eq!(rendered[0].style.bg, Some(colors.selected_line_bg));
        Ok(())
    }

    #[test]
    fn range_selected_diff_row_uses_distinct_background() -> Result<()> {
        let colors = test_colors()?;
        let row = DisplayRow {
            kind: DiffLineKind::Context,
            old_line: Some(8),
            new_line: Some(8),
            raw: " unchanged".to_string(),
            code: "unchanged".to_string(),
            rendered: None,
        };
        let highlighted_segments =
            vec![(Style::default().fg(colors.text_primary), "unchanged".into())];

        let rendered = build_unified_row_lines(
            &row,
            &highlighted_segments,
            RowSelectionKind::Range,
            true,
            50,
            &colors,
        );

        assert!(rendered[0].style.bg.is_some());
        assert_ne!(rendered[0].style.bg, Some(colors.selected_line_bg));
        Ok(())
    }

    #[test]
    fn side_by_side_added_background_stays_on_right_side() -> Result<()> {
        let colors = test_colors()?;
        let highlighted_segments =
            vec![(Style::default().fg(colors.text_primary), "changed".into())];
        let row = DisplayRow {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(8),
            raw: "+changed".to_string(),
            code: "changed".to_string(),
            rendered: None,
        };

        let rendered = build_side_by_side_row_lines(
            &row,
            &highlighted_segments,
            RowSelectionKind::None,
            false,
            80,
            &colors,
        );

        let text = line_plain_text(&rendered[0]);
        let separator_index = text
            .find(" │ ")
            .ok_or_else(|| anyhow!("side-by-side separator should exist"))?;
        let mut seen_columns = 0usize;
        for span in &rendered[0].spans {
            let span_width = span.content.chars().count();
            if seen_columns + span_width <= separator_index {
                assert!(
                    span.style.bg.is_none(),
                    "left side of added row should not be shaded"
                );
            } else if seen_columns > separator_index {
                assert!(
                    span.style.bg.is_some(),
                    "right side of added row should be shaded"
                );
            }
            seen_columns = seen_columns.saturating_add(span_width);
        }
        Ok(())
    }

    #[test]
    fn side_by_side_wrapped_rows_do_not_exceed_pane_width() -> Result<()> {
        let colors = test_colors()?;
        let pane_inner_width = 48usize;
        let content = "use crate::ir::{EncodingInfo, HeaderIr, MediaTypeIr, Operation, ParameterIr, ParameterLocation};";
        let highlighted_segments = vec![(
            Style::default().fg(colors.text_primary),
            content.to_string(),
        )];
        let row = DisplayRow {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(7),
            raw: format!("+{content}"),
            code: content.to_string(),
            rendered: None,
        };

        let rendered = build_side_by_side_row_lines(
            &row,
            &highlighted_segments,
            RowSelectionKind::Current,
            true,
            pane_inner_width,
            &colors,
        );

        assert!(!rendered.is_empty());
        assert!(
            rendered
                .iter()
                .all(|line| line_plain_text(line).chars().count() == pane_inner_width)
        );
        Ok(())
    }

    #[test]
    fn inline_comment_editor_wraps_long_logical_lines() {
        let lines = vec!["alpha beta infest".to_string()];

        let wrapped = wrap_editor_buffer_lines(&lines, 11);
        let wrapped_text = wrapped
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();

        assert_eq!(wrapped_text, vec!["alpha beta ", "infest"]);
        assert_eq!(editor_cursor_visual_position(&lines, 0, 15, 11), (1, 4));
    }

    #[test]
    fn side_by_side_comment_thread_layout_stays_with_comment_side() {
        let pane_inner_width = 100usize;

        let left = comment_thread_layout(true, DiffSide::Left, pane_inner_width);
        let right = comment_thread_layout(true, DiffSide::Right, pane_inner_width);

        assert!(left.indent < pane_inner_width / 2);
        assert!(left.indent + left.outer_width <= pane_inner_width / 2);
        assert!(right.indent >= pane_inner_width / 2);
        assert!(right.indent + right.outer_width <= pane_inner_width);
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
        let area = Rect {
            x: 10,
            y: 5,
            width: 140,
            height: 28,
        };

        let editor =
            inline_comment_editor_area(area).ok_or_else(|| anyhow!("editor should fit"))?;

        assert_eq!(editor.x, 11);
        assert_eq!(editor.width, 88);
        assert_eq!(editor.height, 12);
        assert_eq!(editor.y, 21);
        Ok(())
    }

    #[test]
    fn diff_render_clears_stale_cells_before_drawing_visible_rows() -> Result<()> {
        let mut app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines("src/a.rs", &[(1, "short")])],
            Vec::new(),
        )?;
        let mut terminal = Terminal::new(TestBackend::new(80, 16))?;
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
        };

        terminal.draw(|frame| {
            frame.render_widget(Paragraph::new("Z".repeat(80 * 12)), area);
        })?;
        terminal.draw(|frame| {
            diff::draw_diff_view_for_pane(frame, &mut app, area, DiffPane::Primary);
        })?;

        let stale_cell_found = (area.y..area.y + area.height).any(|y| {
            (area.x..area.x + area.width)
                .any(|x| terminal.backend().buffer()[(x, y)].symbol() == "Z")
        });
        assert!(!stale_cell_found);
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
    fn diff_scroll_does_not_jump_to_bottom_when_selection_nears_end() {
        let scroll = resolve_diff_scroll(70, 100, 10, (80, 80), None);

        assert_eq!(scroll, 71);
    }

    #[test]
    fn diff_scroll_keeps_pending_anchor_visible_without_skipping() {
        let scroll = resolve_diff_scroll(0, 100, 10, (42, 42), Some(42));

        assert_eq!(scroll, 33);
    }
}
