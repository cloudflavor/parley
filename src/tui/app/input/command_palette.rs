use super::*;
use crate::tui::app::help_docs::HELP_DOCS;

impl TuiApp {
    pub(super) fn open_command_palette(&mut self) {
        self.dismiss_ai_progress_popup();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        self.status_line = "command palette opened".into();
    }

    pub(in crate::tui::app) fn command_palette_items() -> Vec<CommandPaletteItem> {
        vec![
            CommandPaletteItem {
                action: CommandPaletteAction::RefreshReviewAndDiff,
                label: "Refresh Review + Diff",
                keywords: "reload sync",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleFullscreen,
                label: "Toggle Content Fullscreen",
                keywords: "layout zoom",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleSplitDiff,
                label: "Toggle Split Diff View",
                keywords: "layout pane",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleSideBySideDiff,
                label: "Toggle Side-by-Side Diff",
                keywords: "layout unified",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleThreadNavigator,
                label: "Toggle Thread Navigator",
                keywords: "thread sidebar",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::JumpNextThread,
                label: "Jump to Next Thread",
                keywords: "thread next",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::JumpPrevThread,
                label: "Jump to Previous Thread",
                keywords: "thread prev previous",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleThreadDensityMode,
                label: "Cycle Thread Density Mode",
                keywords: "thread compact expanded",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleSelectedThreadExpansion,
                label: "Toggle Selected Thread Expanded",
                keywords: "thread expand collapse selected",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::SetReviewOpen,
                label: "Set Review State: Open",
                keywords: "state workflow",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::SetReviewUnderReview,
                label: "Set Review State: Under Review",
                keywords: "state workflow",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::SetReviewDone,
                label: "Set Review State: Done",
                keywords: "state workflow complete",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleFileFilter,
                label: "Cycle File Filter",
                keywords: "files open pending",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleFileSort,
                label: "Cycle File Sort",
                keywords: "files order ranking",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleActiveFileGroup,
                label: "Toggle Active File Group",
                keywords: "files group collapse expand",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CollapseAllFileGroups,
                label: "Collapse All File Groups",
                keywords: "files group collapse all",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenUserNameEditor,
                label: "Edit User Name",
                keywords: "settings profile",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenThemePicker,
                label: "Open Theme Picker",
                keywords: "theme colors",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::ToggleLightDarkTheme,
                label: "Toggle Theme Light/Dark Variant",
                keywords: "theme light dark",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CycleAiProvider,
                label: "Cycle AI Provider",
                keywords: "ai model provider",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::RunAiReviewRefactor,
                label: "Run AI Refactor on Review",
                keywords: "ai run review refactor",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::RunAiThreadRefactor,
                label: "Run AI Refactor on Selected Thread",
                keywords: "ai run thread refactor",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::RunAiThreadReply,
                label: "Run AI Reply on Selected Thread",
                keywords: "ai run thread reply",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::CancelAiRun,
                label: "Cancel AI Run",
                keywords: "ai stop cancel",
            },
            CommandPaletteItem {
                action: CommandPaletteAction::OpenShortcuts,
                label: "Open Help Docs",
                keywords: "help docs keys",
            },
        ]
    }

