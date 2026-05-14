//! Review state operations.
//!
//! Handles review state transitions, reloading, and persistence.

use super::*;
use crate::utils::cast::{i16_to_u16_saturating, u16_to_i16_saturating};

impl TuiApp {
    pub(crate) async fn poll_root_directory_diff_load(
        &mut self,
        _service: &ReviewService,
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
        self.invalidate_visible_file_indices_cache();
        self.row_cache.clear();
        self.root_hydrated_files.clear();
        self.clear_diff_render_cache();
        self.refresh_comment_anchor_projections();

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
            self.refresh_comment_anchor_projections();
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
        let wt = self.worktree_path.clone();
        self.root_file_load_task = Some(task::spawn(async move {
            load_root_directory_file(&config, path, &wt)
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
        self.ensure_row_cache_for_file(self.selected_file);
        self.start_root_file_hydration_if_needed(self.selected_file);
        if self.split_diff_view {
            self.ensure_row_cache_for_file(self.secondary_selected_file);
            self.start_root_file_hydration_if_needed(self.secondary_selected_file);
        }
        self.constrain_selection();
        self.invalidate_redraw();
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
        self.refresh_comment_anchor_projections();
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
        self.diff = load_git_diff(&self.config, &self.diff_source, &self.worktree_path).await?;
        self.invalidate_visible_file_indices_cache();
        self.row_cache.clear();
        self.root_hydrated_files.clear();
        self.clear_diff_render_cache();
        self.refresh_comment_anchor_projections();

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crate::domain::review::SourceAnchorSnapshot;
    use crate::persistence::store::Store;
    use crate::services::review_service::ReviewService;
    use crate::tui::app::state::tests::{
        diff_file_with_context_lines, make_comment_with_anchor, make_test_app,
        make_test_app_with_files_and_comments,
    };
    use anyhow::anyhow;
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

    #[test]
    fn exact_anchor_projection_matches_original_snapshot_text() -> Result<()> {
        let comment = comment_with_original_anchor(1, "src/a.rs", 2, "fn target() {}");
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(1, "fn before() {}"), (2, "fn target() {}")],
            )],
            vec![comment],
        )?;
        let row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(2))
            .ok_or_else(|| anyhow!("projected row should exist"))?;

