use super::*;

impl TuiApp {
    pub(super) fn handle_file_search_key(&mut self, key: KeyEvent) -> Result<()> {
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

    pub(super) fn handle_command_prompt_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            if let Some(prompt) = self.command_prompt.take() {
                let _ = prompt;
                self.status_line = "command cancelled".into();
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

    pub(super) fn goto_line_number(&mut self, target: u32) -> bool {
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

    pub(super) fn jump_search(&mut self, forward: bool) {
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
}