    pub(in crate::tui::app) fn command_palette_filtered_items(
        query: &str,
        items: &[CommandPaletteItem],
    ) -> Vec<CommandPaletteItem> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return items.to_vec();
        }
        let needle = trimmed.to_lowercase();
        items
            .iter()
            .filter(|item| {
                item.label.to_lowercase().contains(&needle)
                    || item.keywords.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    pub(super) fn handle_shortcuts_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.shortcuts_modal_visible = false;
                self.status_line = "help docs closed".into();
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                self.cycle_help_doc(false);
                if let Some(doc) = HELP_DOCS.get(self.shortcuts_modal_doc_index) {
                    self.status_line = format!("help doc: {}", doc.title);
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                self.cycle_help_doc(true);
                if let Some(doc) = HELP_DOCS.get(self.shortcuts_modal_doc_index) {
                    self.status_line = format!("help doc: {}", doc.title);
                }
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                let digit = ch as usize - '0' as usize;
                if digit > 0 {
                    self.set_help_doc_index(digit - 1);
                    if let Some(doc) = HELP_DOCS.get(self.shortcuts_modal_doc_index) {
                        self.status_line = format!("help doc: {}", doc.title);
                    }
                }
            }
            KeyCode::Char('<') => {
                self.resize_help_modal(-1);
                self.status_line = "help zoom out".into();
            }
            KeyCode::Char('>') => {
                self.resize_help_modal(1);
                self.status_line = "help zoom in".into();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_sub(8);
            }
            KeyCode::PageDown => {
                self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_add(8);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.shortcuts_modal_scroll = 0;
            }
            KeyCode::End => {
                self.shortcuts_modal_scroll = usize::MAX;
            }
            KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.shortcuts_modal_scroll = usize::MAX;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) async fn handle_command_palette_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            self.command_palette = None;
            self.status_line = "command palette closed".into();
            return Ok(());
        }

        let all_items = Self::command_palette_items();
        let filtered_items = if let Some(palette) = self.command_palette.as_ref() {
            Self::command_palette_filtered_items(&palette.query, &all_items)
        } else {
            Vec::new()
        };

        if matches!(key.code, KeyCode::Enter) {
            let selected_action = self
                .command_palette
                .as_ref()
                .and_then(|palette| filtered_items.get(palette.selected_index))
                .map(|item| item.action);
            if let Some(action) = selected_action {
                self.command_palette = None;
                if let Err(error) = self.apply_command_palette_action(action, service).await {
                    self.status_line = format!("command failed: {error}");
                }
            }
            return Ok(());
        }

        let Some(palette) = self.command_palette.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                palette.selected_index = palette.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max_index = filtered_items.len().saturating_sub(1);
                palette.selected_index = (palette.selected_index + 1).min(max_index);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                palette.selected_index = 0;
            }
            KeyCode::End => {
                palette.selected_index = filtered_items.len().saturating_sub(1);
            }
            KeyCode::PageUp => {
                palette.selected_index = palette.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = filtered_items.len().saturating_sub(1);
                palette.selected_index = (palette.selected_index + 8).min(max_index);
            }
            KeyCode::Left => {
                palette.cursor_col = palette.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                palette.cursor_col = (palette.cursor_col + 1).min(palette.query.chars().count());
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.cursor_col = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.cursor_col = palette.query.chars().count();
            }
            KeyCode::Backspace => {
                if palette.cursor_col > 0 {
                    remove_char_at(&mut palette.query, palette.cursor_col - 1);
                    palette.cursor_col -= 1;
                    palette.selected_index = 0;
                    palette.scroll = 0;
                }
            }
            KeyCode::Delete => {
                if palette.cursor_col < palette.query.chars().count() {
                    remove_char_at(&mut palette.query, palette.cursor_col);
                    palette.selected_index = 0;
                    palette.scroll = 0;
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut palette.query, palette.cursor_col, ch);
                palette.cursor_col += 1;
                palette.selected_index = 0;
                palette.scroll = 0;
            }
            _ => {}
        }

        let refreshed_items = Self::command_palette_filtered_items(&palette.query, &all_items);
        if refreshed_items.is_empty() {
            palette.selected_index = 0;
            palette.scroll = 0;
        } else {
            palette.selected_index = palette
                .selected_index
                .min(refreshed_items.len().saturating_sub(1));
            if palette.selected_index < palette.scroll {
                palette.scroll = palette.selected_index;
            }
            let lower_bound = palette.scroll.saturating_add(8);
            if palette.selected_index > lower_bound {
                palette.scroll = palette.selected_index.saturating_sub(8);
            }
        }
        Ok(())
    }

    async fn apply_command_palette_action(
        &mut self,
        action: CommandPaletteAction,
        service: &ReviewService,
    ) -> Result<()> {
        match action {
            CommandPaletteAction::ToggleFullscreen => {
                self.toggle_content_fullscreen();
            }
            CommandPaletteAction::ToggleSplitDiff => {
                self.toggle_split_diff_view();
                self.status_line = if self.split_diff_view {
                    "split diff enabled".into()
                } else {
                    "split diff disabled".into()
                };
            }
            CommandPaletteAction::ToggleSideBySideDiff => {
                self.side_by_side_diff = !self.side_by_side_diff;
                self.config.diff_view = if self.side_by_side_diff {
                    DiffViewMode::SideBySide
                } else {
                    DiffViewMode::Unified
                };
                service.save_config(&self.config).await?;
                self.clear_diff_render_cache();
                self.status_line = if self.side_by_side_diff {
                    "side-by-side diff enabled".into()
                } else {
                    "unified diff enabled".into()
                };
            }
            CommandPaletteAction::ToggleThreadNavigator => {
                self.thread_nav_visible = !self.thread_nav_visible;
                self.status_line = if self.thread_nav_visible {
                    "thread navigator visible".into()
                } else {
                    "thread navigator hidden".into()
                };
            }
            CommandPaletteAction::RefreshReviewAndDiff => {
                self.refresh_review_and_diff(service).await?;
                self.status_line = "refreshed review and diff".into();
            }
            CommandPaletteAction::SetReviewOpen => {
                self.set_state(service, ReviewState::Open).await?;
            }
            CommandPaletteAction::SetReviewUnderReview => {
                self.set_state(service, ReviewState::UnderReview).await?;
            }
            CommandPaletteAction::SetReviewDone => {
                let unresolved_ids = self.unresolved_thread_ids();
                if !unresolved_ids.is_empty() {
                    self.status_line = format!(
                        "done blocked: unresolved threads {}",
                        format_unresolved_ids(&unresolved_ids)
                    );
                    return Ok(());
                }
                self.set_state(service, ReviewState::Done).await?;
            }
            CommandPaletteAction::OpenUserNameEditor => {
                self.open_user_name_editor();
            }
            CommandPaletteAction::OpenThemePicker => {
                self.open_theme_picker();
            }
            CommandPaletteAction::ToggleLightDarkTheme => {
                self.toggle_light_dark_theme(service).await?;
            }
            CommandPaletteAction::CycleAiProvider => {
                self.cycle_ai_provider(service).await?;
            }
            CommandPaletteAction::RunAiReviewRefactor => {
                self.start_ai_session(service, false, AiSessionMode::Refactor)
                    .await?;
            }
            CommandPaletteAction::RunAiThreadRefactor => {
                self.start_ai_session(service, true, AiSessionMode::Refactor)
                    .await?;
            }
            CommandPaletteAction::RunAiThreadReply => {
                self.start_ai_session(service, true, AiSessionMode::Reply)
                    .await?;
            }
            CommandPaletteAction::CancelAiRun => {
                self.cancel_ai_task();
            }
            CommandPaletteAction::JumpNextThread => {
                self.ensure_row_cache();
                self.jump_thread(true);
            }
            CommandPaletteAction::JumpPrevThread => {
                self.ensure_row_cache();
                self.jump_thread(false);
            }
            CommandPaletteAction::CycleFileFilter => {
                self.cycle_file_filter_mode();
            }
            CommandPaletteAction::CycleFileSort => {
                self.cycle_file_sort_mode();
            }
            CommandPaletteAction::ToggleActiveFileGroup => {
                self.toggle_active_file_group_collapsed();
            }
            CommandPaletteAction::CollapseAllFileGroups => {
                self.collapse_all_visible_file_groups();
            }
            CommandPaletteAction::CycleThreadDensityMode => {
                self.cycle_thread_density_mode();
            }
            CommandPaletteAction::ToggleSelectedThreadExpansion => {
                self.toggle_selected_thread_expansion();
            }
            CommandPaletteAction::OpenShortcuts => {
                self.open_help_docs();
            }
        }
        self.constrain_selection();
        Ok(())
    }

    pub(super) async fn handle_normal_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if self.ai_progress_visible && self.handle_ai_progress_scroll_key(key) {
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
        if matches!(key.code, KeyCode::Char('z'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.pending_z_prefix_at = None;
            self.pending_action = Some(PendingUiAction::SuspendTuiProcess);
            self.status_line = "suspending parley; run `fg` to resume".into();
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
            KeyCode::Char('V') => {
                self.toggle_split_diff_view();
                self.status_line = if self.split_diff_view {
                    "split diff enabled".into()
                } else {
                    "split diff disabled".into()
                };
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
            KeyCode::Tab => {
                if self.split_diff_view {
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
            KeyCode::Char('/') => self.open_command_prompt(CommandPromptMode::Search),
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
            KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.cycle_thread_density_mode();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.cycle_thread_density_mode();
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
                self.cancel_ai_task();
            }
            KeyCode::Char('H') => self.toggle_ai_progress_popup(),
            KeyCode::Char('L') => {
                self.pending_action = Some(PendingUiAction::OpenLogsInLess);
                self.status_line = format!("opening logs in less: {}", self.log_path.display());
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
                        format_line_reference(comment.old_line, comment.new_line)
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
                        format_line_reference(comment.old_line, comment.new_line)
                    );
                }
            }
            KeyCode::Char('a') => {
                if let Some(comment) = self.selected_comment_details() {
                    let comment_id = comment.id;
                    match service
                        .mark_addressed(&self.review_name, comment_id, Author::User)
                        .await
                    {
                        Ok(_) => {
                            self.refresh_review_and_diff(service).await?;
                            self.status_line = format!("comment #{comment_id} marked addressed");
                        }
                        Err(error) => {
                            self.status_line = format!("mark addressed failed: {error}");
                        }
                    }
                }
            }
            KeyCode::Char('f') => {
                if let Some(comment) = self.selected_comment_details() {
                    let comment_id = comment.id;
                    match service
                        .force_mark_addressed(&self.review_name, comment_id)
                        .await
                    {
                        Ok(_) => {
                            self.refresh_review_and_diff(service).await?;
                            self.status_line = format!("comment #{comment_id} force-addressed");
                        }
                        Err(error) => {
                            self.status_line = format!("force address failed: {error}");
                        }
                    }
                }
            }
            KeyCode::Char('o') => {
                if let Some(comment) = self.selected_comment_details() {
                    let comment_id = comment.id;
                    match service
                        .mark_open(&self.review_name, comment_id, Author::User)
                        .await
                    {
                        Ok(_) => {
                            self.refresh_review_and_diff(service).await?;
                            self.status_line = format!("comment #{comment_id} marked open");
                        }
                        Err(error) => {
                            self.status_line = format!("mark open failed: {error}");
                        }
                    }
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
            KeyCode::Char('d') => {
                let unresolved_ids = self.unresolved_thread_ids();
                if !unresolved_ids.is_empty() {
                    self.status_line = format!(
                        "done blocked: unresolved threads {}",
                        format_unresolved_ids(&unresolved_ids)
                    );
                    self.constrain_selection();
                    return Ok(());
                }
                if let Err(error) = self.set_state(service, ReviewState::Done).await {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('D') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                match service
                    .set_state_force(&self.review_name, ReviewState::Done)
                    .await
                {
                    Ok(_) => {
                        self.reload_review(service).await?;
                        self.status_line = "review force-marked done".into();
                    }
                    Err(error) => {
                        self.status_line = format!("force done failed: {error}");
                    }
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

    fn handle_ai_progress_scroll_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::PageUp => {
                self.ai_progress_scroll_up(8);
                self.status_line = "ai stream scrolled up".into();
                true
            }
            KeyCode::PageDown => {
                self.ai_progress_scroll_down(8);
                self.status_line = "ai stream scrolled down".into();
                true
            }
            KeyCode::Home => {
                self.ai_progress_scroll_home();
                self.status_line = "ai stream at beginning".into();
                true
            }
            KeyCode::End => {
                self.ai_progress_scroll_end();
                self.status_line = "ai stream at latest output".into();
                true
            }
            _ => false,
        }
    }
}
