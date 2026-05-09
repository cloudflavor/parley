//! Thread management state and operations.
//!
//! Handles comment thread selection, expansion, and status tracking.

use super::*;

use crate::utils::time::now_ms;
use anyhow::anyhow;

impl TuiApp {
    pub(crate) fn comments_for_selected_file(&self) -> Vec<&LineComment> {
        let Some(file) = self.current_file() else {
            return Vec::new();
        };
        self.comments_for_file(&file.path)
    }

    pub(crate) fn selected_comment_details(&self) -> Option<&LineComment> {
        let comments = self.comments_for_selected_file();
        comments.get(self.selected_comment).copied()
    }

    pub(crate) fn unresolved_thread_ids(&self) -> Vec<u64> {
        self.review
            .comments
            .iter()
            .filter(|comment| !matches!(comment.status, CommentStatus::Addressed))
            .map(|comment| comment.id)
            .collect()
    }

    pub(crate) fn expanded_thread_ids_for_file(&self, file_path: &str) -> Vec<u64> {
        let mut ids = self
            .comments_for_file(file_path)
            .into_iter()
            .filter_map(|comment| {
                self.expanded_threads
                    .contains(&comment.id)
                    .then_some(comment.id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn is_thread_expanded(
        &self,
        comment_id: u64,
        selected_comment_id: Option<u64>,
    ) -> bool {
        matches!(self.thread_density_mode, ThreadDensityMode::Expanded)
            || (!self.collapsed_threads.contains(&comment_id)
                && selected_comment_id == Some(comment_id))
            || self.expanded_threads.contains(&comment_id)
    }

    pub(crate) fn toggle_selected_thread_expansion(&mut self) {
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

    pub(crate) fn thread_density_mode_label(&self) -> &'static str {
        match self.thread_density_mode {
            ThreadDensityMode::Compact => "compact",
            ThreadDensityMode::Expanded => "expanded",
        }
    }

    pub(crate) fn cycle_thread_density_mode(&mut self) {
        self.thread_density_mode = match self.thread_density_mode {
            ThreadDensityMode::Compact => ThreadDensityMode::Expanded,
            ThreadDensityMode::Expanded => ThreadDensityMode::Compact,
        };
        self.clear_diff_render_cache();
        self.status_line = format!("thread density: {}", self.thread_density_mode_label());
    }

    pub(crate) async fn mark_selected_comment_status(
        &mut self,
        service: &ReviewService,
        status: CommentStatus,
        force: bool,
    ) -> Result<()> {
        let Some(comment) = self.selected_comment_details() else {
            return Ok(());
        };
        let comment_id = comment.id;
        if force {
            self.review
                .set_comment_status_force(comment_id, status.clone(), now_ms()?)
                .map_err(|error| anyhow!(error))?;
        } else {
            self.review
                .set_comment_status(comment_id, status.clone(), Author::User, now_ms()?)
                .map_err(|error| anyhow!(error))?;
        }
        service.save_review(&self.review).await?;
        self.rebuild_comment_index();
        self.clear_diff_render_cache();
        self.status_line = status_message(comment_id, &status, force);
        Ok(())
    }
}

fn status_message(comment_id: u64, status: &CommentStatus, force: bool) -> String {
    match (status, force) {
        (CommentStatus::Addressed, true) => format!("comment #{comment_id} force-addressed"),
        (CommentStatus::Addressed, false) => format!("comment #{comment_id} marked addressed"),
        (CommentStatus::Open, _) => format!("comment #{comment_id} marked open"),
        (CommentStatus::Pending, _) => format!("comment #{comment_id} marked pending"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::store::Store;
    use crate::services::review_service::ReviewService;
    use crate::tui::app::state::tests::{
        cache_entry, cache_key, make_comment_with_anchor, make_test_app,
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn mark_selected_comment_status_updates_review_without_reloading_diff() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![make_comment_with_anchor(
                1,
                "src/a.rs",
                CommentStatus::Pending,
                1,
                1,
            )],
        )?;
        service.save_review(&app.review).await?;
        app.ensure_row_cache();
        app.insert_diff_render_cache(cache_key(0), cache_entry());

        app.mark_selected_comment_status(&service, CommentStatus::Addressed, false)
            .await?;

        assert_eq!(
            app.selected_comment_details()
                .map(|comment| &comment.status),
            Some(&CommentStatus::Addressed)
        );
        assert!(app.row_cache.contains_key(&0));
        assert!(app.diff_render_cache.is_empty());
        let stats = app.comment_stats_for_file("src/a.rs");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.pending, 0);
        let saved = service.load_review(&app.review_name).await?;
        assert_eq!(saved.comments[0].status, CommentStatus::Addressed);
        Ok(())
    }
}
