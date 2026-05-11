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
}
