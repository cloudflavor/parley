//! Review state operations.
//!
//! Handles review state transitions, reloading, and persistence.

use super::*;
use crate::utils::cast::{i16_to_u16_saturating, u16_to_i16_saturating};

impl TuiApp {
    pub(crate) async fn poll_root_directory_diff_load(
        &mut self,
        service: &ReviewService,
    ) -> Result<bool> {
        let Some(task) = self.root_diff_load_task.as_ref() else {
            return Ok(false);
        };
        if !task.is_finished() {
            return Ok(false);
        }

        let task = self
            .root_diff_load_task
            .take()
            .context("root directory diff load task missing")?;
        self.root_diff_load_started_at = None;
        let result = task
            .await
            .context("failed to join root directory diff load task")?
            .context("root directory diff load failed")?;

        let previous_primary_path = self
            .file_for_pane(DiffPane::Primary)
            .map(|f| f.path.clone());
        let previous_secondary_path = self
            .file_for_pane(DiffPane::Secondary)
            .map(|f| f.path.clone());
        let selected_line = self.selected_line;
        let secondary_selected_line = self.secondary_selected_line;
        let selected_comment_id = self.selected_comment_id();

        self.diff = result;
        self.row_cache.clear();
        self.root_hydrated_files.clear();
        self.clear_diff_render_cache();
        if self.remap_comment_anchors() {
            service.save_review(&self.review).await?;
        }

        self.selected_file = previous_primary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(0);
        self.secondary_selected_file = previous_secondary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(self.selected_file);

        self.selected_line = selected_line;
        self.secondary_selected_line = secondary_selected_line;
        self.ensure_row_cache_for_file(self.selected_file);
        self.start_root_file_hydration_if_needed(self.selected_file);
        if self.split_diff_view {
            self.ensure_row_cache_for_file(self.secondary_selected_file);
            self.start_root_file_hydration_if_needed(self.secondary_selected_file);
        }
        self.constrain_selection();
        if let Some(comment_id) = selected_comment_id {
            self.select_comment_by_id(comment_id);
        }
        self.status_line = if self.diff.files.is_empty() {
            "root directory loaded; no reviewable files found".to_string()
        } else {
            format!("loaded {} root files", self.diff.files.len())
        };

        Ok(true)
    }

    pub(crate) async fn poll_root_directory_file_load(&mut self) -> Result<bool> {
        let Some(task) = self.root_file_load_task.as_ref() else {
            self.start_root_file_hydration_if_needed(self.active_file_index());
            return Ok(false);
        };
        if !task.is_finished() {
            return Ok(false);
        }

        let task = self
            .root_file_load_task
            .take()
            .context("root file load task missing")?;
        let (file_index, loaded_file) =
            task.await.context("failed to join root file load task")??;
        self.root_hydrated_files.insert(file_index);
        if let Some(file) = loaded_file
            && let Some(slot) = self.diff.files.get_mut(file_index)
        {
            *slot = file;
            self.row_cache.remove(&file_index);
            self.clear_diff_render_cache_for_file(file_index);
        }
        self.start_root_file_hydration_if_needed(self.active_file_index());
        Ok(true)
    }

    pub(crate) fn start_root_file_hydration_if_needed(&mut self, file_index: usize) {
        if !matches!(self.diff_source, DiffSource::RootDirectory)
            || self.root_file_load_task.is_some()
            || self.root_hydrated_files.contains(&file_index)
        {
            return;
        }
        let Some(file) = self.diff.files.get(file_index) else {
            return;
        };
        if !file.hunks.is_empty() {
            self.root_hydrated_files.insert(file_index);
            return;
        }
        let config = self.config.clone();
        let path = file.path.clone();
        self.root_file_load_task = Some(task::spawn(async move {
            load_root_directory_file(&config, path)
                .await
                .map(|file| (file_index, file))
        }));
    }

