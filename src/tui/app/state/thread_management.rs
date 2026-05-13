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

    pub(crate) fn open_thread_selector(&mut self) {
        self.dismiss_blocking_overlays();
        self.thread_selector = Some(ThreadSelectorState {
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        self.status_line = "thread selector opened".into();
    }

    pub(crate) fn filtered_thread_selector_entries(&self) -> Vec<ThreadSelectorEntry> {
        let Some(selector) = self.thread_selector.as_ref() else {
            return Vec::new();
        };
        let query = selector.query.trim().to_lowercase();
        let mut entries = self
            .review
            .comments
            .iter()
            .map(|comment| ThreadSelectorEntry {
                comment_id: comment.id,
                file_path: comment.file_path.clone(),
                status: comment.status.clone(),
                line_reference: format_comment_reference(comment),
                preview: comment
                    .body
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or("(empty)")
                    .to_string(),
            })
            .filter(|entry| {
                if query.is_empty() {
                    return true;
                }
                entry.file_path.to_lowercase().contains(&query)
                    || entry.preview.to_lowercase().contains(&query)
                    || entry.line_reference.to_lowercase().contains(&query)
                    || entry.comment_id.to_string().contains(&query)
                    || format!("{:?}", entry.status)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.file_path
                .cmp(&right.file_path)
                .then_with(|| left.line_reference.cmp(&right.line_reference))
                .then_with(|| left.comment_id.cmp(&right.comment_id))
        });
        entries
    }

    pub(crate) fn jump_to_thread_selector_entry(&mut self, entry: &ThreadSelectorEntry) {
        let Some(file_index) = self
            .diff
            .files
            .iter()
            .position(|file| file.path == entry.file_path)
        else {
            self.status_line = format!("thread file not visible: {}", entry.file_path);
            return;
        };

        self.select_file(file_index);
        if !self.select_comment_by_id(entry.comment_id) {
            self.status_line = format!("thread #{} not visible in file", entry.comment_id);
            return;
        }
        self.focus_selected_comment_line();
        self.request_scroll_to_thread_tail(self.active_diff_pane, self.active_line_index());
        self.thread_selector = None;
        self.status_line = format!(
            "selected thread #{} at {}",
            entry.comment_id, entry.file_path
        );
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

    pub(crate) fn collapsed_thread_ids_for_file(&self, file_path: &str) -> Vec<u64> {
        let mut ids = self
            .comments_for_file(file_path)
            .into_iter()
            .filter_map(|comment| {
                self.collapsed_threads
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
        !self.collapsed_threads.contains(&comment_id)
            && (selected_comment_id == Some(comment_id)
                || self.expanded_threads.contains(&comment_id))
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

    pub(crate) fn toggle_selected_thread_anchor_expansion(&mut self) {
        let Some(comment) = self.selected_comment_details() else {
            self.status_line = "no thread selected".into();
            return;
        };
        let comment_id = comment.id;
        let active_file_index = self.active_file_index();
        if self.expanded_anchor_threads.contains(&comment_id) {
            self.expanded_anchor_threads.remove(&comment_id);
            self.status_line = format!("thread #{comment_id} anchor collapsed");
        } else {
            self.expanded_anchor_threads.insert(comment_id);
            self.status_line = format!("thread #{comment_id} anchor expanded");
        }
        self.clear_diff_render_cache_for_file(active_file_index);
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
        let active_file_index = self.active_file_index();
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
        self.select_comment_by_id(comment_id);
        self.clear_diff_render_cache_for_file(active_file_index);
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
        cache_entry, cache_key, diff_file_with_context_lines, make_comment_with_anchor,
        make_test_app, make_test_app_with_files_and_comments,
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn mark_selected_comment_status_updates_review_without_reloading_diff() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let mut app = make_test_app(
            vec!["src/a.rs", "src/b.rs"],
            vec![
                make_comment_with_anchor(1, "src/a.rs", CommentStatus::Pending, 1, 1),
                make_comment_with_anchor(2, "src/b.rs", CommentStatus::Pending, 1, 1),
            ],
        )?;
        service.save_review(&app.review).await?;
        app.ensure_row_cache();
        app.insert_diff_render_cache(cache_key(0), cache_entry());
        app.insert_diff_render_cache(cache_key(1), cache_entry());

        app.mark_selected_comment_status(&service, CommentStatus::Addressed, false)
            .await?;

        assert_eq!(
            app.selected_comment_details()
                .map(|comment| &comment.status),
            Some(&CommentStatus::Addressed)
        );
        assert!(app.expanded_threads.contains(&1));
        assert!(app.row_cache.contains_key(&0));
        assert!(!app.diff_render_cache.contains_key(&cache_key(0)));
        assert!(app.diff_render_cache.contains_key(&cache_key(1)));
        let stats = app.comment_stats_for_file("src/a.rs");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.pending, 0);
        let saved = service.load_review(&app.review_name).await?;
        assert_eq!(saved.comments[0].status, CommentStatus::Addressed);
        Ok(())
    }

    #[tokio::test]
    async fn mark_status_can_address_multiple_threads_in_same_file() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![
                make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
                make_comment_with_anchor(2, "src/a.rs", CommentStatus::Open, 2, 2),
            ],
        )?;
        service.save_review(&app.review).await?;

        app.selected_comment = 0;
        app.mark_selected_comment_status(&service, CommentStatus::Addressed, false)
            .await?;
        app.selected_comment = 1;
        app.mark_selected_comment_status(&service, CommentStatus::Addressed, false)
            .await?;

        let statuses = app
            .comments_for_file("src/a.rs")
            .iter()
            .map(|comment| comment.status.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            statuses,
            vec![CommentStatus::Addressed, CommentStatus::Addressed]
        );
        assert!(app.expanded_threads.contains(&1));
        assert!(app.expanded_threads.contains(&2));
        Ok(())
    }

    #[test]
    fn collapsed_thread_remains_collapsed() -> Result<()> {
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![
                make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
                make_comment_with_anchor(2, "src/a.rs", CommentStatus::Open, 2, 2),
            ],
        )?;
        app.collapsed_threads.insert(1);

        assert!(!app.is_thread_expanded(1, Some(1)));
        assert!(!app.is_thread_expanded(2, None));
        Ok(())
    }

    #[test]
    fn toggle_selected_thread_collapses_only_selected_thread() -> Result<()> {
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![
                make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
                make_comment_with_anchor(2, "src/a.rs", CommentStatus::Open, 2, 2),
            ],
        )?;
        app.selected_comment = 0;

        app.toggle_selected_thread_expansion();

        assert!(app.collapsed_threads.contains(&1));
        assert!(!app.collapsed_threads.contains(&2));
        assert!(!app.is_thread_expanded(1, Some(1)));
        // Thread 2 is not selected and not explicitly expanded, so it stays collapsed
        assert!(!app.is_thread_expanded(2, None));

        app.toggle_selected_thread_expansion();

        assert!(!app.collapsed_threads.contains(&1));
        assert!(app.expanded_threads.contains(&1));
        assert!(app.is_thread_expanded(1, Some(1)));
        Ok(())
    }

    #[test]
    fn root_mode_focuses_detached_thread_by_stored_line_reference() -> Result<()> {
        let mut comment = make_comment_with_anchor(1, "src/a.rs", CommentStatus::Pending, 7, 7);
        comment.detached = true;
        let mut app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(6, "before"), (7, "refactored"), (8, "after")],
            )],
            vec![comment],
        )?;
        app.diff_source = DiffSource::RootDirectory;
        app.selected_comment = 0;

        app.focus_selected_comment_line();

        assert_eq!(
            app.row_for_file(app.active_file_index(), app.selected_line)
                .and_then(|row| row.new_line),
            Some(7)
        );
        Ok(())
    }

    #[test]
    fn thread_selector_filters_and_jumps_to_thread_file() -> Result<()> {
        let app = make_test_app(
            vec!["src/a.rs", "src/b.rs"],
            vec![
                make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
                make_comment_with_anchor(2, "src/b.rs", CommentStatus::Pending, 1, 1),
            ],
        )?;
        let mut app = app;
        app.open_thread_selector();
        if let Some(selector) = app.thread_selector.as_mut() {
            selector.query = "src/b".to_string();
            selector.cursor_col = selector.query.chars().count();
        }
        let entries = app.filtered_thread_selector_entries();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].comment_id, 2);

        app.jump_to_thread_selector_entry(&entries[0]);

        assert_eq!(app.active_file_index(), 1);
        assert_eq!(
            app.selected_comment_details().map(|comment| comment.id),
            Some(2)
        );
        assert!(app.thread_selector.is_none());
        Ok(())
    }
}
