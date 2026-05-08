use super::*;

impl TuiApp {
    pub(super) fn toggle_content_fullscreen(&mut self) {
        self.content_fullscreen = !self.content_fullscreen;
        if self.content_fullscreen {
            self.status_line = "content fullscreen enabled".into();
        } else {
            self.status_line = "content fullscreen disabled".into();
        }
    }

    pub(super) fn scroll_active_pane_page(&mut self, forward: bool, half_page: bool) {
        self.ensure_row_cache();
        let pane = self.active_diff_pane;
        let viewport_height = self.viewport_height_for_pane(pane);
        let step = if half_page {
            (viewport_height / 2).max(1)
        } else {
            viewport_height.max(1)
        };

        let row_map: Vec<usize> = self.row_map_for_pane(pane).to_vec();
        let cursor_source_row = self.line_for_pane(pane);
        let cursor_visual_row = row_map
            .iter()
            .position(|row| *row == cursor_source_row)
            .unwrap_or_else(|| cursor_source_row.min(row_map.len().saturating_sub(1)));
        let previous_top = self.viewport_top_for_pane(pane);
        let cursor_offset = cursor_visual_row.saturating_sub(previous_top);

        let mut next_top = if forward {
            previous_top.saturating_add(step)
        } else {
            previous_top.saturating_sub(step)
        };
        if !row_map.is_empty() {
            let max_top = row_map.len().saturating_sub(viewport_height);
            next_top = next_top.min(max_top);
        }
        self.set_viewport_top_for_pane(pane, next_top);

        if row_map.is_empty() {
            let max_source = self.current_rows().len().saturating_sub(1);
            let next_source = if forward {
                cursor_source_row.saturating_add(step).min(max_source)
            } else {
                cursor_source_row.saturating_sub(step)
            };
            self.set_line_for_pane(pane, next_source);
            return;
        }

        let next_visual = (next_top + cursor_offset).min(row_map.len().saturating_sub(1));
        self.set_line_for_pane(pane, row_map[next_visual]);
    }

    pub(super) fn center_active_cursor_in_viewport(&mut self) {
        let pane = self.active_diff_pane;
        let viewport_height = self.viewport_height_for_pane(pane);
        let cursor_source_row = self.line_for_pane(pane);
        let cursor_visual_row = self
            .row_map_for_pane(pane)
            .iter()
            .position(|row| *row == cursor_source_row)
            .unwrap_or(cursor_source_row);
        let next_top = cursor_visual_row.saturating_sub(viewport_height / 2);
        self.set_viewport_top_for_pane(pane, next_top);
        self.status_line = "cursor centered in viewport".into();
    }
}
