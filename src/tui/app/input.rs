use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

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
    CommandPromptMode, CommandPromptState, CommentTarget, DiffPane, InlineCommentState,
    InlineDraftMode, MOUSE_WHEEL_FILE_SCROLL_FILES, MOUSE_WHEEL_SCROLL_LINES, PendingUiAction,
    ReplyTarget, TextBuffer, ThreadAnchor, TuiApp, comment_matches_display_row,
    format_line_reference, insert_char_at, point_in_rect, remove_char_at,
};

impl TuiApp {
    pub(super) async fn handle_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if self.shortcuts_modal_visible {
            return self.handle_shortcuts_modal_key(key);
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

        self.handle_normal_key(key, service).await
    }

    fn handle_shortcuts_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::F(1) => {
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

    async fn handle_normal_key(&mut self, key: KeyEvent, service: &ReviewService) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::F(1) | KeyCode::Char('?') => {
                self.shortcuts_modal_visible = true;
                self.shortcuts_modal_scroll = 0;
                self.status_line = "shortcuts help opened".into();
            }
            KeyCode::Char('z') => {
                self.content_fullscreen = !self.content_fullscreen;
                if self.content_fullscreen {
                    self.status_line = "content fullscreen enabled".into();
                } else {
                    self.status_line = "content fullscreen disabled".into();
                }
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
            KeyCode::Char('u') => {
                self.open_user_name_editor();
            }
            KeyCode::Char('v') => {
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
                if let Err(error) = self.cycle_theme(service).await {
                    self.status_line = format!("theme change failed: {error}");
                }
            }
            KeyCode::Char('T') => {
                if let Err(error) = self.toggle_light_dark_theme(service).await {
                    self.status_line = format!("theme variant toggle failed: {error}");
                }
            }
            KeyCode::Char(']') => {
                let max = self.comments_for_selected_file().len().saturating_sub(1);
                self.selected_comment = (self.selected_comment + 1).min(max);
            }
            KeyCode::Char('[') => {
                self.selected_comment = self.selected_comment.saturating_sub(1);
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
                if let Err(error) = self.set_state(service, ReviewState::Pending).await {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('w') => {
                if let Err(error) = self
                    .set_state(service, ReviewState::WaitingForResponse)
                    .await
                {
                    self.status_line = error.to_string();
                }
            }
            KeyCode::Char('d') => {
                if let Err(error) = self.set_state(service, ReviewState::Done).await {
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
        } else {
            if self.current_rows_contain_query(&query) {
                self.status_line = format!("no further match for: {query}");
            } else {
                self.search_query = None;
                self.status_line = format!("search cleared (no matches): {query}");
            }
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
        self.status_line = format!(
            "thread #{} at line {}",
            target.comment_id,
            format_line_reference(target.old_line, target.new_line)
        );
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

        if self.settings_editor.is_some() || self.command_prompt.is_some() {
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
                    let row = self.last_file_scroll
                        + usize::from(mouse.row.saturating_sub(file_area.y + 1));
                    self.select_file(row);
                    if self.active_file_index() < self.diff.files.len() {
                        self.status_line = format!(
                            "selected file {}",
                            self.diff.files[self.active_file_index()].path
                        );
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
                    if let Some(row_index) = self.last_diff_row_map.get(visible_row_index).copied()
                    {
                        self.set_active_line_index(row_index);
                        self.open_inline_comment_for_row(row_index);
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
                    if let Some(row_index) = self
                        .last_diff_row_map_secondary
                        .get(visible_row_index)
                        .copied()
                    {
                        self.set_active_line_index(row_index);
                        self.open_inline_comment_for_row(row_index);
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

        let Some(inline) = self.inline_comment.as_mut() else {
            return Ok(());
        };

        if inline.preview_mode {
            return Ok(());
        }

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
        Ok(())
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
        });
        self.status_line = "comment box expanded".into();
    }

    fn open_inline_comment_for_row(&mut self, row_index: usize) {
        let already_open_on_row = self
            .inline_comment
            .as_ref()
            .map(|inline| {
                inline.row_index == row_index && matches!(inline.mode, InlineDraftMode::Comment(_))
            })
            .unwrap_or(false);
        if already_open_on_row {
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
        });
        self.status_line = "comment box expanded".into();
    }

    fn start_inline_reply_for_selected_comment(&mut self) {
        let Some(target) = self.reply_target_for_selected_line() else {
            self.status_line = "no comment on selected line".into();
            return;
        };
        self.selected_comment = target.selected_comment_index;
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
        });
        self.status_line = format!("reply box opened for comment #{}", selected_comment_id);
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
