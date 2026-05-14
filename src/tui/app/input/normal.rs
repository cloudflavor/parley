use super::*;
use crate::domain::review::CommentStatus;

impl TuiApp {
    pub(super) async fn handle_normal_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if self.ai_progress_visible && self.handle_ai_progress_key(key)? {
            return Ok(());
        }

        if matches!(key.code, KeyCode::Char('k')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.open_command_palette();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Char('f')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.file_search.focused = true;
            self.file_search.cursor_col = self.file_search.query.chars().count();
            self.status_line = "editing file filter".into();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Char('z')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.pending_z_prefix_at = None;
            self.pending_action = Some(PendingUiAction::SuspendTuiProcess);
            self.status_line = "suspending parley; run `fg` to resume".into();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Char('t' | 'T'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.open_thread_selector();
            return Ok(());
        }
        if matches!(key.code, KeyCode::Char('w')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Err(error) = self.open_worktree_picker().await {
                self.status_line = format!("worktree picker failed: {error}");
            }
            return Ok(());
        }

        if matches!(key.code, KeyCode::Char('z')) && key.modifiers.is_empty() {
            if let Some(pressed_at) = self.pending_z_prefix_at
                && pressed_at.elapsed() < Self::Z_PREFIX_TIMEOUT
            {
                self.pending_z_prefix_at = None;
                self.center_active_cursor_in_viewport();
                self.constrain_selection();
                return Ok(());
            }
            self.pending_z_prefix_at = Some(Instant::now());
            self.status_line = "z pending: press z again to center".into();
            return Ok(());
        }

        if self.pending_z_prefix_at.take().is_some() {
            self.toggle_content_fullscreen();
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.open_help_docs(),
            KeyCode::PageUp => {
                self.scroll_active_pane_page(false, false);
                self.status_line = "paged up".into();
            }
            KeyCode::PageDown => {
                self.scroll_active_pane_page(true, false);
                self.status_line = "paged down".into();
            }
            KeyCode::Char('v' | 'V') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_split_diff_view();
                self.status_line = if self.split_diff_view {
                    "split view enabled".into()
                } else {
                    "split view disabled".into()
                };
            }
            KeyCode::Char('v' | 'V') => {
                self.ensure_row_cache();
                self.toggle_comment_line_selection();
            }
            KeyCode::Char('S') => {
                self.side_by_side_diff = !self.side_by_side_diff;
                self.config.diff_view = if self.side_by_side_diff {
                    DiffViewMode::SideBySide
                } else {
                    DiffViewMode::Unified
                };
                if let Err(error) = service.save_config(&self.config).await {
                    self.status_line = format!("failed to persist diff view mode: {error}");
                    return Ok(());
                }
                self.clear_diff_render_cache();
                self.status_line = if self.side_by_side_diff {
                    "side-by-side diff enabled".into()
                } else {
                    "unified diff enabled".into()
                };
            }
            KeyCode::Tab if self.split_diff_view => {
                let next = if matches!(self.active_diff_pane, DiffPane::Primary) {
                    DiffPane::Secondary
                } else {
                    DiffPane::Primary
                };
                self.activate_pane(next);
                self.status_line = format!(
                    "active pane: {}",
                    if matches!(next, DiffPane::Primary) {
                        "primary"
                    } else {
                        "secondary"
                    }
                );
            }
            KeyCode::Char('<') => {
                self.resize_file_pane(-3);
                self.status_line = "files pane narrowed".into();
            }
            KeyCode::Char('>') => {
                self.resize_file_pane(3);
                self.status_line = "files pane widened".into();
            }
            KeyCode::Char('b') => {
                self.thread_nav_visible = !self.thread_nav_visible;
                if self.thread_nav_visible {
                    self.status_line = "thread navigator visible".into();
                } else {
                    self.status_line = "thread navigator hidden".into();
                }
            }
            KeyCode::Char('M') => {
                self.start_file_heatmap();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.toggle_root_document_rendering();
            }
            KeyCode::Char('D') => {
                self.toggle_root_document_rendering();
            }
            KeyCode::Char('F') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.cycle_file_filter_mode();
            }
            KeyCode::Char('O') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.cycle_file_sort_mode();
            }
            KeyCode::Enter => {
                self.toggle_active_file_group_collapsed();
            }
            KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.collapse_all_visible_file_groups();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_file_selection(-1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_file_selection(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.ensure_row_cache();
                self.set_active_line_index(self.active_line_index().saturating_sub(1));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.ensure_row_cache();
                let max = self.current_rows().len().saturating_sub(1);
                self.set_active_line_index((self.active_line_index() + 1).min(max));
            }
            KeyCode::Char('g') => {
                self.set_active_line_index(0);
            }
            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.ensure_row_cache();
                self.set_active_line_index(self.current_rows().len().saturating_sub(1));
            }
            KeyCode::Char('c') | KeyCode::Char('m') => {
                self.ensure_row_cache();
                self.toggle_inline_comment_for_selected_line();
            }
            KeyCode::Char('r') => {
                self.ensure_row_cache();
                self.start_inline_reply_for_selected_comment();
            }
            KeyCode::Char(':') => self.open_command_prompt(CommandPromptMode::GotoLine),
            KeyCode::Char('/') => {
                self.open_command_prompt(CommandPromptMode::SearchCurrentFile);
            }
            KeyCode::Char('n') => {
                self.ensure_row_cache();
                self.jump_search(true);
            }
            KeyCode::Char('p') => {
                self.ensure_row_cache();
                self.jump_search(false);
            }
            KeyCode::Char('N') => {
                self.ensure_row_cache();
                self.jump_thread(true);
            }
            KeyCode::Char('P') => {
                self.ensure_row_cache();
                self.jump_thread(false);
            }
            KeyCode::Char('e') if key.modifiers.is_empty() => {
                self.toggle_selected_thread_expansion();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_selected_thread_anchor_expansion();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_active_pane_page(false, true);
                self.status_line = "half-page up".into();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_active_pane_page(true, true);
                self.status_line = "half-page down".into();
            }
            KeyCode::Char('U') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.open_user_name_editor();
            }
            KeyCode::Char('u') => {
                self.ensure_row_cache();
                if let Err(error) = self.reanchor_selected_comment(service).await {
                    self.status_line = format!("re-anchor failed: {error}");
                }
            }
            KeyCode::Char('i') => {
                if let Err(error) = self.cycle_ai_provider(service).await {
                    self.status_line = format!("ai provider change failed: {error}");
                }
            }
            KeyCode::Char('I') => {
                if let Err(error) = self.toggle_ai_transport(service).await {
                    self.status_line = format!("ai transport change failed: {error}");
                }
            }
            KeyCode::Char('A') => {
                if let Err(error) = self
                    .start_ai_session(service, false, AiSessionMode::Refactor)
                    .await
                {
                    self.status_line = format!("run ai session failed: {error}");
                }
            }
            KeyCode::Char('x') => {
                if let Err(error) = self
                    .start_ai_session(service, true, AiSessionMode::Refactor)
                    .await
                {
                    self.status_line = format!("run ai thread failed: {error}");
                }
            }
            KeyCode::Char('X') => {
                if let Err(error) = self
                    .start_ai_session(service, true, AiSessionMode::Reply)
                    .await
                {
                    self.status_line = format!("run ai thread failed: {error}");
                }
            }
            KeyCode::Char('K') => {
                self.cancel_ai_task().await;
            }
            KeyCode::Char('H') => self.toggle_ai_progress_popup(),
            KeyCode::Char('L') => {
                self.toggle_ai_activity_overlay();
            }
            KeyCode::Char('t') => {
                self.open_theme_picker();
            }
            KeyCode::Char('T') => {
                if let Err(error) = self.toggle_light_dark_theme(service).await {
                    self.status_line = format!("theme variant toggle failed: {error}");
                }
            }
            KeyCode::Char(']') => {
                let max = self.comments_for_selected_file().len().saturating_sub(1);
                self.selected_comment = (self.selected_comment + 1).min(max);
                self.focus_selected_comment_line();
                self.request_scroll_to_thread_tail(self.active_diff_pane, self.active_line_index());
                if let Some(comment) = self.selected_comment_details() {
                    self.status_line = format!(
                        "selected thread #{} at line {}",
                        comment.id,
                        format_comment_reference(comment)
                    );
                }
            }
            KeyCode::Char('[') => {
                self.selected_comment = self.selected_comment.saturating_sub(1);
                self.focus_selected_comment_line();
                self.request_scroll_to_thread_tail(self.active_diff_pane, self.active_line_index());
                if let Some(comment) = self.selected_comment_details() {
                    self.status_line = format!(
                        "selected thread #{} at line {}",
                        comment.id,
                        format_comment_reference(comment)
                    );
                }
            }
            KeyCode::Char('a') => {
                if let Err(error) = self
                    .mark_selected_comment_status(service, CommentStatus::Addressed, false)
                    .await
                {
                    self.status_line = format!("mark addressed failed: {error}");
                }
            }
            KeyCode::Char('f') => {
                if let Err(error) = self
                    .mark_selected_comment_status(service, CommentStatus::Addressed, true)
                    .await
                {
                    self.status_line = format!("force address failed: {error}");
                }
            }
            KeyCode::Char('o') => {
                if let Err(error) = self
                    .mark_selected_comment_status(service, CommentStatus::Open, false)
                    .await
                {
                    self.status_line = format!("mark open failed: {error}");
                }
            }
            KeyCode::Char('s') => {
                if let Err(error) = self.set_state(service, ReviewState::Open).await {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('w') => {
                if let Err(error) = self.set_state(service, ReviewState::UnderReview).await {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('R') => {
                if let Err(error) = self.refresh_review_and_diff(service).await {
                    self.status_line = format!("refresh failed: {error}");
                } else {
                    self.status_line = "refreshed review and diff".into();
                }
            }
            _ => {}
        }

        self.constrain_selection();
        Ok(())
    }

    fn handle_ai_progress_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::PageUp => {
                self.ai_progress_scroll_up(8);
                self.status_line = "ai stream scrolled up".into();
                Ok(true)
            }
            KeyCode::PageDown => {
                self.ai_progress_scroll_down(8);
                self.status_line = "ai stream scrolled down".into();
                Ok(true)
            }
            KeyCode::Home => {
                self.ai_progress_scroll_home();
                self.status_line = "ai stream at beginning".into();
                Ok(true)
            }
            KeyCode::End => {
                self.ai_progress_scroll_end();
                self.status_line = "ai stream at latest output".into();
                Ok(true)
            }
            KeyCode::Char('O' | 'o') => {
                self.queue_ai_log_pager();
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