    pub(crate) fn toggle_root_document_rendering(&mut self) {
        if !matches!(self.diff_source, DiffSource::RootDirectory) {
            self.status_line = "document rendering is only available in root mode".into();
            return;
        }
        self.root_document_rendering = !self.root_document_rendering;
        self.row_cache.clear();
        self.clear_diff_render_cache();
        self.status_line = if self.root_document_rendering {
            "root JSON/Markdown rendering enabled".into()
        } else {
            "root JSON/Markdown rendering disabled".into()
        };
    }

    pub(crate) fn review_state_code(&self) -> u8 {
        match self.review.state {
            ReviewState::Open => 0,
            ReviewState::UnderReview => 1,
        }
    }

    pub(crate) fn activate_pane(&mut self, pane: DiffPane) {
        if self.active_diff_pane == pane {
            return;
        }
        self.active_diff_pane = pane;
        self.inline_comment = None;
    }

    pub(crate) fn toggle_split_diff_view(&mut self) {
        self.split_diff_view = !self.split_diff_view;
        if !self.split_diff_view {
            self.active_diff_pane = DiffPane::Primary;
            self.inline_comment = None;
        }
    }

    pub(crate) fn resize_file_pane(&mut self, delta_cols: i16) {
        self.file_pane_width_delta = (self.file_pane_width_delta + delta_cols).clamp(-40, 80);
    }

    pub(crate) fn computed_file_pane_width(&self, total_width: u16) -> u16 {
        let min_width = 16i16;
        let base = 28i16;
        let max_by_screen = u16_to_i16_saturating(total_width.saturating_mul(2) / 5);
        let max_by_content = u16_to_i16_saturating(total_width).saturating_sub(30);
        let max_width = max_by_screen.min(max_by_content).clamp(min_width, 56);
        let computed = (base + self.file_pane_width_delta).clamp(min_width, max_width);
        i16_to_u16_saturating(computed)
    }

    pub(crate) fn line_for_pane(&self, pane: DiffPane) -> usize {
        match pane {
            DiffPane::Primary => self.selected_line,
            DiffPane::Secondary => self.secondary_selected_line,
        }
    }

    pub(crate) async fn set_state(
        &mut self,
        service: &ReviewService,
        next: ReviewState,
    ) -> Result<()> {
        service
            .set_state(&self.review_name, next.clone())
            .await
            .with_context(|| format!("failed to set state to {next:?}"))?;
        self.reload_review(service).await?;
        self.status_line = format!("review state set to {next:?}");
        Ok(())
    }

    pub(crate) async fn reload_review(&mut self, service: &ReviewService) -> Result<()> {
        let selected_line = self.selected_line;
        let secondary_selected_line = self.secondary_selected_line;
        let selected_comment_id = self.selected_comment_id();
        self.review = service.load_review(&self.review_name).await?;
        self.rebuild_comment_index();
        self.expanded_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.collapsed_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.clear_diff_render_cache();
        self.selected_line = selected_line;
        self.secondary_selected_line = secondary_selected_line;
        self.constrain_selection();
        if let Some(comment_id) = selected_comment_id {
            self.select_comment_by_id(comment_id);
        }
        Ok(())
    }

    pub(crate) async fn refresh_review_and_diff(&mut self, service: &ReviewService) -> Result<()> {
        let previous_primary_path = self
            .file_for_pane(DiffPane::Primary)
            .map(|f| f.path.clone());
        let previous_secondary_path = self
            .file_for_pane(DiffPane::Secondary)
            .map(|f| f.path.clone());
        let selected_line = self.selected_line;
        let secondary_selected_line = self.secondary_selected_line;
        let selected_comment_id = self.selected_comment_id();

        self.review = service.load_review(&self.review_name).await?;
        self.rebuild_comment_index();
        self.expanded_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.collapsed_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.diff = load_git_diff(&self.config, &self.diff_source).await?;
        self.row_cache.clear();
        self.root_hydrated_files.clear();
        self.clear_diff_render_cache();
        if self.remap_comment_anchors() {
            service.save_review(&self.review).await?;
        }

        self.selected_file = previous_primary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(0);
        self.secondary_selected_file = previous_secondary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(self.selected_file);

        self.selected_line = selected_line;
        self.secondary_selected_line = secondary_selected_line;
        self.ensure_row_cache_for_file(self.selected_file);
        if self.split_diff_view {
            self.ensure_row_cache_for_file(self.secondary_selected_file);
        }
        self.constrain_selection();
        if let Some(comment_id) = selected_comment_id {
            self.select_comment_by_id(comment_id);
        }
        Ok(())
    }