        assert!(app.comment_matches_current_projection(&app.review.comments[0], row));
        Ok(())
    }

    #[test]
    fn exact_anchor_projection_falls_back_to_line_matching_when_outdated() -> Result<()> {
        let comment = comment_with_original_anchor(1, "src/a.rs", 2, "fn target() {}");
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(1, "fn before() {}"), (2, "fn changed() {}")],
            )],
            vec![comment],
        )?;
        let row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(2))
            .ok_or_else(|| anyhow!("candidate row should exist"))?;
        let stored = &app.review.comments[0];

        assert!(app.comment_matches_current_projection(stored, row));
        assert_eq!(stored.old_line, Some(2));
        assert_eq!(stored.new_line, Some(2));
        assert!(!stored.detached);
        Ok(())
    }

    #[test]
    fn exact_anchor_projection_does_not_follow_moved_text_automatically() -> Result<()> {
        let comment = comment_with_original_anchor(1, "src/a.rs", 2, "fn target() {}");
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[
                    (1, "fn before() {}"),
                    (2, "fn replacement() {}"),
                    (9, "fn target() {}"),
                ],
            )],
            vec![comment],
        )?;
        let moved_row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(9))
            .ok_or_else(|| anyhow!("moved row should exist"))?;

        assert!(!app.comment_matches_current_projection(&app.review.comments[0], moved_row));
        assert_eq!(
            app.projected_comment_reference(&app.review.comments[0]),
            "2:2"
        );
        Ok(())
    }

    #[test]
    fn exact_anchor_projection_matches_deleted_line_on_left_side() -> Result<()> {
        let comment = comment_with_side_original_anchor(
            1,
            "src/a.rs",
            DiffSide::Left,
            Some(4),
            None,
            "fn deleted() {}",
        );
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_lines(
                "src/a.rs",
                vec![
                    diff_context_line(3, 3, "fn before() {}"),
                    diff_removed_line(4, "fn deleted() {}"),
                    diff_context_line(5, 4, "fn after() {}"),
                ],
            )],
            vec![comment],
        )?;
        let deleted_row = app
            .current_rows()
            .iter()
            .find(|row| row.old_line == Some(4) && row.new_line.is_none())
            .ok_or_else(|| anyhow!("deleted row should exist"))?;

        assert!(app.comment_matches_current_projection(&app.review.comments[0], deleted_row));
        Ok(())
    }

    #[test]
    fn exact_anchor_projection_keeps_same_file_threads_separate() -> Result<()> {
        let first = comment_with_original_anchor(1, "src/a.rs", 2, "fn first() {}");
        let second = comment_with_original_anchor(2, "src/a.rs", 3, "fn second() {}");
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(2, "fn first() {}"), (3, "fn second() {}")],
            )],
            vec![first, second],
        )?;
        let first_row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(2))
            .ok_or_else(|| anyhow!("first row should exist"))?;
        let second_row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(3))
            .ok_or_else(|| anyhow!("second row should exist"))?;

        assert!(app.comment_matches_current_projection(&app.review.comments[0], first_row));
        assert!(!app.comment_matches_current_projection(&app.review.comments[0], second_row));
        assert!(app.comment_matches_current_projection(&app.review.comments[1], second_row));
        assert!(!app.comment_matches_current_projection(&app.review.comments[1], first_row));
        Ok(())
    }

    #[test]
    fn exact_anchor_projection_falls_back_in_root_mode_when_file_changed() -> Result<()> {
        let mut comment = comment_with_original_anchor(1, "src/a.rs", 2, "fn original() {}");
        if let Some(anchor) = comment.original_anchor.as_mut() {
            anchor.source = Some(SourceAnchorSnapshot {
                file_content_hash: Some("old-file-hash".to_string()),
                selected_text_hash: Some("old-selected-hash".to_string()),
            });
        }
        let mut app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(1, "fn before() {}"), (2, "fn changed() {}")],
            )],
            vec![comment],
        )?;
        app.diff_source = DiffSource::RootDirectory;
        app.refresh_comment_anchor_projections();
        let changed_row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(2))
            .ok_or_else(|| anyhow!("changed root row should exist"))?;

        assert!(app.comment_matches_current_projection(&app.review.comments[0], changed_row));
        assert_eq!(
            app.projected_comment_reference(&app.review.comments[0]),
            "2:2"
        );
        Ok(())
    }

    fn comment_with_original_anchor(
        id: u64,
        file_path: &str,
        line: u32,
        selected_text: &str,
    ) -> LineComment {
        comment_with_side_original_anchor(
            id,
            file_path,
            DiffSide::Right,
            Some(line),
            Some(line),
            selected_text,
        )
    }

    fn comment_with_side_original_anchor(
        id: u64,
        file_path: &str,
        side: DiffSide,
        old_line: Option<u32>,
        new_line: Option<u32>,
        selected_text: &str,
    ) -> LineComment {
        let line = new_line.or(old_line).unwrap_or(1);
        let mut comment = make_comment_with_anchor(id, file_path, CommentStatus::Open, line, line);
        comment.line_anchor = None;
        comment.side = side;
        comment.old_line = old_line;
        comment.new_line = new_line;
        comment.original_anchor = Some(StoredAnchorSnapshot {
            file_path: file_path.to_string(),
            side,
            old_line,
            new_line,
            line_range: None,
            selected_text: selected_text.to_string(),
            before_context: Vec::new(),
            after_context: Vec::new(),
            diff: None,
            source: None,
            base_rev: None,
            head_rev: None,
        });
        comment
    }

    fn diff_file_with_lines(path: &str, lines: Vec<DiffLine>) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            header_lines: vec![
                format!("diff --git a/{path} b/{path}"),
                format!("--- a/{path}"),
                format!("+++ b/{path}"),
            ],
            hunks: vec![DiffHunk {
                header: "@@ -1,3 +1,3 @@".to_string(),
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 3,
                lines,
            }],
        }
    }

    fn diff_context_line(old_line: u32, new_line: u32, code: &str) -> DiffLine {
        DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(old_line),
            new_line: Some(new_line),
            raw: format!(" {code}"),
            code: code.to_string(),
        }
    }

    fn diff_removed_line(old_line: u32, code: &str) -> DiffLine {
        DiffLine {
            kind: DiffLineKind::Removed,
            old_line: Some(old_line),
            new_line: None,
            raw: format!("-{code}"),
            code: code.to_string(),
        }
    }

    #[test]
    fn detached_comment_is_still_invisible_to_projection_fallback() -> Result<()> {
        let mut comment = make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 2, 2);
        comment.detached = true;
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(1, "fn before() {}"), (2, "fn target() {}")],
            )],
            vec![comment],
        )?;
        let row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(2))
            .ok_or_else(|| anyhow!("row at line 2 should exist"))?;

        assert!(!app.comment_matches_current_projection(&app.review.comments[0], row));
        Ok(())
    }

    #[test]
    fn comment_without_projection_matches_by_line_number() -> Result<()> {
        let mut comment = make_comment_with_anchor(1, "src/a.rs", CommentStatus::Open, 2, 2);
        comment.original_anchor = None;
        comment.line_anchor = None;
        let app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(1, "fn before() {}"), (2, "fn target() {}")],
            )],
            vec![comment],
        )?;
        let row = app
            .current_rows()
            .iter()
            .find(|row| row.new_line == Some(2))
            .ok_or_else(|| anyhow!("row at line 2 should exist"))?;

        assert!(app.comment_matches_current_projection(&app.review.comments[0], row));
        Ok(())
    }
}
