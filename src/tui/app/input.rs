use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

use crate::{
    domain::{
        ai::AiSessionMode,
        config::DiffViewMode,
        diff::DiffLineKind,
        review::{Author, DiffSide, LineComment, ReviewState},
    },
    services::review_service::{AddCommentInput, AddReplyInput, ReviewService},
};

use super::{
    CommandPaletteAction, CommandPaletteItem, CommandPaletteState, CommandPromptMode,
    CommandPromptState, CommentTarget, DiffPane, INLINE_FILE_MENTION_MAX_CANDIDATES,
    INLINE_FILE_MENTION_MAX_VISIBLE_ROWS, InlineCommentState, InlineDraftMode,
    InlineFileMentionState, MOUSE_WHEEL_FILE_SCROLL_FILES, MOUSE_WHEEL_SCROLL_LINES,
    PendingUiAction, ReplyTarget, TextBuffer, ThreadAnchor, TuiApp, comment_matches_display_row,
    format_line_reference, insert_char_at, point_in_rect, remove_char_at,
};

impl TuiApp {
    const Z_PREFIX_TIMEOUT: Duration = Duration::from_millis(275);

    pub(super) fn flush_pending_key_sequences(&mut self) -> bool {
        if let Some(pressed_at) = self.pending_z_prefix_at
            && pressed_at.elapsed() >= Self::Z_PREFIX_TIMEOUT
        {
            self.pending_z_prefix_at = None;
            self.toggle_content_fullscreen();
            return true;
        }
        false
    }

    pub(super) async fn handle_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if self.shortcuts_modal_visible {
            return self.handle_shortcuts_modal_key(key);
        }
        if self.command_palette.is_some() {
            return self.handle_command_palette_key(key, service).await;
        }
        if self.theme_picker.is_some() {
            return self.handle_theme_picker_key(key, service).await;
        }
        if self.settings_editor.is_some() {
            return self.handle_settings_editor_key(key, service).await;
        }
        if self.inline_comment.is_some() {
            return self.handle_inline_comment_key(key, service).await;
        }
        if self.command_prompt.is_some() {
            return self.handle_command_prompt_key(key);
        }
        if self.file_search.focused {
            return self.handle_file_search_key(key);
        }

