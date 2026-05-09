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
        let viewport_height = self.effective_viewport_height_for_pane(pane);
        let step = if half_page {
            (viewport_height / 2).max(1)
        } else {
            viewport_height.max(1)
        };

        let row_map: Vec<usize> = self.row_map_for_pane(pane).to_vec();
        let cursor_visual_row = cursor_visual_row_for_pane(self, pane, &row_map);
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
            self.set_visual_row_for_pane(pane, None);
            let cursor_source_row = self.line_for_pane(pane);
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
        self.set_visual_row_for_pane(pane, Some(next_visual));
    }

    pub(super) fn scroll_active_pane_visual_lines(&mut self, forward: bool, step: usize) {
        self.ensure_row_cache();
        let pane = self.active_diff_pane;
        let row_map: Vec<usize> = self.row_map_for_pane(pane).to_vec();
        if row_map.is_empty() {
            self.set_visual_row_for_pane(pane, None);
            let max_source = self.current_rows().len().saturating_sub(1);
            let cursor_source_row = self.line_for_pane(pane);
            let next_source = if forward {
                cursor_source_row.saturating_add(step).min(max_source)
            } else {
                cursor_source_row.saturating_sub(step)
            };
            self.set_line_for_pane(pane, next_source);
            return;
        }

        let step = step.max(1);
        let cursor_visual_row = cursor_visual_row_for_pane(self, pane, &row_map);
        let next_visual = if forward {
            cursor_visual_row
                .saturating_add(step)
                .min(row_map.len().saturating_sub(1))
        } else {
            cursor_visual_row.saturating_sub(step)
        };
        let viewport_height = self.effective_viewport_height_for_pane(pane);
        let mut next_top = self.viewport_top_for_pane(pane);
        if next_visual < next_top {
            next_top = next_visual;
        } else {
            let viewport_end = next_top.saturating_add(viewport_height);
            if next_visual >= viewport_end {
                next_top = next_visual
                    .saturating_add(1)
                    .saturating_sub(viewport_height);
            }
        }
        let max_top = row_map.len().saturating_sub(viewport_height);
        self.set_viewport_top_for_pane(pane, next_top.min(max_top));
        self.set_line_for_pane(pane, row_map[next_visual]);
        self.set_visual_row_for_pane(pane, Some(next_visual));
    }

    pub(super) fn center_active_cursor_in_viewport(&mut self) {
        let pane = self.active_diff_pane;
        let viewport_height = self.effective_viewport_height_for_pane(pane);
        let row_map: Vec<usize> = self.row_map_for_pane(pane).to_vec();
        let cursor_visual_row = cursor_visual_row_for_pane(self, pane, &row_map);
        let next_top = cursor_visual_row.saturating_sub(viewport_height / 2);
        self.set_viewport_top_for_pane(pane, next_top);
        self.status_line = "cursor centered in viewport".into();
    }
}

fn cursor_visual_row_for_pane(app: &TuiApp, pane: DiffPane, row_map: &[usize]) -> usize {
    let cursor_source_row = app.line_for_pane(pane);
    if let Some(visual_row) = app.visual_row_for_pane(pane)
        && row_map
            .get(visual_row)
            .is_some_and(|row| *row == cursor_source_row)
    {
        return visual_row;
    }

    row_map
        .iter()
        .position(|row| *row == cursor_source_row)
        .unwrap_or_else(|| cursor_source_row.min(row_map.len().saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::tests::make_test_app;
    use anyhow::Result;
    use ratatui::layout::Rect;

    #[test]
    fn visual_line_scroll_moves_inside_repeated_comment_source_row() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new())?;
        app.last_diff_area = Some(Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 6,
        });
        app.last_diff_row_map = vec![0, 1, 1, 1, 1, 1, 2];
        app.selected_line = 1;
        app.selected_visual_row = Some(3);
        app.primary_viewport_top_row = 1;

        app.scroll_active_pane_visual_lines(true, 2);

        assert_eq!(app.selected_line, 1);
        assert_eq!(app.visual_row_for_pane(DiffPane::Primary), Some(5));
        assert_eq!(app.viewport_top_for_pane(DiffPane::Primary), 2);
        Ok(())
    }
}
