//! Review state operations.
//!
//! Handles review state transitions, reloading, and persistence.

use super::*;
use crate::domain::review::ReanchorLineComment;

impl TuiApp {
    pub(super) fn review_state_code(&self) -> u8 {
        match self.review.state {
            ReviewState::Open => 0,
            ReviewState::UnderReview => 1,
            ReviewState::Done => 2,
        }
    }

    pub(super) fn activate_pane(&mut self, pane: DiffPane) {
        if self.active_diff_pane == pane {
            return;
        }
        self.active_diff_pane = pane;
        self.inline_comment = None;
    }

    pub(super) fn toggle_split_diff_view(&mut self) {
        self.split_diff_view = !self.split_diff_view;
        if !self.split_diff_view {
            self.active_diff_pane = DiffPane::Primary;
            self.inline_comment = None;
        }
    }

    pub(super) fn resize_file_pane(&mut self, delta_cols: i16) {
        self.file_pane_width_delta = (self.file_pane_width_delta + delta_cols).clamp(-40, 80);
    }

    pub(super) fn computed_file_pane_width(&self, total_width: u16) -> u16 {
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

    pub(super) fn line_for_pane(&self, pane: DiffPane) -> usize {
        match pane {
            DiffPane::Primary => self.selected_line,
            DiffPane::Secondary => self.secondary_selected_line,
        }
    }

    pub(super) async fn set_state(
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

    pub(super) async fn reload_review(&mut self, service: &ReviewService) -> Result<()> {
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

    pub(super) async fn refresh_review_and_diff(&mut self, service: &ReviewService) -> Result<()> {
        self.reload_review(service).await?;
        self.status_line = "review and diff refreshed".into();
        Ok(())
    }

    fn remap_comment_anchors(&mut self) -> bool {
        let mut changed = false;
        let mut updates = Vec::new();

        for comment in &self.review.comments {
            let file_index = self
                .diff
                .files
                .iter()
                .position(|file| file.path == comment.file_path);

            let Some(file_index) = file_index else {
                continue;
            };

            if file_index != self.active_file_index() {
                continue;
            }

            let rows = self.current_rows();
            let resolved = self.resolve_comment_anchor(comment, rows);

            if let Some(resolved) = resolved {
                if resolved.old_line != comment.old_line || resolved.new_line != comment.new_line {
                    updates.push(ReanchorLineComment {
                        comment_id: comment.id,
                        old_line: resolved.old_line,
                        new_line: resolved.new_line,
                        line_anchor: Some(resolved.line_anchor),
                    });
                    changed = true;
                }
            } else if comment.old_line.is_some() || comment.new_line.is_some() {
                updates.push(ReanchorLineComment {
                    comment_id: comment.id,
                    old_line: None,
                    new_line: None,
                    line_anchor: comment.line_anchor.clone(),
                });
                changed = true;
            }
        }

        if !updates.is_empty() {
            for update in updates {
                let _ = self.review.reanchor_comment(&update);
            }
        }

        changed
    }

    fn resolve_comment_anchor(
        &self,
        comment: &LineComment,
        rows: &[DisplayRow],
    ) -> Option<anchor::ResolvedLineAnchor> {
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
