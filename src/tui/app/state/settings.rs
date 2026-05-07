//! Settings and picker state operations.
//!
//! Handles theme pickers, commit pickers, review pickers, and settings editors.

use super::*;

impl TuiApp {
    pub(super) fn dismiss_blocking_overlays(&mut self) {
        self.command_palette = None;
        self.theme_picker = None;
        self.commit_picker = None;
        self.review_picker = None;
        self.settings_editor = None;
        self.command_prompt = None;
        self.shortcuts_modal_visible = false;
    }

    pub(super) fn open_command_prompt(&mut self, mode: CommandPromptMode) {
        self.dismiss_ai_progress_popup();
        let (value, cursor_col, status_line) = match mode {
            CommandPromptMode::GotoLine => (String::new(), 0, "goto line prompt"),
            CommandPromptMode::Search => {
                let value = self.search_query.clone().unwrap_or_default();
                let cursor_col = value.chars().count();
                (value, cursor_col, "search prompt")
            }
        };
        self.command_prompt = Some(CommandPromptState {
            mode,
            value,
            cursor_col,
        });
        self.status_line = status_line.into();
    }

    pub(super) fn open_help_docs(&mut self) {
        self.dismiss_ai_progress_popup();
        self.shortcuts_modal_visible = true;
        self.shortcuts_modal_scroll = 0;
        self.shortcuts_modal_doc_index = 0;
        self.status_line = "help docs opened".into();
    }

    pub(super) fn open_user_name_editor(&mut self) {
        self.dismiss_ai_progress_popup();
        let value = self.config.user_name.clone();
        let cursor_col = value.chars().count();
        self.settings_editor = Some(SettingsEditorState {
            kind: SettingsEditorKind::UserName,
            value,
            cursor_col,
        });
        self.status_line = "editing user name".into();
    }

    pub(super) fn open_create_review_editor(&mut self) {
        self.dismiss_ai_progress_popup();
        self.review_picker = None;
        self.settings_editor = Some(SettingsEditorState {
            kind: SettingsEditorKind::CreateReview,
            value: String::new(),
            cursor_col: 0,
        });
        self.status_line = "creating review".into();
    }

    pub(super) async fn save_settings_editor(&mut self, service: &ReviewService) -> Result<()> {
        let Some(editor) = self.settings_editor.take() else {
            return Ok(());
        };

        match editor.kind {
            SettingsEditorKind::UserName => {
                let next = editor.value.trim();
                if next.is_empty() {
                    self.status_line = "user name cannot be empty".into();
                    self.settings_editor = Some(SettingsEditorState {
                        kind: SettingsEditorKind::UserName,
                        value: editor.value,
                        cursor_col: editor.cursor_col,
                    });
                    return Ok(());
                }
                self.config.user_name = next.to_string();
                service.save_config(&self.config).await?;
                self.status_line = format!("user name set to {}", self.config.user_name);
            }
            SettingsEditorKind::CreateReview => {
                let next = editor.value.trim();
                if next.is_empty() {
                    self.status_line = "review name cannot be empty".into();
                    self.settings_editor = Some(SettingsEditorState {
                        kind: SettingsEditorKind::CreateReview,
                        value: editor.value,
                        cursor_col: editor.cursor_col,
                    });
                    return Ok(());
                }
                let review = service.create_review(next).await?;
                self.review_name = review.name.clone();
                self.review = review;
                self.log_path = service.review_log_path(&self.review_name)?;
                self.selected_comment = 0;
                self.expanded_threads.clear();
                self.collapsed_threads.clear();
                self.clear_diff_render_cache();
                self.constrain_selection();
                self.status_line = format!("created review {}", self.review_name);
            }
        }
        Ok(())
    }

    pub(super) fn open_theme_picker(&mut self) {
        if self.themes.is_empty() {
            self.status_line = "no themes loaded".into();
            return;
        }
        self.dismiss_ai_progress_popup();
        self.theme_picker = Some(super::ThemePickerState {
            selected_index: self.theme_index,
            scroll: self.theme_index.saturating_sub(3),
        });
        self.status_line = "theme picker opened".into();
    }