        self.handle_normal_key(key, service).await
    }

    fn handle_file_search_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter)
            || (matches!(key.code, KeyCode::Char('f'))
                && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.file_search.focused = false;
            self.status_line = if self.file_search_query().is_some() {
                format!("file filter active: {}", self.file_search.query.trim())
            } else {
                "file filter cleared".into()
            };
            return Ok(());
        }

        match key.code {
            KeyCode::Left => {
                self.file_search.cursor_col = self.file_search.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                self.file_search.cursor_col =
                    (self.file_search.cursor_col + 1).min(self.file_search.query.chars().count());
            }
            KeyCode::Home => self.file_search.cursor_col = 0,
            KeyCode::End => self.file_search.cursor_col = self.file_search.query.chars().count(),
            KeyCode::Backspace => {
                if self.file_search.cursor_col > 0 {
                    remove_char_at(&mut self.file_search.query, self.file_search.cursor_col - 1);
                    self.file_search.cursor_col -= 1;
                }
            }
            KeyCode::Delete => {
                if self.file_search.cursor_col < self.file_search.query.chars().count() {
                    remove_char_at(&mut self.file_search.query, self.file_search.cursor_col);
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut self.file_search.query, self.file_search.cursor_col, ch);
                self.file_search.cursor_col += 1;
            }
            _ => {}
        }

        self.constrain_active_file_to_visible_list();
        self.constrain_selection();
        self.status_line = if self.file_search_query().is_some() {
            format!("file filter: {}", self.file_search.query.trim())
        } else {
            "file filter cleared".into()
        };
        Ok(())
    }

    fn toggle_content_fullscreen(&mut self) {
        self.content_fullscreen = !self.content_fullscreen;
        if self.content_fullscreen {
            self.status_line = "content fullscreen enabled".into();
        } else {
            self.status_line = "content fullscreen disabled".into();
        }
    }

    fn scroll_active_pane_page(&mut self, forward: bool, half_page: bool) {
        self.ensure_row_cache();
        let pane = self.active_diff_pane;
        let viewport_height = self.viewport_height_for_pane(pane);
        let step = if half_page {
            (viewport_height / 2).max(1)
        } else {
            viewport_height.max(1)
        };

        let row_map: Vec<usize> = self.row_map_for_pane(pane).to_vec();
        let cursor_source_row = self.line_for_pane(pane);
        let cursor_visual_row = row_map
            .iter()
            .position(|row| *row == cursor_source_row)
            .unwrap_or_else(|| cursor_source_row.min(row_map.len().saturating_sub(1)));
        let previous_top = self.viewport_top_for_pane(pane);
        let cursor_offset = cursor_visual_row.saturating_sub(previous_top);

        let mut next_top = if forward {
            previous_top.saturating_add(step)
        } else {
            previous_top.saturating_sub(step)
        };
        if !row_map.is_empty() {
            let max_top = row_map.len().saturating_sub(viewport_height);
            next_top = next_top.min(max_top);
        }
        self.set_viewport_top_for_pane(pane, next_top);

        if row_map.is_empty() {
            let max_source = self.current_rows().len().saturating_sub(1);
            let next_source = if forward {
                cursor_source_row.saturating_add(step).min(max_source)
            } else {
                cursor_source_row.saturating_sub(step)
            };
            self.set_line_for_pane(pane, next_source);
            return;
        }

        let next_visual = (next_top + cursor_offset).min(row_map.len().saturating_sub(1));
        self.set_line_for_pane(pane, row_map[next_visual]);
    }

    fn center_active_cursor_in_viewport(&mut self) {
        let pane = self.active_diff_pane;
        let viewport_height = self.viewport_height_for_pane(pane);
        let cursor_source_row = self.line_for_pane(pane);
        let cursor_visual_row = self
            .row_map_for_pane(pane)
            .iter()
            .position(|row| *row == cursor_source_row)
            .unwrap_or(cursor_source_row);
        let next_top = cursor_visual_row.saturating_sub(viewport_height / 2);
        self.set_viewport_top_for_pane(pane, next_top);
        self.status_line = "cursor centered in viewport".into();
    }

    fn open_command_palette(&mut self) {
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        self.status_line = "command palette opened".into();
    }

    pub(super) fn command_palette_items() -> Vec<CommandPaletteItem> {
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
                label: "Open Shortcuts Help",
                keywords: "help keys",
            },
        ]
    }

    pub(super) fn command_palette_filtered_items(
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

    fn handle_shortcuts_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.shortcuts_modal_visible = false;
                self.status_line = "shortcuts help closed".into();
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

    async fn handle_command_palette_key(
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
                self.shortcuts_modal_visible = true;
                self.shortcuts_modal_scroll = 0;
                self.status_line = "shortcuts help opened".into();
            }
        }
        self.constrain_selection();
        Ok(())
    }

    async fn handle_normal_key(&mut self, key: KeyEvent, service: &ReviewService) -> Result<()> {
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
            KeyCode::Char('?') => {
                self.shortcuts_modal_visible = true;
                self.shortcuts_modal_scroll = 0;
                self.status_line = "shortcuts help opened".into();
            }
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
            KeyCode::Char(':') => {
                self.command_prompt = Some(CommandPromptState {
                    mode: CommandPromptMode::GotoLine,
                    value: String::new(),
                    cursor_col: 0,
                });
                self.status_line = "goto line prompt".into();
            }
            KeyCode::Char('/') => {
                self.command_prompt = Some(CommandPromptState {
                    mode: CommandPromptMode::Search,
                    value: self.search_query.clone().unwrap_or_default(),
                    cursor_col: self
                        .search_query
                        .as_ref()
                        .map(|value| value.chars().count())
                        .unwrap_or(0),
                });
                self.status_line = "search prompt".into();
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
            KeyCode::Char('u') => {
                self.open_user_name_editor();
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
            KeyCode::Char('H') => {
                self.ai_progress_visible = !self.ai_progress_visible;
                if self.ai_progress_visible {
                    self.ai_progress_scroll_end();
                }
                self.status_line = if self.ai_progress_visible {
                    "ai progress popup visible".into()
                } else {
                    "ai progress popup hidden".into()
                };
            }
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

    async fn handle_settings_editor_key(
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
            KeyCode::Backspace => {
                if editor.cursor_col > 0 {
                    remove_char_at(&mut editor.value, editor.cursor_col - 1);
                    editor.cursor_col -= 1;
                }
            }
            KeyCode::Delete => {
                if editor.cursor_col < editor.value.chars().count() {
                    remove_char_at(&mut editor.value, editor.cursor_col);
                }
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

    async fn handle_theme_picker_key(
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

    fn handle_command_prompt_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            if let Some(prompt) = self.command_prompt.take() {
                if matches!(prompt.mode, CommandPromptMode::Search) {
                    self.search_query = None;
                    self.status_line = "search cleared".into();
                } else {
                    self.status_line = "command cancelled".into();
                }
            } else {
                self.status_line = "command cancelled".into();
            }
            return Ok(());
        }
        if matches!(key.code, KeyCode::Enter) {
            return self.run_command_prompt();
        }

        let Some(prompt) = self.command_prompt.as_mut() else {
            return Ok(());
        };

        match key.code {
            KeyCode::Left => {
                prompt.cursor_col = prompt.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                prompt.cursor_col = (prompt.cursor_col + 1).min(prompt.value.chars().count());
            }
            KeyCode::Home => prompt.cursor_col = 0,
            KeyCode::End => prompt.cursor_col = prompt.value.chars().count(),
            KeyCode::Backspace => {
                if prompt.cursor_col > 0 {
                    remove_char_at(&mut prompt.value, prompt.cursor_col - 1);
                    prompt.cursor_col -= 1;
                }
            }
            KeyCode::Delete => {
                if prompt.cursor_col < prompt.value.chars().count() {
                    remove_char_at(&mut prompt.value, prompt.cursor_col);
                }
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut prompt.value, prompt.cursor_col, ch);
                prompt.cursor_col += 1;
            }
            _ => {}
        }

        Ok(())
    }

    fn run_command_prompt(&mut self) -> Result<()> {
        let Some(prompt) = self.command_prompt.take() else {
            return Ok(());
        };

        match prompt.mode {
            CommandPromptMode::GotoLine => self.goto_line_from_prompt(&prompt.value),
            CommandPromptMode::Search => self.search_from_prompt(&prompt.value),
        }
    }

    fn goto_line_from_prompt(&mut self, input: &str) -> Result<()> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            self.status_line = "goto line expects a number".into();
            return Ok(());
        }

        let Ok(target) = trimmed.parse::<u32>() else {
            self.status_line = format!("invalid line number: {trimmed}");
            return Ok(());
        };

        if self.goto_line_number(target) {
            self.status_line = format!("jumped to line {target}");
        } else {
            self.status_line = format!("line {target} not found in current diff file");
        }
        Ok(())
    }

    fn goto_line_number(&mut self, target: u32) -> bool {
        if target == 0 {
            return false;
        }
        self.ensure_row_cache();

        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.new_line == Some(target))
        {
            self.set_active_line_index(row_index);
            return true;
        }

        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.old_line == Some(target))
        {
            self.set_active_line_index(row_index);
            return true;
        }

        false
    }

    fn search_from_prompt(&mut self, input: &str) -> Result<()> {
        let query = input.trim();
        if query.is_empty() {
            self.search_query = None;
            self.status_line = "search cleared".into();
            return Ok(());
        }
        self.search_query = Some(query.to_string());
        if self.find_search_match(query, true) {
            self.status_line = format!("search match: {query}");
        } else {
            self.status_line = format!("no match for: {query}");
        }
        Ok(())
    }

    fn jump_search(&mut self, forward: bool) {
        let Some(query) = self.search_query.clone() else {
            self.status_line = "no active search (use /text)".into();
            return;
        };

        if self.find_search_match(&query, forward) {
            self.status_line = format!("search match: {query}");
        } else if self.current_rows_contain_query(&query) {
            self.status_line = format!("no further match for: {query}");
        } else {
            self.search_query = None;
            self.status_line = format!("search cleared (no matches): {query}");
        }
    }

    fn current_rows_contain_query(&mut self, query: &str) -> bool {
        self.ensure_row_cache();
        let needle = query.to_lowercase();
        self.current_rows()
            .iter()
            .any(|row| row.raw.to_lowercase().contains(&needle))
    }

    fn find_search_match(&mut self, query: &str, forward: bool) -> bool {
        self.ensure_row_cache();
        let rows = self.current_rows();
        let query_lower = query.to_lowercase();
        if !rows.is_empty() {
            let len = rows.len();
            let mut index = self.active_line_index();

            for _ in 0..len {
                index = if forward {
                    (index + 1) % len
                } else {
                    (index + len - 1) % len
                };

                let haystack = rows[index].raw.to_lowercase();
                if haystack.contains(&query_lower) {
                    self.set_active_line_index(index);
                    return true;
                }
            }
        }

        let files_len = self.diff.files.len();
        if files_len == 0 {
            return false;
        }

        let mut file_index = self.active_file_index();
        for _ in 0..files_len {
            file_index = if forward {
                (file_index + 1) % files_len
            } else {
                (file_index + files_len - 1) % files_len
            };

            let path_matches = self.diff.files[file_index]
                .path
                .to_lowercase()
                .contains(&query_lower);
            if !path_matches {
                continue;
            }

            self.select_file(file_index);
            self.ensure_row_cache_for_file(file_index);

            let first_row_match = self
                .current_rows()
                .iter()
                .enumerate()
                .find(|(_, row)| row.raw.to_lowercase().contains(&query_lower))
                .map(|(idx, _)| idx);
            if let Some(row_idx) = first_row_match {
                self.set_active_line_index(row_idx);
            }
            return true;
        }

        false
    }

    fn jump_thread(&mut self, forward: bool) {
        self.ensure_row_cache();
        let comments = self.comments_for_selected_file();
        if comments.is_empty() {
            self.status_line = "no comments in current file".into();
            return;
        }

        let mut anchors: Vec<ThreadAnchor> = comments
            .iter()
            .enumerate()
            .filter_map(|(comment_index, comment)| {
                self.current_rows()
                    .iter()
                    .position(|row| comment_matches_display_row(comment, row))
                    .map(|row_index| ThreadAnchor {
                        comment_index,
                        row_index,
                        comment_id: comment.id,
                        old_line: comment.old_line,
                        new_line: comment.new_line,
                    })
            })
            .collect();
        if anchors.is_empty() {
            self.status_line = "no thread anchors visible in current file".into();
            return;
        }

        anchors.sort_by_key(|anchor| (anchor.row_index, anchor.comment_index));
        let current_row = self.active_line_index();
        let current_comment = self.selected_comment;

        let target = if forward {
            anchors
                .iter()
                .copied()
                .find(|anchor| {
                    anchor.row_index > current_row
                        || (anchor.row_index == current_row
                            && anchor.comment_index > current_comment)
                })
                .unwrap_or(anchors[0])
        } else {
            anchors
                .iter()
                .rev()
                .copied()
                .find(|anchor| {
                    anchor.row_index < current_row
                        || (anchor.row_index == current_row
                            && anchor.comment_index < current_comment)
                })
                .unwrap_or(*anchors.last().expect("anchors checked as non-empty"))
        };

        self.selected_comment = target.comment_index;
        self.set_active_line_index(target.row_index);
        self.request_scroll_to_thread_tail(self.active_diff_pane, target.row_index);
        self.status_line = format!(
            "thread #{} at line {}",
            target.comment_id,
            format_line_reference(target.old_line, target.new_line)
        );
    }

    fn resolve_file_reference_hit(
        &self,
        pane: DiffPane,
        rendered_row_index: usize,
        content_col: usize,
    ) -> Option<(String, Option<u32>)> {
        let hits = if matches!(pane, DiffPane::Primary) {
            &self.last_diff_link_hits
        } else {
            &self.last_diff_link_hits_secondary
        };
        hits.iter()
            .find(|hit| {
                hit.rendered_row_index == rendered_row_index
                    && content_col >= hit.col_start
                    && content_col < hit.col_end
            })
            .map(|hit| (hit.path.clone(), hit.line))
    }

    fn follow_file_reference(&mut self, pane: DiffPane, raw_path: &str, line: Option<u32>) {
        self.activate_pane(pane);
        let Some(file_index) = self.resolve_file_reference_index(raw_path) else {
            self.status_line = format!("referenced file not in current diff: {raw_path}");
            return;
        };

        self.select_file(file_index);
        if let Some(target_line) = line {
            if self.goto_line_number(target_line) {
                self.status_line = format!(
                    "jumped to {}:{}",
                    self.diff.files[file_index].path, target_line
                );
            } else {
                self.status_line = format!(
                    "opened {}, line {} not found in visible diff hunk",
                    self.diff.files[file_index].path, target_line
                );
            }
        } else {
            self.status_line = format!("opened {}", self.diff.files[file_index].path);
        }
    }

    fn resolve_file_reference_index(&self, raw_path: &str) -> Option<usize> {
        let cleaned = raw_path.trim().trim_start_matches("./").replace('\\', "/");
        if cleaned.is_empty() {
            return None;
        }
        if let Some(index) = self.diff.files.iter().position(|file| file.path == cleaned) {
            return Some(index);
        }

        let slash_cleaned = if cleaned.starts_with('/') {
            cleaned.clone()
        } else {
            format!("/{cleaned}")
        };
        self.diff.files.iter().position(|file| {
            cleaned.ends_with(&file.path) || slash_cleaned.ends_with(&format!("/{}", file.path))
        })
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.shortcuts_modal_visible {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_sub(2);
                }
                MouseEventKind::ScrollDown => {
                    self.shortcuts_modal_scroll = self.shortcuts_modal_scroll.saturating_add(2);
                }
                _ => {}
            }
            return Ok(());
        }

        if self.command_palette.is_some()
            || self.theme_picker.is_some()
            || self.settings_editor.is_some()
            || self.command_prompt.is_some()
        {
            return Ok(());
        }

        if let Some(ai_area) = self.last_ai_progress_area
            && point_in_rect(mouse.column, mouse.row, ai_area)
        {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.ai_progress_scroll_up(2);
                }
                MouseEventKind::ScrollDown => {
                    self.ai_progress_scroll_down(2);
                }
                _ => {}
            }
            return Ok(());
        }

        if let Some(thread_area) = self.last_thread_nav_area
            && point_in_rect(mouse.column, mouse.row, thread_area)
        {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > thread_area.y
                        && mouse.row < thread_area.y + thread_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(thread_area.y + 1));
                    let row_index = self.last_thread_nav_scroll + view_row;
                    if let Some(&comment_index) = self.last_thread_nav_row_map.get(row_index)
                        && comment_index != usize::MAX
                    {
                        self.selected_comment = comment_index;
                        self.focus_selected_comment_line();
                        if let Some(comment) = self.selected_comment_details() {
                            self.status_line = format!(
                                "selected thread #{} at {}",
                                comment.id,
                                format_line_reference(comment.old_line, comment.new_line)
                            );
                        }
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.selected_comment = self.selected_comment.saturating_sub(1);
                    self.focus_selected_comment_line();
                }
                MouseEventKind::ScrollDown => {
                    let max = self.comments_for_selected_file().len().saturating_sub(1);
                    self.selected_comment = (self.selected_comment + 1).min(max);
                    self.focus_selected_comment_line();
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(file_area) = self.last_file_area
            && point_in_rect(mouse.column, mouse.row, file_area)
        {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > file_area.y
                        && mouse.row < file_area.y + file_area.height.saturating_sub(1) =>
                {
                    let visual_row = self.last_file_scroll
                        + usize::from(mouse.row.saturating_sub(file_area.y + 1));
                    if let Some(Some(file_index)) = self.last_file_row_map.get(visual_row) {
                        self.select_file(*file_index);
                        if self.active_file_index() < self.diff.files.len() {
                            self.status_line = format!(
                                "selected file {}",
                                self.diff.files[self.active_file_index()].path
                            );
                        }
                    } else if let Some(Some(group)) =
                        self.last_file_group_map.get(visual_row).cloned()
                    {
                        self.toggle_file_group_collapsed(&group);
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.move_file_selection(-(MOUSE_WHEEL_FILE_SCROLL_FILES as isize));
                }
                MouseEventKind::ScrollDown => {
                    self.move_file_selection(MOUSE_WHEEL_FILE_SCROLL_FILES as isize);
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(search_area) = self.last_file_search_area
            && point_in_rect(mouse.column, mouse.row, search_area)
        {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && mouse.row > search_area.y
                && mouse.row < search_area.y + search_area.height.saturating_sub(1)
            {
                const SEARCH_PREFIX: &str = "search> ";
                let inner_width = usize::from(search_area.width.saturating_sub(2)).max(1);
                let query_width = inner_width.saturating_sub(SEARCH_PREFIX.chars().count());
                let horizontal_scroll = self
                    .file_search
                    .cursor_col
                    .saturating_sub(query_width.saturating_sub(1));
                let content_start = search_area
                    .x
                    .saturating_add(1)
                    .saturating_add(SEARCH_PREFIX.chars().count() as u16);
                let clicked_col = usize::from(mouse.column.saturating_sub(content_start));
                let target_col = horizontal_scroll.saturating_add(clicked_col);
                self.file_search.focused = true;
                self.file_search.cursor_col =
                    target_col.min(self.file_search.query.chars().count());
                self.status_line = "file filter input focused".into();
            }
            return Ok(());
        }

        if let Some(diff_area) = self.last_diff_area
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.activate_pane(DiffPane::Primary);
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll + view_row;
                    let content_col =
                        usize::from(mouse.column.saturating_sub(diff_area.x.saturating_add(1)));
                    if let Some((path, line)) = self.resolve_file_reference_hit(
                        DiffPane::Primary,
                        visible_row_index,
                        content_col,
                    ) {
                        self.follow_file_reference(DiffPane::Primary, &path, line);
                        return Ok(());
                    }
                    if let Some(row_index) = self.last_diff_row_map.get(visible_row_index).copied()
                    {
                        self.set_active_line_index(row_index);
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.set_active_line_index(
                        self.active_line_index()
                            .saturating_sub(MOUSE_WHEEL_SCROLL_LINES),
                    );
                }
                MouseEventKind::ScrollDown => {
                    let max = self.current_rows().len().saturating_sub(1);
                    self.set_active_line_index(
                        self.active_line_index()
                            .saturating_add(MOUSE_WHEEL_SCROLL_LINES)
                            .min(max),
                    );
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        if let Some(diff_area) = self.last_diff_area_secondary
            && point_in_rect(mouse.column, mouse.row, diff_area)
        {
            self.activate_pane(DiffPane::Secondary);
            self.ensure_row_cache();
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if mouse.row > diff_area.y
                        && mouse.row < diff_area.y + diff_area.height.saturating_sub(1) =>
                {
                    let view_row = usize::from(mouse.row.saturating_sub(diff_area.y + 1));
                    let visible_row_index = self.last_diff_scroll_secondary + view_row;
                    let content_col =
                        usize::from(mouse.column.saturating_sub(diff_area.x.saturating_add(1)));
                    if let Some((path, line)) = self.resolve_file_reference_hit(
                        DiffPane::Secondary,
                        visible_row_index,
                        content_col,
                    ) {
                        self.follow_file_reference(DiffPane::Secondary, &path, line);
                        return Ok(());
                    }
                    if let Some(row_index) = self
                        .last_diff_row_map_secondary
                        .get(visible_row_index)
                        .copied()
                    {
                        self.set_active_line_index(row_index);
                    }
                }
                MouseEventKind::ScrollUp => {
                    self.set_active_line_index(
                        self.active_line_index()
                            .saturating_sub(MOUSE_WHEEL_SCROLL_LINES),
                    );
                }
                MouseEventKind::ScrollDown => {
                    let max = self.current_rows().len().saturating_sub(1);
                    self.set_active_line_index(
                        self.active_line_index()
                            .saturating_add(MOUSE_WHEEL_SCROLL_LINES)
                            .min(max),
                    );
                }
                _ => {}
            }
            self.constrain_selection();
            return Ok(());
        }

        Ok(())
    }

    async fn handle_inline_comment_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            if self.clear_inline_file_mention_picker() {
                self.status_line = "file reference picker closed".into();
                return Ok(());
            }
            self.inline_comment = None;
            self.status_line = "comment box collapsed".into();
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')) {
            if let Err(error) = self.submit_inline_comment(service).await {
                self.status_line = format!("save comment failed: {error}");
            }
            return Ok(());
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('p')) {
            if let Some(inline) = self.inline_comment.as_mut() {
                inline.preview_mode = !inline.preview_mode;
                self.status_line = if inline.preview_mode {
                    "markdown preview enabled".into()
                } else {
                    "markdown preview disabled".into()
                };
            }
            return Ok(());
        }

        let Some(preview_mode) = self
            .inline_comment
            .as_ref()
            .map(|inline| inline.preview_mode)
        else {
            return Ok(());
        };
        if preview_mode {
            return Ok(());
        }

        if self.inline_file_mention_picker_active() {
            match key.code {
                KeyCode::Up => {
                    self.move_inline_file_mention_selection(-1);
                    return Ok(());
                }
                KeyCode::Down => {
                    self.move_inline_file_mention_selection(1);
                    return Ok(());
                }
                KeyCode::PageUp => {
                    self.move_inline_file_mention_selection(
                        -(INLINE_FILE_MENTION_MAX_VISIBLE_ROWS as isize),
                    );
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.move_inline_file_mention_selection(
                        INLINE_FILE_MENTION_MAX_VISIBLE_ROWS as isize,
                    );
                    return Ok(());
                }
                KeyCode::Enter | KeyCode::Tab => {
                    if self.accept_inline_file_mention() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }

        let Some(inline) = self.inline_comment.as_mut() else {
            return Ok(());
        };

        match key.code {
            KeyCode::Left => inline.buffer.move_left(),
            KeyCode::Right => inline.buffer.move_right(),
            KeyCode::Up => inline.buffer.move_up(),
            KeyCode::Down => inline.buffer.move_down(),
            KeyCode::Home => inline.buffer.move_home(),
            KeyCode::End => inline.buffer.move_end(),
            KeyCode::Enter => inline.buffer.insert_newline(),
            KeyCode::Tab => inline.buffer.insert_spaces(4),
            KeyCode::Backspace => inline.buffer.backspace(),
            KeyCode::Delete => inline.buffer.delete_char(),
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                inline.buffer.insert_char(ch);
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_home();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_end();
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.kill_to_end();
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_up();
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_down();
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_left();
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                inline.buffer.move_right();
            }
            _ => {}
        }
        self.refresh_inline_file_mention_picker();
        Ok(())
    }

    fn inline_file_mention_picker_active(&self) -> bool {
        self.inline_comment
            .as_ref()
            .and_then(|inline| inline.file_mention.as_ref())
            .is_some()
    }

    fn clear_inline_file_mention_picker(&mut self) -> bool {
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline.file_mention.take().is_some()
    }

    fn move_inline_file_mention_selection(&mut self, delta: isize) {
        let Some(inline) = self.inline_comment.as_mut() else {
            return;
        };
        let Some(mention) = inline.file_mention.as_mut() else {
            return;
        };
        if mention.candidates.is_empty() {
            return;
        }

        let max_index = mention.candidates.len().saturating_sub(1);
        let next = (mention.selected_index as isize + delta).clamp(0, max_index as isize) as usize;
        mention.selected_index = next;

        if mention.selected_index < mention.scroll {
            mention.scroll = mention.selected_index;
        } else if mention.selected_index
            >= mention
                .scroll
                .saturating_add(INLINE_FILE_MENTION_MAX_VISIBLE_ROWS)
        {
            mention.scroll = mention
                .selected_index
                .saturating_sub(INLINE_FILE_MENTION_MAX_VISIBLE_ROWS.saturating_sub(1));
        }
    }

    fn accept_inline_file_mention(&mut self) -> bool {
        let Some((replacement_path, replace_start, replace_end, line_suffix)) = self
            .inline_comment
            .as_ref()
            .and_then(|inline| inline.file_mention.as_ref())
            .and_then(|mention| {
                mention.candidates.get(mention.selected_index).map(|path| {
                    (
                        path.clone(),
                        mention.replace_start_col,
                        mention.replace_end_col,
                        mention.line_suffix.clone(),
                    )
                })
            })
        else {
            return false;
        };

        let mut replacement = format!("@{replacement_path}");
        let inferred_line = self.default_inline_reference_line_for_path(&replacement_path);
        let resolved_line = line_suffix
            .and_then(|suffix| {
                let trimmed = suffix.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .or_else(|| inferred_line.map(|line| line.to_string()));
        if let Some(line) = resolved_line {
            replacement.push(':');
            replacement.push_str(&line);
        }

        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline
            .buffer
            .replace_range_on_cursor_line(replace_start, replace_end, &replacement);
        inline.file_mention = None;
        self.status_line = format!("inserted file reference: {replacement_path}");
        true
    }

    fn default_inline_reference_line_for_path(&self, path: &str) -> Option<u32> {
        let file = self.current_file()?;
        if file.path != path {
            return None;
        }
        self.current_rows()
            .get(self.active_line_index())
            .and_then(|row| row.new_line.or(row.old_line))
    }

    fn refresh_inline_file_mention_picker(&mut self) {
        let Some(inline) = self.inline_comment.as_ref() else {
            return;
        };
        let line = inline
            .buffer
            .lines
            .get(inline.buffer.cursor_line)
            .cloned()
            .unwrap_or_default();
        let cursor_col = inline.buffer.cursor_col;
        let previous_selection = inline
            .file_mention
            .as_ref()
            .and_then(|mention| mention.candidates.get(mention.selected_index))
            .cloned();
        let previous_scroll = inline
            .file_mention
            .as_ref()
            .map(|mention| mention.scroll)
            .unwrap_or(0);

        let Some(context) = parse_inline_file_mention_context(&line, cursor_col) else {
            let _ = self.clear_inline_file_mention_picker();
            return;
        };

        let mut candidates = self.inline_file_mention_candidates(&context.path_query);
        if candidates.len() > INLINE_FILE_MENTION_MAX_CANDIDATES {
            candidates.truncate(INLINE_FILE_MENTION_MAX_CANDIDATES);
        }

        let mut selected_index = 0usize;
        if !candidates.is_empty()
            && let Some(previous) = previous_selection
            && let Some(idx) = candidates.iter().position(|path| *path == previous)
        {
            selected_index = idx;
        }

        let mut scroll = previous_scroll.min(selected_index);
        if selected_index >= scroll.saturating_add(INLINE_FILE_MENTION_MAX_VISIBLE_ROWS) {
            scroll = selected_index.saturating_sub(INLINE_FILE_MENTION_MAX_VISIBLE_ROWS - 1);
        }

        let Some(inline) = self.inline_comment.as_mut() else {
            return;
        };
        inline.file_mention = Some(InlineFileMentionState {
            replace_start_col: context.replace_start_col,
            replace_end_col: context.replace_end_col,
            path_query: context.path_query,
            line_suffix: context.line_suffix,
            candidates,
            selected_index,
            scroll,
        });
    }

    fn inline_file_mention_candidates(&self, query: &str) -> Vec<String> {
        let query = query.trim().to_ascii_lowercase();
        let mut ranked = Vec::new();
        for file in &self.diff.files {
            let path = file.path.clone();
            let path_lower = path.to_ascii_lowercase();
            let Some((rank, tie_breaker)) = inline_file_mention_rank(&path_lower, &query) else {
                continue;
            };
            ranked.push((rank, tie_breaker, path.len(), path));
        }
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
                .then_with(|| left.3.cmp(&right.3))
        });

        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (_, _, _, path) in ranked {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
        out
    }

    fn comment_target_for_row(&self, row_index: usize) -> Option<CommentTarget> {
        let file = self.current_file()?;
        let row = self.current_rows().get(row_index)?.clone();

        let (side, old_line, new_line) = match row.kind {
            DiffLineKind::Added => (DiffSide::Right, None, row.new_line),
            DiffLineKind::Removed => (DiffSide::Left, row.old_line, None),
            DiffLineKind::Context => (DiffSide::Right, row.old_line, row.new_line),
            _ => return None,
        };

        Some(CommentTarget {
            side,
            old_line,
            new_line,
            file_path: file.path.clone(),
        })
    }

    fn toggle_inline_comment_for_selected_line(&mut self) {
        self.toggle_inline_comment_for_row(self.active_line_index());
    }

    fn toggle_inline_comment_for_row(&mut self, row_index: usize) {
        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index == row_index
            && matches!(inline.mode, InlineDraftMode::Comment(_))
        {
            self.inline_comment = None;
            self.status_line = "comment box collapsed".into();
            return;
        }

        let Some(target) = self.comment_target_for_row(row_index) else {
            self.inline_comment = None;
            self.status_line = "selected line cannot receive comments".into();
            return;
        };

        self.inline_comment = Some(InlineCommentState {
            row_index,
            mode: InlineDraftMode::Comment(target),
            buffer: TextBuffer::new(),
            preview_mode: false,
            file_mention: None,
        });
        self.status_line = "comment box expanded".into();
    }

    fn start_inline_reply_for_selected_comment(&mut self) {
        let Some(target) = self
            .reply_target_for_selected_line()
            .or_else(|| self.reply_target_for_selected_thread())
        else {
            self.status_line = "no comment on selected line".into();
            return;
        };
        self.selected_comment = target.selected_comment_index;
        self.focus_selected_comment_line();
        self.request_scroll_to_thread_tail(self.active_diff_pane, self.active_line_index());
        let selected_comment_id = target.comment_id;
        let old_line = target.old_line;
        let new_line = target.new_line;

        if let Some(inline) = self.inline_comment.as_ref()
            && matches!(
                inline.mode,
                InlineDraftMode::Reply {
                    comment_id,
                    ..
                } if comment_id == selected_comment_id
            )
        {
            self.inline_comment = None;
            self.status_line = "reply box collapsed".into();
            return;
        }

        self.inline_comment = Some(InlineCommentState {
            row_index: self.active_line_index(),
            mode: InlineDraftMode::Reply {
                comment_id: selected_comment_id,
                old_line,
                new_line,
            },
            buffer: TextBuffer::new(),
            preview_mode: false,
            file_mention: None,
        });
        self.status_line = format!("reply box opened for comment #{selected_comment_id}");
    }

    fn reply_target_for_selected_line(&self) -> Option<ReplyTarget> {
        let row = self.current_rows().get(self.active_line_index())?;
        let comments = self.comments_for_selected_file();
        let matches: Vec<(usize, &LineComment)> = comments
            .into_iter()
            .enumerate()
            .filter(|(_, comment)| comment_matches_display_row(comment, row))
            .collect();
        if matches.is_empty() {
            return None;
        }

        let selected = if let Some(selected) = matches
            .iter()
            .find(|(idx, _)| *idx == self.selected_comment)
            .copied()
        {
            selected
        } else {
            matches.last().copied()?
        };

        Some(ReplyTarget {
            selected_comment_index: selected.0,
            comment_id: selected.1.id,
            old_line: selected.1.old_line,
            new_line: selected.1.new_line,
        })
    }

    fn reply_target_for_selected_thread(&self) -> Option<ReplyTarget> {
        let comment = self.selected_comment_details()?;
        Some(ReplyTarget {
            selected_comment_index: self.selected_comment,
            comment_id: comment.id,
            old_line: comment.old_line,
            new_line: comment.new_line,
        })
    }

    async fn submit_inline_comment(&mut self, service: &ReviewService) -> Result<()> {
        let Some(inline) = self.inline_comment.take() else {
            return Ok(());
        };

        if inline.buffer.is_blank() {
            self.status_line = "comment body cannot be empty".into();
            self.inline_comment = Some(inline);
            return Ok(());
        }

        let body = inline.buffer.to_text();

        match inline.mode {
            InlineDraftMode::Comment(target) => {
                service
                    .add_comment(
                        &self.review_name,
                        AddCommentInput {
                            file_path: target.file_path,
                            old_line: target.old_line,
                            new_line: target.new_line,
                            side: target.side,
                            body,
                            author: Author::User,
                        },
                    )
                    .await
                    .context("failed to save comment")?;
                self.status_line = "comment saved".into();
            }
            InlineDraftMode::Reply {
                comment_id,
                old_line,
                new_line,
            } => {
                service
                    .add_reply(
                        &self.review_name,
                        AddReplyInput {
                            comment_id,
                            author: Author::User,
                            body,
                        },
                    )
                    .await
                    .context("failed to save reply")?;
                self.status_line = format!(
                    "reply saved on #{} at {}:{}",
                    comment_id,
                    old_line
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "_".to_string()),
                    new_line
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "_".to_string())
                );
            }
        }
        self.reload_review(service).await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct InlineFileMentionContext {
    replace_start_col: usize,
    replace_end_col: usize,
    path_query: String,
    line_suffix: Option<String>,
}

fn parse_inline_file_mention_context(
    line: &str,
    cursor_col: usize,
) -> Option<InlineFileMentionContext> {
    let chars: Vec<char> = line.chars().collect();
    let cursor = cursor_col.min(chars.len());

    let mut scan = cursor;
    let mut at_pos = None;
    while scan > 0 {
        let ch = chars[scan - 1];
        if ch == '@' {
            at_pos = Some(scan - 1);
            break;
        }
        if ch.is_whitespace() || !is_inline_file_token_char(ch) {
            break;
        }
        scan -= 1;
    }
    let at_pos = at_pos?;
    if at_pos > 0 && is_inline_file_identifier_char(chars[at_pos - 1]) {
        return None;
    }

    let mut end_col = at_pos + 1;
    while end_col < chars.len() && is_inline_file_path_char(chars[end_col]) {
        end_col += 1;
    }

    let mut line_suffix = None;
    if end_col < chars.len() && chars[end_col] == ':' {
        let digits_start = end_col + 1;
        end_col += 1;
        while end_col < chars.len() && chars[end_col].is_ascii_digit() {
            end_col += 1;
        }
        line_suffix = Some(chars[digits_start..end_col].iter().collect());
    }

    if cursor < at_pos + 1 || cursor > end_col {
        return None;
    }

    let colon_pos = chars[at_pos + 1..end_col]
        .iter()
        .position(|ch| *ch == ':')
        .map(|offset| at_pos + 1 + offset);
    let path_query_end = colon_pos.map_or(cursor, |pos| cursor.min(pos));
    let path_query: String = chars[at_pos + 1..path_query_end].iter().collect();

    Some(InlineFileMentionContext {
        replace_start_col: at_pos,
        replace_end_col: end_col,
        path_query,
        line_suffix,
    })
}

fn is_inline_file_path_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
}

fn is_inline_file_token_char(ch: char) -> bool {
    is_inline_file_path_char(ch) || ch == ':'
}

fn is_inline_file_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
}

