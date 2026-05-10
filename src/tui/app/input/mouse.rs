use super::*;
use crate::utils::cast::{usize_to_isize_saturating, usize_to_u16_saturating};

impl TuiApp {
    pub(in crate::tui::app) async fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.file_heatmap.is_some() || self.file_heatmap_started_at.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.scroll_file_heatmap(-3);
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_file_heatmap(3);
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if self
                        .last_file_heatmap_area
                        .is_some_and(|area| !point_in_rect(mouse.column, mouse.row, area))
                    {
                        self.close_file_heatmap();
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        if self.shortcuts_modal_visible {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_sub(2);
                }
                MouseEventKind::ScrollDown => {
                    self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_add(2);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.ai_activity_visible {
            if let Some(area) = self.last_ai_activity_area
                && point_in_rect(mouse.column, mouse.row, area)
            {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        self.ai_activity_scroll_up(3);
                    }
                    MouseEventKind::ScrollDown => {
                        self.ai_activity_scroll_down(3);
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if mouse.row > area.y
                            && mouse.row < area.y + area.height.saturating_sub(1) =>
                    {
                        let view_row = usize::from(mouse.row.saturating_sub(area.y + 2));
                        self.ai_activity_selected =
                            self.ai_activity_scroll.saturating_add(view_row);
                        self.ai_activity_jump_selected();
                    }
                    _ => {}
                }
            }
            return Ok(());
        }

        if self.thread_selector.is_some() {
            if let Some(area) = self.last_thread_selector_area
                && point_in_rect(mouse.column, mouse.row, area)
            {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        if let Some(selector) = self.thread_selector.as_mut() {
                            selector.selected_index = selector.selected_index.saturating_sub(3);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        let max_index = self
                            .filtered_thread_selector_entries()
                            .len()
                            .saturating_sub(1);
                        if let Some(selector) = self.thread_selector.as_mut() {
                            selector.selected_index = (selector.selected_index + 3).min(max_index);
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if mouse.row > area.y.saturating_add(2)
                            && mouse.row < area.y + area.height.saturating_sub(1) =>
                    {
                        let view_row = usize::from(mouse.row.saturating_sub(area.y + 2));
                        let index = self.last_thread_selector_scroll.saturating_add(view_row);
                        let entry = self.filtered_thread_selector_entries().get(index).cloned();
                        if let Some(selector) = self.thread_selector.as_mut() {
                            selector.selected_index = index;
                        }
                        if let Some(entry) = entry {
                            self.jump_to_thread_selector_entry(&entry);
                        }
                    }
                    _ => {}
                }
            }
            return Ok(());
        }

        if self.inline_file_reference_picker_active() {
            self.handle_inline_file_reference_picker_mouse(mouse);
            self.constrain_selection();
            return Ok(());
        }

        if self.code_search.is_some() {
            self.handle_code_search_mouse(mouse).await?;
            return Ok(());
        }

        if self.command_palette.is_some()
            || self.theme_picker.is_some()
            || self.commit_picker.is_some()
            || self.settings_editor.is_some()
            || self.command_prompt.is_some()
        {
            return Ok(());
        }

        if let Some(ai_area) = self.last_ai_progress_area
            && point_in_rect(mouse.column, mouse.row, ai_area)
        {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.ai_progress_scroll_up(2);
                }
                MouseEventKind::ScrollDown => {
                    self.ai_progress_scroll_down(2);
                }
                _ => {}
            }
            return Ok(());
        }

