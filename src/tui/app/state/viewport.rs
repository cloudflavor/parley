//! Viewport and rendering cache state.
//!
//! Handles scroll positions, row caches, and diff render caches.

use super::*;

impl TuiApp {
    pub(super) fn active_line_index(&self) -> usize {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            self.secondary_selected_line
        } else {
            self.selected_line
        }
    }

    pub(super) fn set_active_line_index(&mut self, index: usize) {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            if self.secondary_selected_line != index {
                self.pending_scroll_anchor_row_secondary = None;
            }
            self.secondary_selected_line = index;
        } else {
            if self.selected_line != index {
                self.pending_scroll_anchor_row = None;
            }
            self.selected_line = index;
        }
    }

    pub(super) fn set_line_for_pane(&mut self, pane: DiffPane, index: usize) {
        match pane {
            DiffPane::Primary => {
                if self.selected_line != index {
                    self.pending_scroll_anchor_row = None;
                }
                self.selected_line = index;
            }
            DiffPane::Secondary => {
                if self.secondary_selected_line != index {
                    self.pending_scroll_anchor_row_secondary = None;
                }
                self.secondary_selected_line = index;
            }
        }
    }

    pub(super) fn viewport_top_for_pane(&self, pane: DiffPane) -> usize {
        match pane {
            DiffPane::Primary => self.primary_viewport_top_row,
            DiffPane::Secondary => self.secondary_viewport_top_row,
        }
    }

    pub(super) fn set_viewport_top_for_pane(&mut self, pane: DiffPane, top_row: usize) {
        match pane {
            DiffPane::Primary => {
                self.primary_viewport_top_row = top_row;
            }
            DiffPane::Secondary => {
                self.secondary_viewport_top_row = top_row;
            }
        }
    }

    pub(super) fn take_pending_scroll_anchor(&mut self, pane: DiffPane) -> Option<usize> {
        match pane {
            DiffPane::Primary => self.pending_scroll_anchor_row.take(),
            DiffPane::Secondary => self.pending_scroll_anchor_row_secondary.take(),
        }
    }

    pub(super) fn row_map_for_pane(&self, pane: DiffPane) -> &[usize] {
        match pane {
            DiffPane::Primary => &self.last_diff_row_map,
            DiffPane::Secondary => &self.last_diff_row_map_secondary,
        }
    }

    pub(super) fn viewport_height_for_pane(&self, pane: DiffPane) -> usize {
        let area = match pane {
            DiffPane::Primary => self.last_diff_area,
            DiffPane::Secondary => self.last_diff_area_secondary,
        };
        area.map(|rect| usize::from(rect.height.saturating_sub(2)))
            .unwrap_or(1)
            .max(1)
    }

    pub(super) fn current_rows(&self) -> &[DisplayRow] {
        self.row_cache
            .get(&self.active_file_index())
            .map(|cached| cached.rows.as_slice())
            .unwrap_or(&[])
    }

    pub(super) fn line_anchor_snapshot_for_row(
        &self,
        row_index: usize,
    ) -> Option<LineAnchorSnapshot> {
        let rows = self.current_rows();
        let row = rows.get(row_index)?;
        if !anchor::is_commentable_row(row) {
            return None;
        }
        Some(anchor::build_line_anchor_snapshot(rows, row_index))
    }

    pub(super) fn rows_and_highlights_for_file(
        &self,
        file_index: usize,
    ) -> Option<(&[DisplayRow], &[HighlightParts])> {
        let cached = self.row_cache.get(&file_index)?;
        Some((&cached.rows, &cached.highlights))
    }

    pub(super) fn constrain_selection(&mut self) {
        let rows_len = self
            .row_cache
            .get(&self.active_file_index())
            .map(|cached| cached.rows.len())
            .unwrap_or(0);
        if rows_len == 0 {
            self.set_active_line_index(0);
        } else if self.active_line_index() >= rows_len {
            self.set_active_line_index(rows_len - 1);
        }

        let comments_len = self.comments_for_selected_file().len();
        if comments_len == 0 {
            self.selected_comment = 0;
        } else if self.selected_comment >= comments_len {
            self.selected_comment = comments_len - 1;
        }

        if self.selected_file >= self.diff.files.len() {
            self.selected_file = self.diff.files.len().saturating_sub(1);
        }
        if self.secondary_selected_file >= self.diff.files.len() {
            self.secondary_selected_file = self.diff.files.len().saturating_sub(1);
        }
        self.constrain_active_file_to_visible_list();

        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index >= rows_len
        {
            self.inline_comment = None;
        }
    }

    pub(super) fn ensure_row_cache(&mut self) {
        self.ensure_row_cache_for_file(self.active_file_index());
    }

    pub(super) fn ensure_row_cache_for_file(&mut self, file_index: usize) {
        if self.row_cache.contains_key(&file_index) {
            return;
        }
        self.rebuild_row_cache_for_file(file_index);
    }

    pub(super) fn rebuild_row_cache_for_file(&mut self, file_index: usize) {
        let Some(file) = self.diff.files.get(file_index) else {
            self.row_cache.remove(&file_index);
            self.clear_diff_render_cache_for_file(file_index);
            return;
        };

        let mut rows = Vec::new();
        for header in &file.header_lines {
            rows.push(DisplayRow {
                kind: DiffLineKind::Meta,
                old_line: None,
                new_line: None,
                raw: header.clone(),
                code: header.clone(),
            });
        }
        for hunk in &file.hunks {
            for line in &hunk.lines {
                rows.push(DisplayRow {
                    kind: line.kind.clone(),
                    old_line: line.old_line,
                    new_line: line.new_line,
                    raw: line.raw.clone(),
                    code: line.code.clone(),
                });
            }
        }

        let theme_colors = self.theme().colors.clone();
        let mut painter = SyntaxPainter::for_path(&file.path, &theme_colors);
        let mut highlights = Vec::with_capacity(rows.len());
        for row in &rows {
            let parts = match row.kind {
                DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context => {
                    painter.highlight(&row.code, &theme_colors)
                }
                _ => Vec::new(),
            };
            highlights.push(parts);
        }
        self.row_cache
            .insert(file_index, CachedFileRows { rows, highlights });
        self.clear_diff_render_cache_for_file(file_index);
    }

    pub(super) fn clear_diff_render_cache(&mut self) {
        self.diff_render_cache.clear();
        self.diff_render_cache_order.clear();
    }

    pub(super) fn clear_diff_render_cache_for_file(&mut self, file_index: usize) {
        self.diff_render_cache
            .retain(|key, _| key.file_index != file_index);
        self.diff_render_cache_order
            .retain(|key| key.file_index != file_index);
    }

    pub(super) fn get_diff_render_cache(
        &self,
        key: &DiffRenderCacheKey,
    ) -> Option<DiffRenderCacheEntry> {
        self.diff_render_cache.get(key).cloned()
    }

    pub(super) fn insert_diff_render_cache(
        &mut self,
        key: DiffRenderCacheKey,
        entry: DiffRenderCacheEntry,
    ) {
        if self.diff_render_cache.contains_key(&key) {
            self.diff_render_cache_order
                .retain(|existing| existing != &key);
        }
        self.diff_render_cache.insert(key.clone(), entry);
        self.diff_render_cache_order.push_back(key);

        while self.diff_render_cache_order.len() > DIFF_RENDER_CACHE_MAX_ENTRIES {
            if let Some(evicted) = self.diff_render_cache_order.pop_front() {
                self.diff_render_cache.remove(&evicted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_diff_render_cache_for_file_is_scoped() {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], vec![]);
        let key_a = cache_key(0);
        let key_b = cache_key(1);
        app.insert_diff_render_cache(key_a.clone(), cache_entry());
        app.insert_diff_render_cache(key_b.clone(), cache_entry());

        app.clear_diff_render_cache_for_file(0);

        assert!(!app.diff_render_cache.contains_key(&key_a));
        assert!(app.diff_render_cache.contains_key(&key_b));
    }
}