fn inline_file_mention_rank(path: &str, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }
    if path.starts_with(query) {
        return Some((0, 0));
    }
    if let Some(position) = path.find(query) {
        return Some((1, position));
    }
    inline_subsequence_penalty(path, query).map(|penalty| (2, penalty))
}

fn inline_subsequence_penalty(path: &str, query: &str) -> Option<usize> {
    let path_chars: Vec<char> = path.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    if query_chars.is_empty() {
        return Some(0);
    }

    let mut next_start = 0usize;
    let mut penalty = 0usize;
    let mut last_index = None;

    for needle in query_chars {
        let mut found = None;
        for (index, candidate) in path_chars.iter().enumerate().skip(next_start) {
            if *candidate == needle {
                found = Some(index);
                break;
            }
        }
        let index = found?;
        penalty += if let Some(previous) = last_index {
            index.saturating_sub(previous + 1)
        } else {
            index
        };
        last_index = Some(index);
        next_start = index + 1;
    }

    Some(penalty)
}

fn format_unresolved_ids(ids: &[u64]) -> String {
    const LIMIT: usize = 8;
    let mut visible = ids
        .iter()
        .take(LIMIT)
        .map(u64::to_string)
        .collect::<Vec<_>>();
    if ids.len() > LIMIT {
        visible.push(format!("+{}", ids.len() - LIMIT));
    }
    visible.join(",")
}