    pub(super) fn open_commit_picker(&mut self) -> Result<()> {
        let commits = crate::git::history::recent_commits(200)?;
        if commits.is_empty() {
            self.status_line = "commit picker unavailable: no commits found".into();
            return Ok(());
        }
        self.dismiss_ai_progress_popup();
        self.commit_picker = Some(super::CommitPickerState {
            commits: commits
                .into_iter()
                .map(|commit| super::CommitPickerEntry {
                    oid: commit.oid,
                    short_oid: commit.short_oid,
                    summary: commit.summary,
                })
                .collect(),
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        self.status_line = "commit picker opened".into();
        Ok(())
    }

    pub(super) async fn open_review_picker(&mut self, service: &ReviewService) -> Result<()> {
        let review_names = service.list_reviews().await?;
        if review_names.is_empty() {
            self.status_line = "review picker unavailable: no reviews found".into();
            return Ok(());
        }

        let mut reviews = Vec::with_capacity(review_names.len());
        for name in review_names {
            let review = service
                .load_review(&name)
                .await
                .with_context(|| format!("failed to load review {name}"))?;
            let open_count = review
                .comments
                .iter()
                .filter(|comment| comment.status == crate::domain::review::CommentStatus::Open)
                .count();
            let pending_count = review
                .comments
                .iter()
                .filter(|comment| comment.status == crate::domain::review::CommentStatus::Pending)
                .count();
            let addressed_count = review
                .comments
                .iter()
                .filter(|comment| comment.status == crate::domain::review::CommentStatus::Addressed)
                .count();
            reviews.push(super::ReviewPickerEntry {
                name: review.name,
                state: review.state,
                open_count,
                pending_count,
                addressed_count,
            });
        }

        let selected_index = reviews
            .iter()
            .position(|review| review.name == self.review_name)
            .unwrap_or(0);
        self.dismiss_ai_progress_popup();
        self.review_picker = Some(super::ReviewPickerState {
            reviews,
            query: String::new(),
            cursor_col: 0,
            selected_index,
            scroll: selected_index.saturating_sub(3),
        });
        self.status_line = "review picker opened".into();
        Ok(())
    }

    pub(super) fn commit_picker_filtered_indices(&self) -> Vec<usize> {
        let Some(picker) = self.commit_picker.as_ref() else {
            return Vec::new();
        };
        let needle = picker.query.trim().to_ascii_lowercase();
        picker
            .commits
            .iter()
            .enumerate()
            .filter(|(_, commit)| {
                if needle.is_empty() {
                    return true;
                }
                commit.oid.to_ascii_lowercase().contains(&needle)
                    || commit.short_oid.to_ascii_lowercase().contains(&needle)
                    || commit.summary.to_ascii_lowercase().contains(&needle)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub(super) fn review_picker_filtered_indices(&self) -> Vec<usize> {
        let Some(picker) = self.review_picker.as_ref() else {
            return Vec::new();
        };
        let needle = picker.query.trim().to_ascii_lowercase();
        picker
            .reviews
            .iter()
            .enumerate()
            .filter(|(_, review)| {
                if needle.is_empty() {
                    return true;
                }
                let state = match review.state {
                    ReviewState::Open => "open",
                    ReviewState::UnderReview => "under_review",
                    ReviewState::Done => "done",
                };
                review.name.to_ascii_lowercase().contains(&needle) || state.contains(&needle)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub(super) async fn apply_theme_picker_selection(
        &mut self,
        service: &ReviewService,
    ) -> Result<()> {
        let Some(picker) = self.theme_picker.take() else {
            return Ok(());
        };
        let next_index = picker
            .selected_index
            .min(self.themes.len().saturating_sub(1));
        self.theme_index = next_index;
        self.config.theme = self.theme().name.clone();
        service.save_config(&self.config).await?;
        self.row_cache.clear();
        self.clear_diff_render_cache();
        self.status_line = format!("theme set to {}", self.config.theme);
        Ok(())
    }

    pub(super) async fn toggle_light_dark_theme(&mut self, service: &ReviewService) -> Result<()> {
        if self.themes.is_empty() {
            self.status_line = "no themes loaded".into();
            return Ok(());
        }

        let current = self.theme().name.clone();
        let candidate = if let Some(prefix) = current.strip_suffix("_dark") {
            format!("{prefix}_light")
        } else if let Some(prefix) = current.strip_suffix("_light") {
            format!("{prefix}_dark")
        } else if current.contains("dark") {
            current.replace("dark", "light")
        } else if current.contains("light") {
            current.replace("light", "dark")
        } else {
            "gruvbox_light".to_string()
        };

        let target_index = resolve_theme_index(&self.themes, &candidate).unwrap_or_else(|| {
            resolve_theme_index(&self.themes, "gruvbox_light")
                .or_else(|| resolve_theme_index(&self.themes, "gruvbox_dark"))
                .unwrap_or(self.theme_index)
        });

        self.theme_index = target_index;
        self.config.theme = self.theme().name.clone();
        service.save_config(&self.config).await?;
        self.row_cache.clear();
        self.clear_diff_render_cache();
        self.status_line = format!("theme set to {}", self.config.theme);
        Ok(())
    }

    pub(super) fn help_docs_count(&self) -> usize {
        super::help_docs::HELP_DOCS.len()
    }

    pub(super) fn cycle_help_doc(&mut self, forward: bool) {
        let count = self.help_docs_count();
        let current = self.shortcuts_modal_doc_index.min(count.saturating_sub(1));
        let next = if forward {
            (current + 1) % count
        } else if current == 0 {
            count.saturating_sub(1)
        } else {
            current - 1
        };
        self.shortcuts_modal_doc_index = next;
        self.shortcuts_modal_scroll = 0;
    }

    pub(super) fn set_help_doc_index(&mut self, index: usize) {
        let count = self.help_docs_count();
        self.shortcuts_modal_doc_index = index.min(count.saturating_sub(1));
        self.shortcuts_modal_scroll = 0;
    }

    pub(super) fn resize_help_modal(&mut self, delta: i16) {
        let next = self.shortcuts_modal_zoom_step.saturating_add(delta);
        self.shortcuts_modal_zoom_step = next.clamp(-8, 12);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_modal_overlays_hides_ai_progress_popup() {
        let mut app = make_test_app(vec!["src/a.rs"], vec![]);
        app.ai_progress_visible = true;

        app.open_help_docs();
        assert!(!app.ai_progress_visible);
    }

    #[test]
    fn review_picker_filter_matches_name_and_state() {
        let mut app = make_test_app(vec!["src/a.rs"], vec![]);
        app.review_picker = Some(super::ReviewPickerState {
            reviews: vec![
                super::ReviewPickerEntry {
                    name: "test-review".to_string(),
                    state: ReviewState::Open,
                    open_count: 1,
                    pending_count: 0,
                    addressed_count: 0,
                },
                super::ReviewPickerEntry {
                    name: "done-review".to_string(),
                    state: ReviewState::Done,
                    open_count: 0,
                    pending_count: 0,
                    addressed_count: 5,
                },
            ],
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });

        let filtered = app.review_picker_filtered_indices();
        assert_eq!(filtered.len(), 2);

        app.review_picker.as_mut().unwrap().query = "done".to_string();
        let filtered_done = app.review_picker_filtered_indices();
        assert_eq!(filtered_done.len(), 1);
    }

    #[test]
    fn showing_ai_progress_popup_closes_other_blocking_overlays() {
        let mut app = make_test_app(vec!["src/a.rs"], vec![]);
        app.shortcuts_modal_visible = true;
        app.toggle_ai_progress_popup();
        assert!(app.ai_progress_visible);
        assert!(!app.shortcuts_modal_visible);
    }
}
