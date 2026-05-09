use crate::tui::app::TextBuffer;
use crate::tui::app::helpers::slice_chars;

impl TextBuffer {
    pub(crate) fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
        }
    }

    pub(crate) fn char_len(&self) -> usize {
        let text_chars: usize = self.lines.iter().map(|line| line.chars().count()).sum();
        text_chars + self.lines.len().saturating_sub(1)
    }

    pub(crate) fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.line_len(self.cursor_line);
        }
    }

    pub(crate) fn move_right(&mut self) {
        let line_len = self.line_len(self.cursor_line);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    pub(crate) fn move_word_left(&mut self) {
        let target = self.previous_word_boundary();
        self.set_cursor_absolute_index(target);
    }

    pub(crate) fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
        }
    }

    pub(crate) fn move_down(&mut self) {
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
        }
    }

    pub(crate) fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor_col = self.line_len(self.cursor_line);
    }

    pub(crate) fn insert_char(&mut self, ch: char) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        chars.insert(self.cursor_col, ch);
        self.lines[self.cursor_line] = chars.into_iter().collect();
        self.cursor_col += 1;
    }

    pub(crate) fn insert_spaces(&mut self, count: usize) {
        for _ in 0..count {
            self.insert_char(' ');
        }
    }

    pub(crate) fn insert_newline(&mut self) {
        let current = self.lines[self.cursor_line].clone();
        let left = slice_chars(&current, 0, self.cursor_col);
        let right = slice_chars(
            &current,
            self.cursor_col,
            current.chars().count().saturating_sub(self.cursor_col),
        );
        self.lines[self.cursor_line] = left;
        self.lines.insert(self.cursor_line + 1, right);
        self.cursor_line += 1;
        self.cursor_col = 0;
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
            let remove_at = self.cursor_col - 1;
            if remove_at < chars.len() {
                chars.remove(remove_at);
                self.lines[self.cursor_line] = chars.into_iter().collect();
                self.cursor_col -= 1;
            }
            return;
        }

        if self.cursor_line > 0 {
            let current = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            let previous_len = self.line_len(self.cursor_line);
            self.lines[self.cursor_line].push_str(&current);
            self.cursor_col = previous_len;
        }
    }

    pub(crate) fn delete_char(&mut self) {
        let line_len = self.line_len(self.cursor_line);
        if self.cursor_col < line_len {
            let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
            chars.remove(self.cursor_col);
            self.lines[self.cursor_line] = chars.into_iter().collect();
            return;
        }

        if self.cursor_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
        }
    }

    pub(crate) fn replace_range_on_cursor_line(
        &mut self,
        start_col: usize,
        end_col: usize,
        replacement: &str,
    ) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        let start = start_col.min(chars.len());
        let end = end_col.min(chars.len()).max(start);
        let replacement_chars: Vec<char> = replacement.chars().collect();
        let replacement_len = replacement_chars.len();
        chars.splice(start..end, replacement_chars);
        self.lines[self.cursor_line] = chars.into_iter().collect();
        self.cursor_col = start + replacement_len;
    }

    pub(crate) fn kill_to_end(&mut self) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        chars.truncate(self.cursor_col);
        self.lines[self.cursor_line] = chars.into_iter().collect();
    }

    pub(crate) fn delete_word_right(&mut self) {
        let start = self.cursor_absolute_index();
        let end = self.next_word_boundary();
        if end <= start {
            return;
        }

        let mut chars: Vec<char> = self.to_text().chars().collect();
        chars.drain(start..end);
        let text: String = chars.into_iter().collect();
        self.replace_text_and_cursor(text, start);
    }

    pub(crate) fn line_len(&self, idx: usize) -> usize {
        self.lines[idx].chars().count()
    }

    fn cursor_absolute_index(&self) -> usize {
        let prior_lines_len: usize = self.lines[..self.cursor_line]
            .iter()
            .map(|line| line.chars().count() + 1)
            .sum();
        prior_lines_len + self.cursor_col
    }

    fn set_cursor_absolute_index(&mut self, target: usize) {
        let mut remaining = target.min(self.char_len());
        for (line_idx, line) in self.lines.iter().enumerate() {
            let line_len = line.chars().count();
            if remaining <= line_len {
                self.cursor_line = line_idx;
                self.cursor_col = remaining;
                return;
            }

            if line_idx + 1 == self.lines.len() {
                self.cursor_line = line_idx;
                self.cursor_col = line_len;
                return;
            }

            remaining = remaining.saturating_sub(line_len + 1);
        }

        self.cursor_line = 0;
        self.cursor_col = 0;
    }

    fn replace_text_and_cursor(&mut self, text: String, cursor_abs: usize) {
        self.lines = text.split('\n').map(ToString::to_string).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.set_cursor_absolute_index(cursor_abs);
    }

    fn previous_word_boundary(&self) -> usize {
        let chars: Vec<char> = self.to_text().chars().collect();
        let mut index = self.cursor_absolute_index().min(chars.len());
        while index > 0 && chars[index - 1].is_whitespace() {
            index -= 1;
        }
        while index > 0 && !chars[index - 1].is_whitespace() {
            index -= 1;
        }
        index
    }

    fn next_word_boundary(&self) -> usize {
        let chars: Vec<char> = self.to_text().chars().collect();
        let mut index = self.cursor_absolute_index().min(chars.len());
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        while index < chars.len() && !chars[index].is_whitespace() {
            index += 1;
        }
        index
    }
}
