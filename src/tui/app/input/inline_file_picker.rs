use super::*;
use crate::utils::cast::offset_index;

impl TuiApp {
    pub(super) fn inline_file_mention_picker_active(&self) -> bool {
        self.inline_comment
            .as_ref()
            .and_then(|inline| inline.file_mention.as_ref())
            .is_some()
    }

    pub(super) fn inline_file_reference_picker_active(&self) -> bool {
        self.inline_comment
            .as_ref()
            .and_then(|inline| inline.file_reference_picker.as_ref())
            .is_some()
    }

    pub(super) fn clear_inline_file_mention_picker(&mut self) -> bool {
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline.file_mention.take().is_some()
    }

    pub(super) fn clear_inline_file_reference_picker(&mut self) -> bool {
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline.file_reference_picker.take().is_some()
    }

    pub(super) fn move_inline_file_mention_selection(&mut self, delta: isize) {
        let Some(inline) = self.inline_comment.as_mut() else {
            return;
        };
        let Some(mention) = inline.file_mention.as_mut() else {
            return;
        };
        if mention.candidates.is_empty() {
            return;
        }

        mention.selected_index =
            offset_index(mention.selected_index, mention.candidates.len(), delta);

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

    pub(super) fn begin_inline_file_reference_line_picker(&mut self) -> bool {
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

        let replacement = format!("@{replacement_path}");
        let origin_pane = self.active_diff_pane;
        let origin_file_index = self.active_file_index();
        let origin_row_index = self
            .inline_comment
            .as_ref()
            .map_or(0, |inline| inline.row_index);
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline
            .buffer
            .replace_range_on_cursor_line(replace_start, replace_end, &replacement);
        inline.file_mention = None;
        inline.file_reference_picker = Some(crate::tui::app::InlineFileReferencePickerState {
            path: replacement_path.clone(),
            replace_start_col: replace_start,
            replace_end_col: replace_start + replacement.chars().count(),
            origin_pane,
            origin_file_index,
            origin_row_index,
        });

        let explicit_line = line_suffix.and_then(|suffix| suffix.trim().parse::<u32>().ok());
        self.open_inline_file_reference_target(&replacement_path, explicit_line)
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

    fn open_inline_file_reference_target(
        &mut self,
        path: &str,
        requested_line: Option<u32>,
    ) -> bool {
        let inferred_line = self.default_inline_reference_line_for_path(path);
        let Some(file_index) = self.resolve_file_reference_index(path) else {
            self.status_line = format!("referenced file not in current diff: {path}");
            let _ = self.clear_inline_file_reference_picker();
            return false;
        };

        if file_index != self.active_file_index() {
            self.set_active_file_index(file_index);
            self.set_active_line_index(0);
            self.selected_comment = 0;
        }
        self.ensure_row_cache_for_file(file_index);

        let target_line = requested_line.or(inferred_line);
        let line_selected = target_line.is_some_and(|line| self.goto_line_number(line))
            || self.select_first_inline_reference_line_in_current_file();

        if !line_selected {
            let _ = self.clear_inline_file_reference_picker();
        }

        self.status_line = if line_selected {
            format!("select a diff line for {path} (Enter/Tab confirms, click inserts)")
        } else {
            format!("opened {path} but no diff line is available to reference")
        };
        line_selected
    }

    fn select_first_inline_reference_line_in_current_file(&mut self) -> bool {
        self.ensure_row_cache();
        let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| row.new_line.or(row.old_line).is_some())
        else {
            return false;
        };
        self.set_active_line_index(row_index);
        true
    }

    pub(super) fn accept_inline_file_reference_line_selection(&mut self) -> bool {
        let Some((
            path,
            replace_start,
            replace_end,
            origin_pane,
            origin_file_index,
            origin_row_index,
        )) = self
            .inline_comment
            .as_ref()
            .and_then(|inline| inline.file_reference_picker.as_ref())
            .map(|picker| {
                (
                    picker.path.clone(),
                    picker.replace_start_col,
                    picker.replace_end_col,
                    picker.origin_pane,
                    picker.origin_file_index,
                    picker.origin_row_index,
                )
            })
        else {
            return false;
        };
        let Some(line) = self.current_inline_reference_line_number() else {
            self.status_line = "select a diff line with a line number".into();
            return false;
        };

        let replacement = format!("@{path}:{line}");
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline
            .buffer
            .replace_range_on_cursor_line(replace_start, replace_end, &replacement);
        inline.file_reference_picker = None;
        self.restore_inline_file_reference_origin(origin_pane, origin_file_index, origin_row_index);
        self.status_line = format!("inserted file reference: {path}:{line}");
        true
    }

    pub(super) fn current_inline_reference_line_number(&mut self) -> Option<u32> {
        self.ensure_row_cache();
        self.current_rows()
            .get(self.active_line_index())
            .and_then(|row| row.new_line.or(row.old_line))
    }

    fn restore_inline_file_reference_origin(
        &mut self,
        pane: DiffPane,
        file_index: usize,
        row_index: usize,
    ) {
        self.activate_pane(pane);
        self.set_active_file_index(file_index);
        self.ensure_row_cache_for_file(file_index);
        let max_row = self.current_rows().len().saturating_sub(1);
        self.set_active_line_index(row_index.min(max_row));
        self.constrain_selection();
    }

    pub(super) fn refresh_inline_file_mention_picker(&mut self) {
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
            .map_or(0, |mention| mention.scroll);

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
