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
    services::review_service::{
        AddCommentInput, AddReplyInput, ReanchorCommentInput, ReviewService,
    },
};

use super::{
    CommandPaletteAction, CommandPaletteItem, CommandPaletteState, CommandPromptMode,
    CommentTarget, DiffPane, INLINE_FILE_MENTION_MAX_CANDIDATES,
    INLINE_FILE_MENTION_MAX_VISIBLE_ROWS, InlineCommentState, InlineDraftMode,
    InlineFileMentionState, MOUSE_WHEEL_FILE_SCROLL_FILES, MOUSE_WHEEL_SCROLL_LINES,
    PendingUiAction, ReplyTarget, TextBuffer, ThreadAnchor, TuiApp, comment_matches_display_row,
    format_line_reference, insert_char_at, point_in_rect, remove_char_at,
};

mod command_palette;
mod inline_comment;

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

        if self.inline_file_reference_picker_active() {
            self.handle_inline_file_reference_picker_mouse(mouse);
            self.constrain_selection();
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

    fn handle_inline_file_reference_picker_mouse(&mut self, mouse: MouseEvent) {
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
                        let _ = self.accept_inline_file_reference_line_selection();
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
            return;
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
                        let _ = self.accept_inline_file_reference_line_selection();
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
        }
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::{
        config::AppConfig,
        diff::{DiffDocument, DiffFile, DiffHunk, DiffLine, DiffLineKind},
        review::{LineAnchorSnapshot, ReviewSession, ReviewState},
    };
    use crate::persistence::store::Store;
    use crate::tui::app::{InlineFileReferencePickerState, TuiAppInit};
    use crate::tui::theme::load_themes;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn opening_command_palette_hides_ai_progress_popup() {
        let mut app = make_test_app(vec!["src/a.rs"]);
        app.ai_progress_visible = true;

        app.open_command_palette();

        assert!(app.command_palette.is_some());
        assert!(!app.ai_progress_visible);
    }

    #[test]
    fn selecting_file_reference_opens_line_picker_in_diff_viewer() {
        let mut app = make_test_app_with_files(vec![
            empty_diff_file("src/a.rs"),
            diff_file_with_lines(
                "src/target.rs",
                &[(10, "fn ten() {}"), (11, "fn eleven() {}")],
            ),
        ]);
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(1),
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
            }),
            buffer: text_buffer_with_line("@src/target.rs"),
            preview_mode: false,
            file_mention: Some(InlineFileMentionState {
                replace_start_col: 0,
                replace_end_col: "@src/target.rs".chars().count(),
                path_query: "src/target.rs".into(),
                line_suffix: None,
                candidates: vec!["src/target.rs".into()],
                selected_index: 0,
                scroll: 0,
            }),
            file_reference_picker: None,
        });

        assert!(app.begin_inline_file_reference_line_picker());
        assert_eq!(app.active_file_index(), 1);
        assert_eq!(app.current_inline_reference_line_number(), Some(10));
        assert_eq!(
            app.inline_comment
                .as_ref()
                .expect("inline comment should exist")
                .buffer
                .to_text(),
            "@src/target.rs"
        );
        assert!(
            app.inline_comment
                .as_ref()
                .and_then(|inline| inline.file_reference_picker.as_ref())
                .is_some()
        );
    }

    #[test]
    fn accepting_file_reference_line_selection_inserts_line_number() {
        let mut app = make_test_app_with_files(vec![diff_file_with_lines(
            "src/target.rs",
            &[(10, "fn ten() {}"), (11, "fn eleven() {}")],
        )]);
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(10),
                file_path: "src/target.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
            }),
            buffer: text_buffer_with_line("@src/target.rs"),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: Some(InlineFileReferencePickerState {
                path: "src/target.rs".into(),
                replace_start_col: 0,
                replace_end_col: "@src/target.rs".chars().count(),
                origin_pane: DiffPane::Primary,
                origin_file_index: 0,
                origin_row_index: 0,
            }),
        });
        app.ensure_row_cache();
        assert!(app.goto_line_number(11));

        assert!(app.accept_inline_file_reference_line_selection());

        let inline = app
            .inline_comment
            .as_ref()
            .expect("inline comment should exist");
        assert_eq!(inline.buffer.to_text(), "@src/target.rs:11");
        assert!(inline.file_reference_picker.is_none());
    }

    #[tokio::test]
    async fn alt_b_moves_backward_by_word_in_inline_comment_editor() {
        let mut app = make_test_app(vec!["src/a.rs"]);
        let service = make_test_service();
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(1),
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
            }),
            buffer: text_buffer_with_line("alpha  beta"),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            &service,
        )
        .await
        .expect("alt+b should be handled");

        let inline = app
            .inline_comment
            .as_ref()
            .expect("inline comment should exist");
        assert_eq!(inline.buffer.cursor_line, 0);
        assert_eq!(inline.buffer.cursor_col, "alpha  ".chars().count());
    }

    #[tokio::test]
    async fn alt_d_deletes_forward_word_in_inline_comment_editor() {
        let mut app = make_test_app(vec!["src/a.rs"]);
        let service = make_test_service();
        app.inline_comment = Some(InlineCommentState {
            row_index: 0,
            mode: InlineDraftMode::Comment(CommentTarget {
                side: DiffSide::Right,
                old_line: None,
                new_line: Some(1),
                file_path: "src/a.rs".into(),
                line_anchor: LineAnchorSnapshot::default(),
            }),
            buffer: TextBuffer {
                lines: vec!["alpha".into(), "beta gamma".into()],
                cursor_line: 0,
                cursor_col: "alpha".chars().count(),
            },
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });

        app.handle_inline_comment_key(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT),
            &service,
        )
        .await
        .expect("alt+d should be handled");

        let inline = app
            .inline_comment
            .as_ref()
            .expect("inline comment should exist");
        assert_eq!(inline.buffer.lines, vec!["alpha gamma"]);
        assert_eq!(inline.buffer.cursor_line, 0);
        assert_eq!(inline.buffer.cursor_col, "alpha".chars().count());
    }

    #[tokio::test]
    async fn ctrl_z_queues_suspend_action() {
        let mut app = make_test_app(vec!["src/a.rs"]);
        let service = make_test_service();

        app.handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
            &service,
        )
        .await
        .expect("ctrl+z should be handled");

        assert!(matches!(
            app.pending_action,
            Some(PendingUiAction::SuspendTuiProcess)
        ));
        assert_eq!(app.status_line, "suspending parley; run `fg` to resume");
    }

    #[tokio::test]
    async fn pressing_u_reanchors_selected_thread_and_persists_review() {
        let tempdir = tempdir().expect("tempdir should exist");
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        service
            .create_review("test-review")
            .await
            .expect("review should be created");
        service
            .add_comment(
                "test-review",
                AddCommentInput {
                    file_path: "src/a.rs".into(),
                    old_line: Some(10),
                    new_line: Some(10),
                    side: DiffSide::Right,
                    line_anchor: None,
                    body: "anchor me".into(),
                    author: Author::User,
                },
            )
            .await
            .expect("comment should be added");
        let review = service
            .load_review("test-review")
            .await
            .expect("review should load");

        let mut app = make_test_app_with_review_and_files(
            review,
            vec![diff_file_with_lines(
                "src/a.rs",
                &[(10, "fn old_anchor() {}"), (12, "fn new_anchor() {}")],
            )],
        );
        app.ensure_row_cache();
        assert!(app.goto_line_number(12));
        app.selected_comment = 0;

        app.handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::empty()),
            &service,
        )
        .await
        .expect("re-anchor key should succeed");

        let updated = service
            .load_review("test-review")
            .await
            .expect("updated review should load");
        let comment = updated
            .comments
            .iter()
            .find(|comment| comment.id == 1)
            .expect("comment should exist");
        assert_eq!(comment.old_line, Some(12));
        assert_eq!(comment.new_line, Some(12));
        assert!(!comment.detached);
        assert!(comment.line_anchor.is_some());
        assert!(app.status_line.contains("re-anchored"));
    }

    fn make_test_app(paths: Vec<&str>) -> TuiApp {
        make_test_app_with_files(paths.into_iter().map(empty_diff_file).collect())
    }

    fn make_test_app_with_files(files: Vec<DiffFile>) -> TuiApp {
        let review = ReviewSession {
            name: "test-review".to_string(),
            state: ReviewState::Open,
            created_at_ms: 0,
            updated_at_ms: 0,
            done_at_ms: None,
            comments: Vec::new(),
            next_comment_id: 1,
            next_reply_id: 1,
        };
        make_test_app_with_review_and_files(review, files)
    }

    fn make_test_app_with_review_and_files(review: ReviewSession, files: Vec<DiffFile>) -> TuiApp {
        let review = ReviewSession {
            name: review.name,
            state: review.state,
            created_at_ms: review.created_at_ms,
            updated_at_ms: review.updated_at_ms,
            done_at_ms: review.done_at_ms,
            comments: review.comments,
            next_comment_id: review.next_comment_id,
            next_reply_id: review.next_reply_id,
        };
        let diff = DiffDocument { files };
        let themes = load_themes().expect("embedded themes should load");
        TuiApp::new(TuiAppInit {
            review_name: review.name.clone(),
            review,
            diff,
            diff_source: crate::git::diff::DiffSource::WorkingTree,
            config: AppConfig::default(),
            themes,
            theme_index: 0,
            log_path: PathBuf::from("test.log"),
        })
    }

    fn empty_diff_file(path: &str) -> DiffFile {
        DiffFile {
            path: path.to_string(),
            header_lines: Vec::new(),
            hunks: Vec::new(),
        }
    }

    fn diff_file_with_lines(path: &str, lines: &[(u32, &str)]) -> DiffFile {
        let mut hunk_lines = vec![DiffLine {
            kind: DiffLineKind::HunkHeader,
            old_line: None,
            new_line: None,
            raw: "@@ -1,1 +1,1 @@".into(),
            code: "@@ -1,1 +1,1 @@".into(),
        }];
        hunk_lines.extend(lines.iter().map(|(line, code)| DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(*line),
            new_line: Some(*line),
            raw: format!(" {code}"),
            code: (*code).to_string(),
        }));
        DiffFile {
            path: path.to_string(),
            header_lines: Vec::new(),
            hunks: vec![DiffHunk {
                old_start: lines.first().map(|(line, _)| *line).unwrap_or(1),
                old_count: lines.len() as u32,
                new_start: lines.first().map(|(line, _)| *line).unwrap_or(1),
                new_count: lines.len() as u32,
                header: "@@ -1,1 +1,1 @@".into(),
                lines: hunk_lines,
            }],
        }
    }

    fn text_buffer_with_line(line: &str) -> TextBuffer {
        TextBuffer {
            lines: vec![line.to_string()],
            cursor_line: 0,
            cursor_col: line.chars().count(),
        }
    }

    fn make_test_service() -> ReviewService {
        let tempdir = tempdir().expect("tempdir should exist");
        ReviewService::new(Store::from_project_root(tempdir.path()))
    }
}