        if let Some(thread_area) = self.last_thread_nav_area
            && point_in_rect(mouse.column, mouse.row, thread_area)
        {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > thread_area.y
                        && mouse.row < thread_area.y + thread_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(thread_area.y + 1));
                    let row_index = self.last_thread_nav_scroll + view_row;
                    if let Some(&comment_index) = self.last_thread_nav_row_map.get(row_index)
                        && comment_index != usize::MAX
                    {
                        self.selected_comment = comment_index;
                        self.focus_selected_comment_line();
                        if let Some(comment) = self.selected_comment_details() {
                            self.status_line = format!(
                                "selected thread #{} at {}",
                                comment.id,
                                format_comment_reference(comment)
                            );
                        }
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.selected_comment = self.selected_comment.saturating_sub(1);
                    self.focus_selected_comment_line();
                }
                MouseEventKind::ScrollDown => {
                    let max = self.comments_for_selected_file().len().saturating_sub(1);
                    self.selected_comment = (self.selected_comment + 1).min(max);
                    self.focus_selected_comment_line();
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(file_area) = self.last_file_area
            && point_in_rect(mouse.column, mouse.row, file_area)
        {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > file_area.y
                        && mouse.row < file_area.y + file_area.height.saturating_sub(1) =>
                {
                    let visual_row = self.last_file_scroll
                        + usize::from(mouse.row.saturating_sub(file_area.y + 1));
                    if let Some(Some(file_index)) = self.last_file_row_map.get(visual_row) {
                        self.select_file(*file_index);
                        if self.active_file_index() < self.diff.files.len() {
                            self.status_line = format!(
                                "selected file {}",
                                self.diff.files[self.active_file_index()].path
                            );
                        }
                    } else if let Some(Some(group)) =
                        self.last_file_group_map.get(visual_row).cloned()
                    {
                        self.toggle_file_group_collapsed(&group);
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.move_file_selection(-usize_to_isize_saturating(
                        MOUSE_WHEEL_FILE_SCROLL_FILES,
                    ));
                }
                MouseEventKind::ScrollDown => {
                    self.move_file_selection(usize_to_isize_saturating(
                        MOUSE_WHEEL_FILE_SCROLL_FILES,
                    ));
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(search_area) = self.last_file_search_area
            && point_in_rect(mouse.column, mouse.row, search_area)
        {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && mouse.row > search_area.y
                && mouse.row < search_area.y + search_area.height.saturating_sub(1)
            {
                const SEARCH_PREFIX: &str = "search> ";
                let inner_width = usize::from(search_area.width.saturating_sub(2)).max(1);
                let query_width = inner_width.saturating_sub(SEARCH_PREFIX.chars().count());
                let horizontal_scroll = self
                    .file_search
                    .cursor_col
                    .saturating_sub(query_width.saturating_sub(1));
                let content_start = search_area
                    .x
                    .saturating_add(1)
                    .saturating_add(usize_to_u16_saturating(SEARCH_PREFIX.chars().count()));
                let clicked_col = usize::from(mouse.column.saturating_sub(content_start));
                let target_col = horizontal_scroll.saturating_add(clicked_col);
                self.file_search.focused = true;
                self.file_search.cursor_col =
                    target_col.min(self.file_search.query.chars().count());
                self.status_line = "file filter input focused".into();
            }
            return Ok(());
        }

        if let Some(diff_area) = self.last_diff_area
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.activate_pane(DiffPane::Primary);
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll + view_row;
                    let content_col =
                        usize::from(mouse.column.saturating_sub(diff_area.x.saturating_add(1)));
                    if let Some((path, line)) = self.resolve_file_reference_hit(
                        DiffPane::Primary,
                        visible_row_index,
                        content_col,
                    ) {
                        self.follow_file_reference(DiffPane::Primary, &path, line);
                        return Ok(());
                    }
                    if let Some(row_index) = self.last_diff_row_map.get(visible_row_index).copied()
                    {
                        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                            self.extend_comment_line_selection_to(DiffPane::Primary, row_index);
                        } else {
                            self.set_active_line_index(row_index);
                        }
                        self.set_visual_row_for_pane(DiffPane::Primary, Some(visible_row_index));
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.scroll_active_pane_visual_lines(false, MOUSE_WHEEL_SCROLL_LINES);
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_active_pane_visual_lines(true, MOUSE_WHEEL_SCROLL_LINES);
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(diff_area) = self.last_diff_area_secondary
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.activate_pane(DiffPane::Secondary);
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll_secondary + view_row;
                    let content_col =
                        usize::from(mouse.column.saturating_sub(diff_area.x.saturating_add(1)));
                    if let Some((path, line)) = self.resolve_file_reference_hit(
                        DiffPane::Secondary,
                        visible_row_index,
                        content_col,
                    ) {
                        self.follow_file_reference(DiffPane::Secondary, &path, line);
                        return Ok(());
                    }
                    if let Some(row_index) = self
                        .last_diff_row_map_secondary
                        .get(visible_row_index)
                        .copied()
                    {
                        if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                            self.extend_comment_line_selection_to(DiffPane::Secondary, row_index);
                        } else {
                            self.set_active_line_index(row_index);
                        }
                        self.set_visual_row_for_pane(DiffPane::Secondary, Some(visible_row_index));
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.scroll_active_pane_visual_lines(false, MOUSE_WHEEL_SCROLL_LINES);
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_active_pane_visual_lines(true, MOUSE_WHEEL_SCROLL_LINES);
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        Ok(())
    }

    fn handle_inline_file_reference_picker_mouse(&mut self, mouse: MouseEvent) {
        if let Some(diff_area) = self.last_diff_area
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.activate_pane(DiffPane::Primary);
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll + view_row;
                    if let Some(row_index) = self.last_diff_row_map.get(visible_row_index).copied()
                    {
                        self.set_active_line_index(row_index);
                        self.set_visual_row_for_pane(DiffPane::Primary, Some(visible_row_index));
                        let _ = self.accept_inline_file_reference_line_selection();
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.scroll_active_pane_visual_lines(false, MOUSE_WHEEL_SCROLL_LINES);
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_active_pane_visual_lines(true, MOUSE_WHEEL_SCROLL_LINES);
                }
                _ => {}
            }
            return;
        }

        if let Some(diff_area) = self.last_diff_area_secondary
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.activate_pane(DiffPane::Secondary);
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll_secondary + view_row;
                    if let Some(row_index) = self
                        .last_diff_row_map_secondary
                        .get(visible_row_index)
                        .copied()
                    {
                        self.set_active_line_index(row_index);
                        self.set_visual_row_for_pane(DiffPane::Secondary, Some(visible_row_index));
                        let _ = self.accept_inline_file_reference_line_selection();
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.scroll_active_pane_visual_lines(false, MOUSE_WHEEL_SCROLL_LINES);
                }
                MouseEventKind::ScrollDown => {
                    self.scroll_active_pane_visual_lines(true, MOUSE_WHEEL_SCROLL_LINES);
                }
                _ => {}
            }
        }
    }

    async fn handle_code_search_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        let Some(area) = self.last_code_search_area else {
            return Ok(());
        };

        if !point_in_rect(mouse.column, mouse.row, area) {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.code_search = None;
                self.status_line = "code search closed".into();
            }
            return Ok(());
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let first_result_row = area.y.saturating_add(3);
                let visible_rows = usize_to_u16_saturating(self.last_code_search_visible_rows);
                let result_row_end = first_result_row.saturating_add(visible_rows);
                if mouse.row >= first_result_row && mouse.row < result_row_end {
                    let result_offset = usize::from(mouse.row.saturating_sub(first_result_row));
                    let result_index = self.last_code_search_scroll.saturating_add(result_offset);
                    if self
                        .code_search
                        .as_ref()
                        .is_some_and(|search| result_index < search.results.len())
                    {
                        if let Some(search) = self.code_search.as_mut() {
                            search.selected_index = result_index;
                        }
                        self.open_code_search_result_at_index(result_index).await?;
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(search) = self.code_search.as_mut() {
                    search.selected_index = search.selected_index.saturating_sub(1);
                    self.constrain_code_search_selection();
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(search) = self.code_search.as_mut() {
                    let max_index = search.results.len().saturating_sub(1);
                    search.selected_index = (search.selected_index + 1).min(max_index);
                    self.constrain_code_search_selection();
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::history::FileHeatmapEntry;
    use crate::tui::app::FileHeatmapSortMode;
    use crate::tui::app::FileHeatmapState;
    use crate::tui::app::state::tests::make_test_app;
    use anyhow::Result;
    use crossterm::event::KeyModifiers;
    use ratatui::layout::Rect;

    #[tokio::test]
    async fn clicking_visible_file_row_selects_scrolled_file() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs", "src/c.rs"], Vec::new())?;
        app.last_file_area = Some(Rect {
            x: 0,
            y: 0,
            width: 24,
            height: 4,
        });
        app.last_file_scroll = 1;
        app.last_file_row_map = vec![Some(0), Some(1), Some(2)];
        app.last_file_group_map = vec![None, None, None];

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::empty(),
        })
        .await?;

        assert_eq!(app.active_file_index(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn heatmap_mouse_wheel_scrolls_heatmap_not_background_diff() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new())?;
        app.file_heatmap = Some(FileHeatmapState {
            entries: vec![FileHeatmapEntry {
                path: "src/a.rs".to_string(),
                commits: 1,
                changes: 2,
                insertions: 1,
                deletions: 1,
            }],
            scroll: 0,
            sort_mode: FileHeatmapSortMode::Churn,
            sort_descending: true,
            loaded_at: None,
        });
        app.last_file_heatmap_area = Some(Rect {
            x: 10,
            y: 2,
            width: 60,
            height: 20,
        });
        app.last_diff_area = Some(Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        });
        app.last_diff_row_map = vec![0, 1, 2, 3, 4, 5];

        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 12,
            row: 4,
            modifiers: KeyModifiers::empty(),
        })
        .await?;

        assert_eq!(
            app.file_heatmap.as_ref().map(|heatmap| heatmap.scroll),
            Some(3)
        );
        assert_eq!(app.viewport_top_for_pane(DiffPane::Primary), 0);
        Ok(())
    }
}