    fn remap_comment_anchors(&mut self) -> bool {
        let mut changed = false;
        let remap_timestamp = anchor::now_ms_utc();

        for index in 0..self.review.comments.len() {
            let snapshot = self.review.comments[index].clone();
            let resolved = self.resolve_comment_anchor(&snapshot);
            let comment = &mut self.review.comments[index];

            match resolved {
                Some(target) => {
                    let needs_update = comment.side != target.side
                        || comment.old_line != target.old_line
                        || comment.new_line != target.new_line
                        || comment.detached
                        || comment.line_anchor.as_ref() != Some(&target.line_anchor);
                    if needs_update {
                        comment.side = target.side;
                        comment.old_line = target.old_line;
                        comment.new_line = target.new_line;
                        comment.detached = false;
                        comment.line_anchor = Some(target.line_anchor);
                        comment.updated_at_ms = remap_timestamp;
                        changed = true;
                    }
                }
                None => {
                    if !comment.detached {
                        comment.detached = true;
                        comment.updated_at_ms = remap_timestamp;
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.review.updated_at_ms = remap_timestamp;
        }
        changed
    }

    fn resolve_comment_anchor(
        &mut self,
        comment: &LineComment,
    ) -> Option<anchor::ResolvedLineAnchor> {
        let file_index = self
            .diff
            .files
            .iter()
            .position(|file| file.path == comment.file_path)?;
        self.ensure_row_cache_for_file(file_index);
        let rows = self.row_cache.get(&file_index)?.rows.as_slice();

        if let Some((row_index, _)) = rows.iter().enumerate().find(|(_, row)| {
            anchor::is_commentable_row(row) && anchor::row_matches_exact_anchor(comment, row)
        }) {
            return Some(anchor::ResolvedLineAnchor::from_row(rows, row_index));
        }

        let snapshot = comment.line_anchor.as_ref()?;
        if snapshot.target_code.trim().is_empty() {
            return None;
        }

        let mut best_match: Option<(i32, usize)> = None;
        for (row_index, row) in rows.iter().enumerate() {
            if !anchor::is_commentable_row(row) {
                continue;
            }
            let score = anchor::score_anchor_candidate(
                comment.side.clone(),
                snapshot,
                rows,
                row_index,
                row,
            );
            if let Some((best_score, _)) = best_match
                && score <= best_score
            {
                continue;
            }
            best_match = Some((score, row_index));
        }

        let (score, row_index) = best_match?;
        (score >= 90).then(|| anchor::ResolvedLineAnchor::from_row(rows, row_index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::store::Store;
    use crate::services::review_service::ReviewService;
    use crate::tui::app::state::tests::{make_comment_with_anchor, make_test_app};
    use tempfile::tempdir;

    #[tokio::test]
    async fn reload_review_preserves_selected_thread_by_id_when_order_changes() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let mut app = make_test_app(
            vec!["src/a.rs"],
            vec![
                make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
                make_comment_with_anchor(2, "src/a.rs", CommentStatus::Pending, 2, 2),
            ],
        )?;
        app.selected_comment = 1;

        let mut stored = app.review.clone();
        stored.comments = vec![
            make_comment_with_anchor(2, "src/a.rs", CommentStatus::Pending, 2, 2),
            make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 1, 1),
        ];
        service.save_review(&stored).await?;

        app.reload_review(&service).await?;

        assert_eq!(
            app.selected_comment_details().map(|comment| comment.id),
            Some(2)
        );
        Ok(())
    }

    #[test]
    fn computed_file_pane_width_stays_compact_for_long_paths() -> Result<()> {
        let app = make_test_app(
            vec!["src/a/really/deep/path/with/a/very/long/file/name.rs"],
            Vec::new(),
        )?;

        assert_eq!(app.computed_file_pane_width(120), 28);
        Ok(())
    }
}
