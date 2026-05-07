//! Thread management state and operations.
//!
//! Handles comment thread selection, expansion, and status tracking.

use super::*;

impl TuiApp {
    pub(super) fn comments_for_selected_file(&self) -> Vec<&LineComment> {
        let Some(file) = self.current_file() else {
            return Vec::new();
        };
        self.comments_for_file(&file.path)
    }

    pub(super) fn selected_comment_details(&self) -> Option<&LineComment> {
        let comments = self.comments_for_selected_file();
        comments.get(self.selected_comment).copied()
    }

    pub(super) fn unresolved_thread_ids(&self) -> Vec<u64> {
        self.review
            .comments
            .iter()
            .filter(|comment| !matches!(comment.status, CommentStatus::Addressed))
            .map(|comment| comment.id)
            .collect()
    }

    pub(super) fn expanded_thread_ids_for_file(&self, file_path: &str) -> Vec<u64> {
        let mut ids = self
            .review
            .comments
            .iter()
            .filter(|comment| comment.file_path == file_path)
            .filter_map(|comment| {
                self.expanded_threads
                    .contains(&comment.id)
                    .then_some(comment.id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub(super) fn is_thread_expanded(
        &self,
        comment_id: u64,
        selected_comment_id: Option<u64>,
    ) -> bool {
        matches!(self.thread_density_mode, ThreadDensityMode::Expanded)
            || (!self.collapsed_threads.contains(&comment_id)
                && selected_comment_id == Some(comment_id))
            || self.expanded_threads.contains(&comment_id)
    }

    pub(super) fn toggle_selected_thread_expansion(&mut self) {
        let Some(comment) = self.selected_comment_details() else {
            self.status_line = "no thread selected".into();
            return;
        };
        let active_file_index = self.active_file_index();
        let comment_id = comment.id;
        let is_expanded = self.is_thread_expanded(comment_id, Some(comment_id));
        if is_expanded {
            self.expanded_threads.remove(&comment_id);
            self.collapsed_threads.insert(comment_id);
            self.status_line = format!("thread #{comment_id} collapsed");
        } else {
            self.collapsed_threads.remove(&comment_id);
            self.expanded_threads.insert(comment_id);
            self.status_line = format!("thread #{comment_id} expanded");
        }
        self.clear_diff_render_cache_for_file(active_file_index);
    }

    pub(super) fn thread_density_mode_label(&self) -> &'static str {
        match self.thread_density_mode {
            ThreadDensityMode::Compact => "compact",
            ThreadDensityMode::Expanded => "expanded",
        }
    }

    pub(super) fn cycle_thread_density_mode(&mut self) {
        self.thread_density_mode = match self.thread_density_mode {
            ThreadDensityMode::Compact => ThreadDensityMode::Expanded,
            ThreadDensityMode::Expanded => ThreadDensityMode::Compact,
        };
        self.clear_diff_render_cache();
        self.status_line = format!("thread density: {}", self.thread_density_mode_label());
    }
}
