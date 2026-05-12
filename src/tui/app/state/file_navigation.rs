//! File navigation state and operations.
//!
//! Handles file selection, filtering, sorting, and group management.

use super::*;
use crate::utils::cast::offset_index;
use std::collections::{HashMap, HashSet};

impl TuiApp {
    pub(crate) fn active_file_index(&self) -> usize {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            self.secondary_selected_file
        } else {
            self.selected_file
        }
    }

    pub(crate) fn set_active_file_index(&mut self, index: usize) {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            if self.secondary_selected_file != index {
                self.pending_scroll_anchor_row_secondary = None;
                self.secondary_viewport_top_row = 0;
                self.secondary_selected_visual_row = None;
                self.clear_comment_line_selection();
            }
            self.secondary_selected_file = index;
        } else {
            if self.selected_file != index {
                self.pending_scroll_anchor_row = None;
                self.primary_viewport_top_row = 0;
                self.selected_visual_row = None;
                self.clear_comment_line_selection();
            }
            self.selected_file = index;
        }
    }

    pub(crate) fn file_for_pane(&self, pane: DiffPane) -> Option<&DiffFile> {
        let idx = match pane {
            DiffPane::Primary => self.selected_file,
            DiffPane::Secondary => self.secondary_selected_file,
        };
        self.diff.files.get(idx)
    }

    pub(crate) fn select_file(&mut self, index: usize) {
        self.file_sidebar_manual_scroll = false;
        if self.diff.files.is_empty() {
            self.set_active_file_index(0);
            return;
        }

        let clamped = index.min(self.diff.files.len().saturating_sub(1));
        if clamped == self.active_file_index() {
            return;
        }

        self.set_active_file_index(clamped);
        self.start_root_file_hydration_if_needed(clamped);
        self.set_active_line_index(0);
        self.clear_comment_line_selection();
        self.selected_comment = 0;
        self.inline_comment = None;
    }

    pub(crate) fn move_file_selection(&mut self, delta: isize) {
        self.file_sidebar_manual_scroll = false;
        let ordered_files = self.ordered_file_selection_indices();
        if ordered_files.is_empty() {
            self.set_active_file_index(0);
            return;
        }

        let current_pos = ordered_files
            .iter()
            .position(|index| *index == self.active_file_index())
            .unwrap_or(0);
        let next_pos = offset_index(current_pos, ordered_files.len(), delta);
        self.select_file(ordered_files[next_pos]);
    }

    pub(crate) fn scroll_file_sidebar(&mut self, delta: isize) {
        if delta < 0 {
            self.last_file_scroll = self.last_file_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            self.last_file_scroll = self.last_file_scroll.saturating_add(delta as usize);
        }
        self.file_sidebar_manual_scroll = true;
    }

    fn ordered_file_selection_indices(&self) -> Vec<usize> {
        let rendered_rows = self
            .last_file_row_map
            .iter()
            .filter_map(|entry| *entry)
            .collect::<Vec<_>>();
        if !rendered_rows.is_empty() {
            return rendered_rows;
        }
        self.visible_file_indices()
    }

    pub(crate) fn current_file(&self) -> Option<&DiffFile> {
        self.diff.files.get(self.active_file_index())
    }

    pub(crate) fn build_comment_index(review: &ReviewSession) -> HashMap<String, Vec<usize>> {
        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for (comment_index, comment) in review.comments.iter().enumerate() {
            index
                .entry(comment.file_path.clone())
                .or_default()
                .push(comment_index);
        }
        index
    }

    pub(crate) fn build_comment_stats(review: &ReviewSession) -> HashMap<String, FileCommentStats> {
        let mut stats: HashMap<String, FileCommentStats> = HashMap::new();
        for comment in &review.comments {
            let entry = stats.entry(comment.file_path.clone()).or_default();
            entry.total += 1;
            if matches!(comment.status, CommentStatus::Open) {
                entry.open += 1;
            }
            if matches!(comment.status, CommentStatus::Pending) {
                entry.pending += 1;
            }
        }
        stats
    }

    pub(crate) fn rebuild_comment_index(&mut self) {
        self.comment_indices_by_file = Self::build_comment_index(&self.review);
        self.comment_stats_by_file = Self::build_comment_stats(&self.review);
    }

    pub(crate) fn comments_for_file(&self, file_path: &str) -> Vec<&LineComment> {
        self.comment_indices_by_file
            .get(file_path)
            .into_iter()
            .flat_map(|indices| indices.iter())
            .filter_map(|index| self.review.comments.get(*index))
            .collect()
    }

    pub(crate) fn comment_stats_for_file(&self, file_path: &str) -> FileCommentStats {
        self.comment_stats_by_file
            .get(file_path)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn selected_comment_id(&self) -> Option<u64> {
        self.selected_comment_details().map(|comment| comment.id)
    }

    pub(crate) fn select_comment_by_id(&mut self, comment_id: u64) -> bool {
        let Some(index) = self
            .comments_for_selected_file()
            .iter()
            .position(|comment| comment.id == comment_id)
        else {
            return false;
        };

        self.selected_comment = index;
        self.collapsed_threads.remove(&comment_id);
        self.expanded_threads.insert(comment_id);
        true
    }

    pub(crate) fn visible_file_indices(&self) -> Vec<usize> {
        let file_query = self.file_search_query().map(str::to_lowercase);
        let mut indices: Vec<usize> = self
            .diff
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| {
                let stats = self.comment_stats_for_file(&file.path);
                let visible = match self.file_filter_mode {
                    FileFilterMode::All => true,
                    FileFilterMode::Open => stats.open > 0,
                    FileFilterMode::Pending => stats.pending > 0,
                };
                if !visible {
                    return None;
                }
                if let Some(query) = file_query.as_ref() {
                    let path = file.path.to_lowercase();
                    if !path.contains(query) {
                        return None;
                    }
                }
                Some(idx)
            })
            .collect();

        indices.sort_by(|left, right| {
            let left_file = &self.diff.files[*left];
            let right_file = &self.diff.files[*right];
            let left_stats = self.comment_stats_for_file(&left_file.path);
            let right_stats = self.comment_stats_for_file(&right_file.path);
            match self.file_sort_mode {
                FileSortMode::Path => left_file.path.cmp(&right_file.path),
                FileSortMode::OpenCountDesc => right_stats
                    .open
                    .cmp(&left_stats.open)
                    .then_with(|| left_file.path.cmp(&right_file.path)),
                FileSortMode::TotalCountDesc => right_stats
                    .total
                    .cmp(&left_stats.total)
                    .then_with(|| left_file.path.cmp(&right_file.path)),
            }
        });
        indices
    }

    pub(crate) fn constrain_active_file_to_visible_list(&mut self) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            self.selected_file = self.diff.files.len().saturating_sub(1);
            if self.secondary_selected_file >= self.diff.files.len() {
                self.secondary_selected_file = self.diff.files.len().saturating_sub(1);
            }
            return;
        }

        if !visible.contains(&self.selected_file) {
            self.selected_file = visible[0];
            self.selected_line = 0;
            self.selected_comment = 0;
        }
        if !visible.contains(&self.secondary_selected_file) {
            self.secondary_selected_file = self.selected_file;
            self.secondary_selected_line = 0;
        }
    }

    pub(crate) fn cycle_file_filter_mode(&mut self) {
        let next = match self.file_filter_mode {
            FileFilterMode::All => FileFilterMode::Open,
            FileFilterMode::Open => FileFilterMode::Pending,
            FileFilterMode::Pending => FileFilterMode::All,
        };
        self.set_file_filter_mode(next);
    }

    pub(crate) fn set_file_filter_mode(&mut self, mode: FileFilterMode) {
        self.file_filter_mode = mode;
        self.constrain_active_file_to_visible_list();
        self.status_line = format!("file filter: {}", self.file_filter_mode_label());
    }

    pub(crate) fn cycle_file_sort_mode(&mut self) {
        let next = match self.file_sort_mode {
            FileSortMode::Path => FileSortMode::OpenCountDesc,
            FileSortMode::OpenCountDesc => FileSortMode::TotalCountDesc,
            FileSortMode::TotalCountDesc => FileSortMode::Path,
        };
        self.set_file_sort_mode(next);
    }

    pub(crate) fn set_file_sort_mode(&mut self, mode: FileSortMode) {
        self.file_sort_mode = mode;
        self.constrain_active_file_to_visible_list();
        self.status_line = format!("file sort: {}", self.file_sort_mode_label());
    }

    pub(crate) fn file_filter_mode_label(&self) -> &'static str {
        match self.file_filter_mode {
            FileFilterMode::All => "all",
            FileFilterMode::Open => "open",
            FileFilterMode::Pending => "pending",
        }
    }

    pub(crate) fn file_sort_mode_label(&self) -> &'static str {
        match self.file_sort_mode {
            FileSortMode::Path => "path",
            FileSortMode::OpenCountDesc => "open_count",
            FileSortMode::TotalCountDesc => "total_count",
        }
    }

    pub(crate) fn file_group_name_for_index(&self, file_index: usize) -> String {
        let Some(file) = self.diff.files.get(file_index) else {
            return ".".to_string();
        };
        let path = file.path.as_str();
        path.rsplit_once('/').map_or_else(
            || ".".to_string(),
            |(group, _)| {
                if group.is_empty() {
                    ".".to_string()
                } else {
                    group.to_string()
                }
            },
        )
    }

    pub(crate) fn file_search_query(&self) -> Option<&str> {
        let trimmed = self.file_search.query.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub(crate) fn toggle_file_group_collapsed(&mut self, group: &str) {
        if self.collapsed_file_groups.contains(group) {
            self.collapsed_file_groups.remove(group);
            self.status_line = format!("expanded group: {group}");
        } else {
            self.collapsed_file_groups.insert(group.to_string());
            self.status_line = format!("collapsed group: {group}");
            self.constrain_active_file_to_visible_list();
        }
    }

    pub(crate) fn toggle_active_file_group_collapsed(&mut self) {
        let group = self.file_group_name_for_index(self.active_file_index());
        self.toggle_file_group_collapsed(&group);
    }

    pub(crate) fn collapse_all_visible_file_groups(&mut self) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            self.status_line = "no file groups to collapse".into();
            return;
        }
        let mut groups: HashSet<String> = HashSet::new();
        for file_index in visible {
            groups.insert(self.file_group_name_for_index(file_index));
        }
        let before = self.collapsed_file_groups.len();
        self.collapsed_file_groups.extend(groups);
        let added = self.collapsed_file_groups.len().saturating_sub(before);
        self.constrain_active_file_to_visible_list();
        self.status_line = if added == 0 {
            "all visible groups already collapsed".into()
        } else {
            format!("collapsed {added} file group(s)")
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::review::CommentStatus;
    use crate::tui::app::state::tests::{make_comment_with_anchor, make_test_app};
    use anyhow::Result;

    #[test]
    fn visible_file_indices_respects_filter_sort_and_search_query() -> Result<()> {
        let comments = vec![
            make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
            make_comment_with_anchor(2, "src/b.rs", CommentStatus::Pending, 2, 2),
            make_comment_with_anchor(3, "src/c.rs", CommentStatus::Addressed, 3, 3),
        ];
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs", "src/c.rs"], comments)?;

        let visible = app.visible_file_indices();
        assert_eq!(visible.len(), 3);

        app.set_file_filter_mode(FileFilterMode::Open);
        let visible_open = app.visible_file_indices();
        assert_eq!(visible_open.len(), 1);

        app.set_file_filter_mode(FileFilterMode::Pending);
        let visible_pending = app.visible_file_indices();
        assert_eq!(visible_pending.len(), 1);
        Ok(())
    }

    #[test]
    fn file_filter_constrains_selection_to_visible_files() -> Result<()> {
        let comments = vec![
            make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
            make_comment_with_anchor(2, "src/b.rs", CommentStatus::Pending, 2, 2),
        ];
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], comments)?;
        app.select_file(1);

        app.set_file_filter_mode(FileFilterMode::Open);
        assert_eq!(app.selected_file, 0);
        Ok(())
    }

    #[test]
    fn collapse_all_visible_file_groups_only_collapses_current_filter_scope() -> Result<()> {
        let comments = vec![
            make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
            make_comment_with_anchor(2, "src/b.rs", CommentStatus::Pending, 2, 2),
        ];
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], comments)?;

        app.collapse_all_visible_file_groups();
        assert_eq!(app.collapsed_file_groups.len(), 1);
        Ok(())
    }

    #[test]
    fn move_file_selection_follows_rendered_sidebar_order() -> Result<()> {
        let comments = vec![
            make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
            make_comment_with_anchor(2, "src/b.rs", CommentStatus::Open, 2, 2),
            make_comment_with_anchor(3, "src/c.rs", CommentStatus::Open, 3, 3),
        ];
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs", "src/c.rs"], comments)?;

        app.move_file_selection(1);
        assert_eq!(app.active_file_index(), 1);

        app.move_file_selection(1);
        assert_eq!(app.active_file_index(), 2);

        app.move_file_selection(1);
        assert_eq!(app.active_file_index(), 2);
        Ok(())
    }

    #[test]
    fn comments_for_file_uses_rebuilt_comment_index() -> Result<()> {
        let comments = vec![
            make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
            make_comment_with_anchor(2, "src/b.rs", CommentStatus::Pending, 2, 2),
            make_comment_with_anchor(3, "src/a.rs", CommentStatus::Addressed, 3, 3),
        ];
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], comments)?;

        let initial_ids = app
            .comments_for_file("src/a.rs")
            .into_iter()
            .map(|comment| comment.id)
            .collect::<Vec<_>>();
        assert_eq!(initial_ids, vec![1, 3]);

        app.review.comments = vec![make_comment_with_anchor(
            4,
            "src/b.rs",
            CommentStatus::Open,
            4,
            4,
        )];
        app.rebuild_comment_index();

        assert!(app.comments_for_file("src/a.rs").is_empty());
        assert_eq!(app.comments_for_file("src/b.rs")[0].id, 4);
        let stats = app.comment_stats_for_file("src/b.rs");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.open, 1);
        Ok(())
    }
}
