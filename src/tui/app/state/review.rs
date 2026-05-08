//! Review state operations.
//!
//! Handles review state transitions, reloading, and persistence.

use super::*;

impl TuiApp {
    pub(crate) fn review_state_code(&self) -> u8 {
        match self.review.state {
            ReviewState::Open => 0,
            ReviewState::UnderReview => 1,
            ReviewState::Done => 2,
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
        let longest_path = self
            .diff
            .files
            .iter()
            .map(|file| file.path.chars().count())
            .max()
            .unwrap_or(16) as i16;
        let base = longest_path + 8;
        let min_width = 16i16;
        let max_width = (total_width as i16 - 30).clamp(min_width, 90);
        let computed = (base + self.file_pane_width_delta).clamp(min_width, max_width);
        computed as u16
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
        let selected_comment = self.selected_comment;
        self.review = service.load_review(&self.review_name).await?;
        self.expanded_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.collapsed_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.clear_diff_render_cache();
        self.selected_line = selected_line;
        self.secondary_selected_line = secondary_selected_line;
        self.selected_comment = selected_comment;
        self.constrain_selection();
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
        let selected_comment = self.selected_comment;

        self.review = service.load_review(&self.review_name).await?;
        self.expanded_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.collapsed_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.diff = load_git_diff(&self.config, &self.diff_source).await?;
        self.row_cache.clear();
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
        self.selected_comment = selected_comment;
        self.ensure_row_cache_for_file(self.selected_file);
        if self.split_diff_view {
            self.ensure_row_cache_for_file(self.secondary_selected_file);
        }
        self.constrain_selection();
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
