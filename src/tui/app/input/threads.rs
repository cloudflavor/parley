use super::*;

impl TuiApp {
    pub(super) fn handle_thread_selector_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc)
            || (matches!(key.code, KeyCode::Char('t' | 'T'))
                && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.thread_selector = None;
            self.status_line = "thread selector closed".into();
            return Ok(());
        }

        let entries = self.filtered_thread_selector_entries();
        if matches!(key.code, KeyCode::Enter) {
            let selected_entry = self
                .thread_selector
                .as_ref()
                .and_then(|selector| entries.get(selector.selected_index))
                .cloned();
            if let Some(entry) = selected_entry {
                self.jump_to_thread_selector_entry(&entry);
            }
            return Ok(());
        }

        let Some(selector) = self.thread_selector.as_mut() else {
            return Ok(());
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                selector.selected_index = selector.selected_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                selector.selected_index =
                    (selector.selected_index + 1).min(entries.len().saturating_sub(1));
            }
            KeyCode::PageUp => {
                selector.selected_index = selector.selected_index.saturating_sub(8);
            }
            KeyCode::PageDown => {
                selector.selected_index =
                    (selector.selected_index + 8).min(entries.len().saturating_sub(1));
            }
            KeyCode::Home => {
                selector.selected_index = 0;
            }
            KeyCode::End => {
                selector.selected_index = entries.len().saturating_sub(1);
            }
            KeyCode::Left => {
                selector.cursor_col = selector.cursor_col.saturating_sub(1);
            }
            KeyCode::Right => {
                selector.cursor_col = (selector.cursor_col + 1).min(selector.query.chars().count());
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                selector.cursor_col = 0;
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                selector.cursor_col = selector.query.chars().count();
            }
            KeyCode::Backspace if selector.cursor_col > 0 => {
                remove_char_at(&mut selector.query, selector.cursor_col - 1);
                selector.cursor_col -= 1;
                selector.selected_index = 0;
                selector.scroll = 0;
            }
            KeyCode::Delete if selector.cursor_col < selector.query.chars().count() => {
                remove_char_at(&mut selector.query, selector.cursor_col);
                selector.selected_index = 0;
                selector.scroll = 0;
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                insert_char_at(&mut selector.query, selector.cursor_col, ch);
                selector.cursor_col += 1;
                selector.selected_index = 0;
                selector.scroll = 0;
            }
            _ => {}
        }

        let refreshed_entries = self.filtered_thread_selector_entries();
        if refreshed_entries.is_empty() {
            if let Some(selector) = self.thread_selector.as_mut() {
                selector.selected_index = 0;
                selector.scroll = 0;
            }
        } else if let Some(selector) = self.thread_selector.as_mut() {
            selector.selected_index = selector
                .selected_index
                .min(refreshed_entries.len().saturating_sub(1));
        }
        Ok(())
    }

    pub(super) fn jump_thread(&mut self, forward: bool) {
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
                    .position(|row| self.comment_matches_for_navigation(comment, row))
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
                .unwrap_or_else(|| *anchors.last().unwrap_or(&anchors[0]))
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
}
