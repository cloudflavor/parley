use super::*;

impl TuiApp {
    pub(super) async fn handle_settings_editor_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.settings_editor = None;
            self.status_line = "settings edit cancelled".into();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Enter) {
            return self.save_settings_editor(service).await;
        }

        let Some(editor) = self.settings_editor.as_mut() else {
            return Ok(());
        };

        match key.code {
            KeyCode::Left => {
                editor.cursor_col = editor.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                editor.cursor_col = (editor.cursor_col + 1).min(editor.value.chars().count());
            }
            KeyCode::Home => editor.cursor_col = 0,
            KeyCode::End => editor.cursor_col = editor.value.chars().count(),
            KeyCode::Backspace if editor.cursor_col > 0 => {
                remove_char_at(&mut editor.value, editor.cursor_col - 1);
                editor.cursor_col -= 1;
            }
            KeyCode::Delete if editor.cursor_col < editor.value.chars().count() => {
                remove_char_at(&mut editor.value, editor.cursor_col);
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut editor.value, editor.cursor_col, ch);
                editor.cursor_col += 1;
            }
            _ => {}
        }

        Ok(())
    }

    pub(super) async fn handle_theme_picker_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.theme_picker = None;
            self.status_line = "theme picker closed".into();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Enter) {
            return self.apply_theme_picker_selection(service).await;
        }

        let Some(picker) = self.theme_picker.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                picker.selected_index = picker.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max_index = self.themes.len().saturating_sub(1);
                picker.selected_index = (picker.selected_index + 1).min(max_index);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                picker.selected_index = 0;
            }
            KeyCode::End => {
                picker.selected_index = self.themes.len().saturating_sub(1);
            }
            KeyCode::PageUp => {
                picker.selected_index = picker.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = self.themes.len().saturating_sub(1);
                picker.selected_index = (picker.selected_index + 8).min(max_index);
            }
            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                picker.selected_index = self.themes.len().saturating_sub(1);
            }
            _ => {}
        }
        if picker.selected_index < picker.scroll {
            picker.scroll = picker.selected_index;
        }
        let lower_bound = picker.scroll.saturating_add(8);
        if picker.selected_index > lower_bound {
            picker.scroll = picker.selected_index.saturating_sub(8);
        }
        Ok(())
    }

    pub(super) async fn handle_commit_picker_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.commit_picker = None;
            self.status_line = "commit picker closed".into();
            return Ok(());
        }

        if matches!(key.code, KeyCode::Enter) {
            let filtered = self.commit_picker_filtered_indices();
            let Some(picker) = self.commit_picker.as_ref() else {
                return Ok(());
            };
            if filtered.is_empty() {
                self.status_line = "no commits match the current search".into();
                return Ok(());
            }
            let selected = picker.selected_index.min(filtered.len().saturating_sub(1));
            let commit = picker
                .commits
                .get(filtered[selected])
                .cloned()
                .context("selected commit is unavailable")?;
            self.commit_picker = None;
            self.diff_source = crate::git::diff::DiffSource::Commit { rev: commit.oid };
            self.refresh_review_and_diff(service).await?;
            self.status_line = format!("diff source set to {}", commit.short_oid);
            return Ok(());
        }

        let filtered_len = self.commit_picker_filtered_indices().len();
        let Some(picker) = self.commit_picker.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up => {
                picker.selected_index = picker.selected_index.saturating_sub(1);
            }
            KeyCode::Down => {
                let max_index = filtered_len.saturating_sub(1);
                picker.selected_index = (picker.selected_index + 1).min(max_index);
            }
            KeyCode::Home => {
                picker.selected_index = 0;
            }
            KeyCode::End => {
                picker.selected_index = filtered_len.saturating_sub(1);
            }
            KeyCode::PageUp => {
                picker.selected_index = picker.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = filtered_len.saturating_sub(1);
                picker.selected_index = (picker.selected_index + 8).min(max_index);
            }
            KeyCode::Left => {
                picker.cursor_col = picker.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                picker.cursor_col = (picker.cursor_col + 1).min(picker.query.chars().count());
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.cursor_col = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.cursor_col = picker.query.chars().count();
            }
            KeyCode::Backspace if picker.cursor_col > 0 => {
                remove_char_at(&mut picker.query, picker.cursor_col - 1);
                picker.cursor_col -= 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            KeyCode::Delete if picker.cursor_col < picker.query.chars().count() => {
                remove_char_at(&mut picker.query, picker.cursor_col);
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut picker.query, picker.cursor_col, ch);
                picker.cursor_col += 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            _ => {}
        }

        let refreshed_len = self.commit_picker_filtered_indices().len();
        if let Some(picker) = self.commit_picker.as_mut() {
            if refreshed_len == 0 {
                picker.selected_index = 0;
                picker.scroll = 0;
            } else {
                picker.selected_index = picker.selected_index.min(refreshed_len.saturating_sub(1));
                if picker.selected_index < picker.scroll {
                    picker.scroll = picker.selected_index;
                }
                let lower_bound = picker.scroll.saturating_add(8);
                if picker.selected_index > lower_bound {
                    picker.scroll = picker.selected_index.saturating_sub(8);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn handle_review_picker_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.review_picker = None;
            self.status_line = "review picker closed".into();
            return Ok(());
        }

        if matches!(key.code, KeyCode::Enter) {
            let filtered = self.review_picker_filtered_indices();
            let Some(picker) = self.review_picker.as_ref() else {
                return Ok(());
            };
            if filtered.is_empty() {
                let name = picker.query.trim().to_string();
                self.review_picker = None;
                self.settings_editor = Some(SettingsEditorState {
                    kind: SettingsEditorKind::CreateReview,
                    cursor_col: name.chars().count(),
                    value: name,
                });
                self.status_line = "creating review".into();
                return Ok(());
            }
            let selected = picker.selected_index.min(filtered.len().saturating_sub(1));
            let review = picker
                .reviews
                .get(filtered[selected])
                .cloned()
                .context("selected review is unavailable")?;
            self.review_picker = None;
            self.review_name = review.name.clone();
            self.log_path = service.review_log_path(&self.review_name)?;
            self.reload_review(service).await?;
            self.status_line = format!("review context set to {}", review.name);
            return Ok(());
        }

        let filtered_len = self.review_picker_filtered_indices().len();
        let Some(picker) = self.review_picker.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up => {
                picker.selected_index = picker.selected_index.saturating_sub(1);
            }
            KeyCode::Down => {
                let max_index = filtered_len.saturating_sub(1);
                picker.selected_index = (picker.selected_index + 1).min(max_index);
            }
            KeyCode::Home => {
                picker.selected_index = 0;
            }
            KeyCode::End => {
                picker.selected_index = filtered_len.saturating_sub(1);
            }
            KeyCode::PageUp => {
                picker.selected_index = picker.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = filtered_len.saturating_sub(1);
                picker.selected_index = (picker.selected_index + 8).min(max_index);
            }
            KeyCode::Left => {
                picker.cursor_col = picker.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                picker.cursor_col = (picker.cursor_col + 1).min(picker.query.chars().count());
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.cursor_col = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.cursor_col = picker.query.chars().count();
            }
            KeyCode::Backspace if picker.cursor_col > 0 => {
                remove_char_at(&mut picker.query, picker.cursor_col - 1);
                picker.cursor_col -= 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            KeyCode::Delete if picker.cursor_col < picker.query.chars().count() => {
                remove_char_at(&mut picker.query, picker.cursor_col);
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut picker.query, picker.cursor_col, ch);
                picker.cursor_col += 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            _ => {}
        }

        let refreshed_len = self.review_picker_filtered_indices().len();
        if let Some(picker) = self.review_picker.as_mut() {
            if refreshed_len == 0 {
                picker.selected_index = 0;
                picker.scroll = 0;
            } else {
                picker.selected_index = picker.selected_index.min(refreshed_len.saturating_sub(1));
                if picker.selected_index < picker.scroll {
                    picker.scroll = picker.selected_index;
                }
                let lower_bound = picker.scroll.saturating_add(8);
                if picker.selected_index > lower_bound {
                    picker.scroll = picker.selected_index.saturating_sub(8);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn handle_worktree_picker_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.worktree_picker = None;
            self.status_line = "worktree picker closed".into();
            return Ok(());
        }

        if matches!(key.code, KeyCode::Enter) {
            return self.apply_worktree_picker_selection(service).await;
        }

        let filtered_len = self.worktree_picker_filtered_indices().len();
        let Some(picker) = self.worktree_picker.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up => {
                picker.selected_index = picker.selected_index.saturating_sub(1);
            }
            KeyCode::Down => {
                let max_index = filtered_len.saturating_sub(1);
                picker.selected_index = (picker.selected_index + 1).min(max_index);
            }
            KeyCode::Home => {
                picker.selected_index = 0;
            }
            KeyCode::End => {
                picker.selected_index = filtered_len.saturating_sub(1);
            }
            KeyCode::PageUp => {
                picker.selected_index = picker.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = filtered_len.saturating_sub(1);
                picker.selected_index = (picker.selected_index + 8).min(max_index);
            }
            KeyCode::Left => {
                picker.cursor_col = picker.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                picker.cursor_col = (picker.cursor_col + 1).min(picker.query.chars().count());
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.cursor_col = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.cursor_col = picker.query.chars().count();
            }
            KeyCode::Backspace if picker.cursor_col > 0 => {
                remove_char_at(&mut picker.query, picker.cursor_col - 1);
                picker.cursor_col -= 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            KeyCode::Delete if picker.cursor_col < picker.query.chars().count() => {
                remove_char_at(&mut picker.query, picker.cursor_col);
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut picker.query, picker.cursor_col, ch);
                picker.cursor_col += 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            _ => {}
        }

        let refreshed_len = self.worktree_picker_filtered_indices().len();
        if let Some(picker) = self.worktree_picker.as_mut() {
            if refreshed_len == 0 {
                picker.selected_index = 0;
                picker.scroll = 0;
            } else {
                picker.selected_index = picker.selected_index.min(refreshed_len.saturating_sub(1));
                if picker.selected_index < picker.scroll {
                    picker.scroll = picker.selected_index;
                }
                let lower_bound = picker.scroll.saturating_add(8);
                if picker.selected_index > lower_bound {
                    picker.scroll = picker.selected_index.saturating_sub(8);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn apply_worktree_picker_selection(
        &mut self,
        service: &ReviewService,
    ) -> Result<()> {
        let filtered = self.worktree_picker_filtered_indices();
        let Some(picker) = self.worktree_picker.as_ref() else {
            return Ok(());
        };
        if filtered.is_empty() {
            self.status_line = "no worktrees match the current search".into();
            return Ok(());
        }
        let selected = picker.selected_index.min(filtered.len().saturating_sub(1));
        let entry = picker
            .worktrees
            .get(filtered[selected])
            .cloned()
            .context("selected worktree is unavailable")?;
        self.worktree_picker = None;

        let new_path = std::path::PathBuf::from(&entry.path);
        self.worktree_path = new_path.clone();
        self.config.last_worktree = Some(entry.name.clone());
        service.save_config(&self.config).await?;

        // Clear caches and reload diff for the new worktree
        self.refresh_review_and_diff(service).await?;
        self.code_search = None;
        self.search_query = None;
        self.file_heatmap_task = None;
        self.file_heatmap = None;
        self.row_cache.clear();
        self.clear_diff_render_cache();
        self.invalidate_visible_file_indices_cache();

        self.status_line = format!("worktree set to {}", entry.name);
        Ok(())
    }

    pub(crate) fn worktree_picker_filtered_indices(&self) -> Vec<usize> {
        let Some(picker) = self.worktree_picker.as_ref() else {
            return Vec::new();
        };
        let trimmed = picker.query.trim().to_lowercase();
        if trimmed.is_empty() {
            return (0..picker.worktrees.len()).collect();
        }
        picker
            .worktrees
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.name.to_lowercase().contains(&trimmed)
                    || entry.path.to_lowercase().contains(&trimmed)
                    || entry.branch.to_lowercase().contains(&trimmed)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub(super) async fn handle_branch_picker_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.branch_picker = None;
                self.status_line = "branch picker closed".into();
            }
            KeyCode::Enter => {
                self.apply_branch_picker_selection(service).await?;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let Some(picker) = self.branch_picker.as_mut() else {
                    return Ok(());
                };
                if picker.selected_index > 0 {
                    picker.selected_index -= 1;
                    if picker.selected_index < picker.scroll {
                        picker.scroll = picker.selected_index;
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let filtered = self.branch_picker_filtered_indices();
                let Some(picker) = self.branch_picker.as_mut() else {
                    return Ok(());
                };
                if picker.selected_index < filtered.len().saturating_sub(1) {
                    picker.selected_index += 1;
                    let visible = 10;
                    if picker.selected_index >= picker.scroll + visible {
                        picker.scroll = picker.selected_index.saturating_sub(visible - 1);
                    }
                }
            }
            KeyCode::Backspace => {
                let Some(picker) = self.branch_picker.as_mut() else {
                    return Ok(());
                };
                if picker.cursor_col > 0 && !picker.query.is_empty() {
                    picker.cursor_col -= 1;
                    picker.query.remove(picker.cursor_col);
                }
            }
            KeyCode::Left => {
                let Some(picker) = self.branch_picker.as_mut() else {
                    return Ok(());
                };
                if picker.cursor_col > 0 {
                    picker.cursor_col -= 1;
                }
            }
            KeyCode::Right => {
                let Some(picker) = self.branch_picker.as_mut() else {
                    return Ok(());
                };
                if picker.cursor_col < picker.query.len() {
                    picker.cursor_col += 1;
                }
            }
            KeyCode::Home => {
                if let Some(picker) = self.branch_picker.as_mut() {
                    picker.cursor_col = 0;
                }
            }
            KeyCode::End => {
                if let Some(picker) = self.branch_picker.as_mut() {
                    picker.cursor_col = picker.query.len();
                }
            }
            KeyCode::Char(c) if c.is_ascii() && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let Some(picker) = self.branch_picker.as_mut() else {
                    return Ok(());
                };
                let cursor = picker.cursor_col;
                picker.query.insert(cursor, c);
                picker.cursor_col += 1;
                picker.selected_index = 0;
                picker.scroll = 0;
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn branch_picker_filtered_indices(&self) -> Vec<usize> {
        let Some(picker) = self.branch_picker.as_ref() else {
            return Vec::new();
        };
        let needle = picker.query.trim().to_ascii_lowercase();
        picker
            .branches
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                if needle.is_empty() {
                    return true;
                }
                entry.name.to_ascii_lowercase().contains(&needle)
            })
            .map(|(idx, _)| idx)
            .collect()
    }
}
