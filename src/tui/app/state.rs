use super::*;

impl TextBuffer {
    pub(super) fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
        }
    }

    pub(super) fn char_len(&self) -> usize {
        let text_chars: usize = self.lines.iter().map(|line| line.chars().count()).sum();
        text_chars + self.lines.len().saturating_sub(1)
    }

    pub(super) fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    pub(super) fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    pub(super) fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.line_len(self.cursor_line);
        }
    }

    pub(super) fn move_right(&mut self) {
        let line_len = self.line_len(self.cursor_line);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    pub(super) fn move_word_left(&mut self) {
        let target = self.previous_word_boundary();
        self.set_cursor_absolute_index(target);
    }

    pub(super) fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
        }
    }

    pub(super) fn move_down(&mut self) {
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
        }
    }

    pub(super) fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub(super) fn move_end(&mut self) {
        self.cursor_col = self.line_len(self.cursor_line);
    }

    pub(super) fn insert_char(&mut self, ch: char) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        chars.insert(self.cursor_col, ch);
        self.lines[self.cursor_line] = chars.into_iter().collect();
        self.cursor_col += 1;
    }

    pub(super) fn insert_spaces(&mut self, count: usize) {
        for _ in 0..count {
            self.insert_char(' ');
        }
    }

    pub(super) fn insert_newline(&mut self) {
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

    pub(super) fn backspace(&mut self) {
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

    pub(super) fn delete_char(&mut self) {
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

    pub(super) fn replace_range_on_cursor_line(
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

    pub(super) fn kill_to_end(&mut self) {
        let mut chars: Vec<char> = self.lines[self.cursor_line].chars().collect();
        chars.truncate(self.cursor_col);
        self.lines[self.cursor_line] = chars.into_iter().collect();
    }

    pub(super) fn delete_word_right(&mut self) {
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

    pub(super) fn line_len(&self, idx: usize) -> usize {
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

impl TuiApp {
    pub(super) fn new(init: TuiAppInit) -> Self {
        let TuiAppInit {
            review_name,
            review,
            diff,
            diff_source,
            config,
            themes,
            theme_index,
            log_path,
        } = init;
        let ai_provider = config.ai.default_provider;
        let side_by_side_diff = config.diff_view.is_side_by_side();
        Self {
            review_name,
            review,
            diff_source,
            config,
            themes,
            theme_index,
            diff,
            ai_provider,
            log_path,
            selected_file: 0,
            secondary_selected_file: 0,
            active_diff_pane: DiffPane::Primary,
            split_diff_view: false,
            side_by_side_diff,
            file_pane_width_delta: 0,
            content_fullscreen: false,
            thread_nav_visible: false,
            selected_line: 0,
            secondary_selected_line: 0,
            primary_viewport_top_row: 0,
            secondary_viewport_top_row: 0,
            selected_comment: 0,
            status_line: "ready".to_string(),
            last_status_line_snapshot: "ready".to_string(),
            status_toast_message: None,
            status_toast_until: None,
            last_ai_detail: None,
            inline_comment: None,
            command_palette: None,
            theme_picker: None,
            commit_picker: None,
            review_picker: None,
            file_search: FileSearchState {
                query: String::new(),
                cursor_col: 0,
                focused: false,
            },
            file_filter_mode: FileFilterMode::All,
            file_sort_mode: FileSortMode::Path,
            collapsed_file_groups: std::collections::HashSet::new(),
            thread_density_mode: ThreadDensityMode::Compact,
            expanded_threads: std::collections::HashSet::new(),
            collapsed_threads: std::collections::HashSet::new(),
            settings_editor: None,
            command_prompt: None,
            pending_action: None,
            ai_task: None,
            ai_progress_visible: false,
            ai_progress_lines: VecDeque::with_capacity(AI_PROGRESS_MAX_LINES),
            ai_progress_scroll: 0,
            ai_progress_follow_tail: true,
            shortcuts_modal_visible: false,
            shortcuts_modal_scroll: 0,
            shortcuts_modal_doc_index: 0,
            shortcuts_modal_zoom_step: 0,
            search_query: None,
            last_ai_progress_area: None,
            last_shortcuts_modal_area: None,
            last_file_area: None,
            last_file_search_area: None,
            last_file_scroll: 0,
            last_file_row_map: Vec::new(),
            last_file_group_map: Vec::new(),
            last_diff_area: None,
            last_diff_scroll: 0,
            last_diff_row_map: Vec::new(),
            last_diff_link_hits: Vec::new(),
            pending_scroll_anchor_row: None,
            last_diff_area_secondary: None,
            last_diff_scroll_secondary: 0,
            last_diff_row_map_secondary: Vec::new(),
            last_diff_link_hits_secondary: Vec::new(),
            pending_scroll_anchor_row_secondary: None,
            last_thread_nav_area: None,
            last_thread_nav_scroll: 0,
            last_thread_nav_row_map: Vec::new(),
            row_cache: HashMap::new(),
            diff_render_cache: HashMap::new(),
            diff_render_cache_order: VecDeque::new(),
            pending_z_prefix_at: None,
            redraw_invalidated: true,
            should_quit: false,
        }
    }

    pub(super) fn theme(&self) -> &UiTheme {
        &self.themes[self.theme_index]
    }

    pub(super) fn author_label(&self, author: &Author) -> &str {
        match author {
            Author::User => &self.config.user_name,
            Author::Ai => "AI",
        }
    }

    pub(super) fn active_file_index(&self) -> usize {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            self.secondary_selected_file
        } else {
            self.selected_file
        }
    }

    pub(super) fn set_active_file_index(&mut self, index: usize) {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            if self.secondary_selected_file != index {
                self.pending_scroll_anchor_row_secondary = None;
                self.secondary_viewport_top_row = 0;
            }
            self.secondary_selected_file = index;
        } else {
            if self.selected_file != index {
                self.pending_scroll_anchor_row = None;
                self.primary_viewport_top_row = 0;
            }
            self.selected_file = index;
        }
    }

    pub(super) fn active_line_index(&self) -> usize {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            self.secondary_selected_line
        } else {
            self.selected_line
        }
    }

    pub(super) fn set_active_line_index(&mut self, index: usize) {
        if self.split_diff_view && matches!(self.active_diff_pane, DiffPane::Secondary) {
            if self.secondary_selected_line != index {
                self.pending_scroll_anchor_row_secondary = None;
            }
            self.secondary_selected_line = index;
        } else {
            if self.selected_line != index {
                self.pending_scroll_anchor_row = None;
            }
            self.selected_line = index;
        }
    }

    pub(super) fn set_line_for_pane(&mut self, pane: DiffPane, index: usize) {
        match pane {
            DiffPane::Primary => {
                if self.selected_line != index {
                    self.pending_scroll_anchor_row = None;
                }
                self.selected_line = index;
            }
            DiffPane::Secondary => {
                if self.secondary_selected_line != index {
                    self.pending_scroll_anchor_row_secondary = None;
                }
                self.secondary_selected_line = index;
            }
        }
    }

    pub(super) fn viewport_top_for_pane(&self, pane: DiffPane) -> usize {
        match pane {
            DiffPane::Primary => self.primary_viewport_top_row,
            DiffPane::Secondary => self.secondary_viewport_top_row,
        }
    }

    pub(super) fn set_viewport_top_for_pane(&mut self, pane: DiffPane, top_row: usize) {
        match pane {
            DiffPane::Primary => {
                self.primary_viewport_top_row = top_row;
            }
            DiffPane::Secondary => {
                self.secondary_viewport_top_row = top_row;
            }
        }
    }

    pub(super) fn take_pending_scroll_anchor(&mut self, pane: DiffPane) -> Option<usize> {
        match pane {
            DiffPane::Primary => self.pending_scroll_anchor_row.take(),
            DiffPane::Secondary => self.pending_scroll_anchor_row_secondary.take(),
        }
    }

    pub(super) fn row_map_for_pane(&self, pane: DiffPane) -> &[usize] {
        match pane {
            DiffPane::Primary => &self.last_diff_row_map,
            DiffPane::Secondary => &self.last_diff_row_map_secondary,
        }
    }

    pub(super) fn viewport_height_for_pane(&self, pane: DiffPane) -> usize {
        let area = match pane {
            DiffPane::Primary => self.last_diff_area,
            DiffPane::Secondary => self.last_diff_area_secondary,
        };
        area.map(|rect| usize::from(rect.height.saturating_sub(2)))
            .unwrap_or(1)
            .max(1)
    }

    pub(super) fn activate_pane(&mut self, pane: DiffPane) {
        if self.active_diff_pane == pane {
            return;
        }
        self.active_diff_pane = pane;
        self.inline_comment = None;
    }

    pub(super) fn toggle_split_diff_view(&mut self) {
        self.split_diff_view = !self.split_diff_view;
        if !self.split_diff_view {
            self.active_diff_pane = DiffPane::Primary;
            self.inline_comment = None;
        }
    }

    pub(super) fn resize_file_pane(&mut self, delta_cols: i16) {
        self.file_pane_width_delta = (self.file_pane_width_delta + delta_cols).clamp(-40, 80);
    }

    pub(super) fn computed_file_pane_width(&self, total_width: u16) -> u16 {
        let longest_path = self
            .diff
            .files
            .iter()
            .map(|file| file.path.chars().count())
            .max()
            .unwrap_or(16) as i16;
        // marker + spacing + border breathing room
        let base = longest_path + 8;
        let min_width = 16i16;
        let max_width = (total_width as i16 - 30).clamp(min_width, 90);
        let computed = (base + self.file_pane_width_delta).clamp(min_width, max_width);
        computed as u16
    }

    pub(super) fn file_for_pane(&self, pane: DiffPane) -> Option<&DiffFile> {
        let idx = match pane {
            DiffPane::Primary => self.selected_file,
            DiffPane::Secondary => self.secondary_selected_file,
        };
        self.diff.files.get(idx)
    }

    pub(super) fn line_for_pane(&self, pane: DiffPane) -> usize {
        match pane {
            DiffPane::Primary => self.selected_line,
            DiffPane::Secondary => self.secondary_selected_line,
        }
    }

    pub(super) fn select_file(&mut self, index: usize) {
        if self.diff.files.is_empty() {
            self.set_active_file_index(0);
            return;
        }

        let clamped = index.min(self.diff.files.len().saturating_sub(1));
        if clamped == self.active_file_index() {
            return;
        }

        self.set_active_file_index(clamped);
        self.set_active_line_index(0);
        self.selected_comment = 0;
        self.inline_comment = None;
    }

    pub(super) fn move_file_selection(&mut self, delta: isize) {
        let ordered_files = self.ordered_file_selection_indices();
        if ordered_files.is_empty() {
            self.set_active_file_index(0);
            return;
        }

        let current_pos = ordered_files
            .iter()
            .position(|index| *index == self.active_file_index())
            .unwrap_or(0);
        let max = ordered_files.len().saturating_sub(1) as isize;
        let next_pos = (current_pos as isize + delta).clamp(0, max) as usize;
        self.select_file(ordered_files[next_pos]);
    }

    fn ordered_file_selection_indices(&self) -> Vec<usize> {
        let rendered_rows = self
            .last_file_row_map
            .iter()
            .filter_map(|entry| *entry)
            .collect::<Vec<_>>();
        if !rendered_rows.is_empty() {
            return rendered_rows;
        }
        self.visible_file_indices()
    }

    pub(super) fn current_file(&self) -> Option<&DiffFile> {
        self.diff.files.get(self.active_file_index())
    }

    pub(super) fn current_rows(&self) -> &[DisplayRow] {
        self.row_cache
            .get(&self.active_file_index())
            .map(|cached| cached.rows.as_slice())
            .unwrap_or(&[])
    }

    pub(super) fn line_anchor_snapshot_for_row(
        &self,
        row_index: usize,
    ) -> Option<LineAnchorSnapshot> {
        let rows = self.current_rows();
        let row = rows.get(row_index)?;
        if !is_commentable_row(row) {
            return None;
        }
        Some(build_line_anchor_snapshot(rows, row_index))
    }

    pub(super) fn rows_and_highlights_for_file(
        &self,
        file_index: usize,
    ) -> Option<(&[DisplayRow], &[HighlightParts])> {
        let cached = self.row_cache.get(&file_index)?;
        Some((&cached.rows, &cached.highlights))
    }

    pub(super) fn comments_for_file(&self, file_path: &str) -> Vec<&LineComment> {
        self.review
            .comments
            .iter()
            .filter(|comment| comment.file_path == file_path)
            .collect()
    }

    pub(super) fn review_state_code(&self) -> u8 {
        match self.review.state {
            ReviewState::Open => 0,
            ReviewState::UnderReview => 1,
            ReviewState::Done => 2,
        }
    }

    pub(super) fn expanded_thread_ids_for_file(&self, file_path: &str) -> Vec<u64> {
        let mut ids = self
            .review
            .comments
            .iter()
            .filter(|comment| comment.file_path == file_path)
            .filter_map(|comment| {
                self.expanded_threads
                    .contains(&comment.id)
                    .then_some(comment.id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    pub(super) fn file_comment_stats(&self) -> HashMap<String, (usize, usize, usize)> {
        let mut stats = HashMap::new();
        for comment in &self.review.comments {
            let entry = stats.entry(comment.file_path.clone()).or_insert((0, 0, 0));
            entry.0 += 1;
            if matches!(comment.status, CommentStatus::Open) {
                entry.1 += 1;
            }
            if matches!(comment.status, CommentStatus::Pending) {
                entry.2 += 1;
            }
        }
        stats
    }

    pub(super) fn visible_file_indices(&self) -> Vec<usize> {
        let stats = self.file_comment_stats();
        let file_query = self.file_search_query().map(str::to_lowercase);
        let mut indices: Vec<usize> = self
            .diff
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, file)| {
                let (_total, open, pending) = stats.get(&file.path).copied().unwrap_or((0, 0, 0));
                let visible = match self.file_filter_mode {
                    FileFilterMode::All => true,
                    FileFilterMode::Open => open > 0,
                    FileFilterMode::Pending => pending > 0,
                };
                if !visible {
                    return None;
                }
                if let Some(query) = file_query.as_ref() {
                    let path = file.path.to_lowercase();
                    if !path.contains(query) {
                        return None;
                    }
                }
                Some(idx)
            })
            .collect();

        indices.sort_by(|left, right| {
            let left_file = &self.diff.files[*left];
            let right_file = &self.diff.files[*right];
            let left_stats = stats.get(&left_file.path).copied().unwrap_or((0, 0, 0));
            let right_stats = stats.get(&right_file.path).copied().unwrap_or((0, 0, 0));
            match self.file_sort_mode {
                FileSortMode::Path => left_file.path.cmp(&right_file.path),
                FileSortMode::OpenCountDesc => right_stats
                    .1
                    .cmp(&left_stats.1)
                    .then_with(|| left_file.path.cmp(&right_file.path)),
                FileSortMode::TotalCountDesc => right_stats
                    .0
                    .cmp(&left_stats.0)
                    .then_with(|| left_file.path.cmp(&right_file.path)),
            }
        });
        indices
    }

    pub(super) fn constrain_active_file_to_visible_list(&mut self) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            self.selected_file = self.diff.files.len().saturating_sub(1);
            if self.secondary_selected_file >= self.diff.files.len() {
                self.secondary_selected_file = self.diff.files.len().saturating_sub(1);
            }
            return;
        }

        if !visible.contains(&self.selected_file) {
            self.selected_file = visible[0];
            self.selected_line = 0;
            self.selected_comment = 0;
        }
        if !visible.contains(&self.secondary_selected_file) {
            self.secondary_selected_file = self.selected_file;
            self.secondary_selected_line = 0;
        }
    }

    pub(super) fn cycle_file_filter_mode(&mut self) {
        let next = match self.file_filter_mode {
            FileFilterMode::All => FileFilterMode::Open,
            FileFilterMode::Open => FileFilterMode::Pending,
            FileFilterMode::Pending => FileFilterMode::All,
        };
        self.set_file_filter_mode(next);
    }

    pub(super) fn set_file_filter_mode(&mut self, mode: FileFilterMode) {
        self.file_filter_mode = mode;
        self.constrain_active_file_to_visible_list();
        self.status_line = format!("file filter: {}", self.file_filter_mode_label());
    }

    pub(super) fn cycle_file_sort_mode(&mut self) {
        let next = match self.file_sort_mode {
            FileSortMode::Path => FileSortMode::OpenCountDesc,
            FileSortMode::OpenCountDesc => FileSortMode::TotalCountDesc,
            FileSortMode::TotalCountDesc => FileSortMode::Path,
        };
        self.set_file_sort_mode(next);
    }

    pub(super) fn set_file_sort_mode(&mut self, mode: FileSortMode) {
        self.file_sort_mode = mode;
        self.constrain_active_file_to_visible_list();
        self.status_line = format!("file sort: {}", self.file_sort_mode_label());
    }

    pub(super) fn file_filter_mode_label(&self) -> &'static str {
        match self.file_filter_mode {
            FileFilterMode::All => "all",
            FileFilterMode::Open => "open",
            FileFilterMode::Pending => "pending",
        }
    }

    pub(super) fn file_sort_mode_label(&self) -> &'static str {
        match self.file_sort_mode {
            FileSortMode::Path => "path",
            FileSortMode::OpenCountDesc => "open_count",
            FileSortMode::TotalCountDesc => "total_count",
        }
    }

    pub(super) fn file_group_name_for_index(&self, file_index: usize) -> String {
        let Some(file) = self.diff.files.get(file_index) else {
            return ".".to_string();
        };
        let path = file.path.as_str();
        path.rsplit_once('/')
            .map(|(group, _)| {
                if group.is_empty() {
                    ".".to_string()
                } else {
                    group.to_string()
                }
            })
            .unwrap_or_else(|| ".".to_string())
    }

    pub(super) fn file_search_query(&self) -> Option<&str> {
        let trimmed = self.file_search.query.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    pub(super) fn thread_density_mode_label(&self) -> &'static str {
        match self.thread_density_mode {
            ThreadDensityMode::Compact => "compact",
            ThreadDensityMode::Expanded => "expanded",
        }
    }

    pub(super) fn cycle_thread_density_mode(&mut self) {
        self.thread_density_mode = match self.thread_density_mode {
            ThreadDensityMode::Compact => ThreadDensityMode::Expanded,
            ThreadDensityMode::Expanded => ThreadDensityMode::Compact,
        };
        self.clear_diff_render_cache();
        self.status_line = format!("thread density: {}", self.thread_density_mode_label());
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

    pub(super) fn is_thread_expanded(
        &self,
        comment_id: u64,
        selected_comment_id: Option<u64>,
    ) -> bool {
        matches!(self.thread_density_mode, ThreadDensityMode::Expanded)
            || (!self.collapsed_threads.contains(&comment_id)
                && selected_comment_id == Some(comment_id))
            || self.expanded_threads.contains(&comment_id)
    }

    pub(super) fn toggle_selected_thread_expansion(&mut self) {
        let Some(comment) = self.selected_comment_details() else {
            self.status_line = "no thread selected".into();
            return;
        };
        let active_file_index = self.active_file_index();
        let comment_id = comment.id;
        let is_expanded = self.is_thread_expanded(comment_id, Some(comment_id));
        if is_expanded {
            self.expanded_threads.remove(&comment_id);
            self.collapsed_threads.insert(comment_id);
            self.status_line = format!("thread #{comment_id} collapsed");
        } else {
            self.collapsed_threads.remove(&comment_id);
            self.expanded_threads.insert(comment_id);
            self.status_line = format!("thread #{comment_id} expanded");
        }
        self.clear_diff_render_cache_for_file(active_file_index);
    }

    pub(super) fn toggle_file_group_collapsed(&mut self, group: &str) {
        if self.collapsed_file_groups.contains(group) {
            self.collapsed_file_groups.remove(group);
            self.status_line = format!("expanded group: {group}");
        } else {
            self.collapsed_file_groups.insert(group.to_string());
            self.status_line = format!("collapsed group: {group}");
            self.constrain_active_file_to_visible_list();
        }
    }

    pub(super) fn toggle_active_file_group_collapsed(&mut self) {
        let group = self.file_group_name_for_index(self.active_file_index());
        self.toggle_file_group_collapsed(&group);
    }

    pub(super) fn collapse_all_visible_file_groups(&mut self) {
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            self.status_line = "no file groups to collapse".into();
            return;
        }
        let mut groups: std::collections::HashSet<String> = std::collections::HashSet::new();
        for file_index in visible {
            groups.insert(self.file_group_name_for_index(file_index));
        }
        let before = self.collapsed_file_groups.len();
        self.collapsed_file_groups.extend(groups);
        let added = self.collapsed_file_groups.len().saturating_sub(before);
        self.constrain_active_file_to_visible_list();
        self.status_line = if added == 0 {
            "all visible groups already collapsed".into()
        } else {
            format!("collapsed {added} file group(s)")
        };
    }

    pub(super) fn comments_for_selected_file(&self) -> Vec<&LineComment> {
        let Some(file) = self.current_file() else {
            return Vec::new();
        };
        self.comments_for_file(&file.path)
    }

    pub(super) fn selected_comment_details(&self) -> Option<&LineComment> {
        let comments = self.comments_for_selected_file();
        comments.get(self.selected_comment).copied()
    }

    pub(super) fn unresolved_thread_ids(&self) -> Vec<u64> {
        self.review
            .comments
            .iter()
            .filter(|comment| !matches!(comment.status, CommentStatus::Addressed))
            .map(|comment| comment.id)
            .collect()
    }

    pub(super) fn constrain_selection(&mut self) {
        let rows_len = self
            .row_cache
            .get(&self.active_file_index())
            .map(|cached| cached.rows.len())
            .unwrap_or(0);
        if rows_len == 0 {
            self.set_active_line_index(0);
        } else if self.active_line_index() >= rows_len {
            self.set_active_line_index(rows_len - 1);
        }

        let comments_len = self.comments_for_selected_file().len();
        if comments_len == 0 {
            self.selected_comment = 0;
        } else if self.selected_comment >= comments_len {
            self.selected_comment = comments_len - 1;
        }

        if self.selected_file >= self.diff.files.len() {
            self.selected_file = self.diff.files.len().saturating_sub(1);
        }
        if self.secondary_selected_file >= self.diff.files.len() {
            self.secondary_selected_file = self.diff.files.len().saturating_sub(1);
        }
        self.constrain_active_file_to_visible_list();

        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index >= rows_len
        {
            self.inline_comment = None;
        }
    }

    pub(super) fn ensure_row_cache(&mut self) {
        self.ensure_row_cache_for_file(self.active_file_index());
    }

    pub(super) fn ensure_row_cache_for_file(&mut self, file_index: usize) {
        if self.row_cache.contains_key(&file_index) {
            return;
        }
        self.rebuild_row_cache_for_file(file_index);
    }

    pub(super) fn rebuild_row_cache_for_file(&mut self, file_index: usize) {
        let Some(file) = self.diff.files.get(file_index) else {
            self.row_cache.remove(&file_index);
            self.clear_diff_render_cache_for_file(file_index);
            return;
        };

        let mut rows = Vec::new();
        for header in &file.header_lines {
            rows.push(DisplayRow {
                kind: DiffLineKind::Meta,
                old_line: None,
                new_line: None,
                raw: header.clone(),
                code: header.clone(),
            });
        }
        for hunk in &file.hunks {
            for line in &hunk.lines {
                rows.push(DisplayRow {
                    kind: line.kind.clone(),
                    old_line: line.old_line,
                    new_line: line.new_line,
                    raw: line.raw.clone(),
                    code: line.code.clone(),
                });
            }
        }

        let theme_colors = self.theme().colors.clone();
        let mut painter = SyntaxPainter::for_path(&file.path, &theme_colors);
        let mut highlights = Vec::with_capacity(rows.len());
        for row in &rows {
            let parts = match row.kind {
                DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context => {
                    painter.highlight(&row.code, &theme_colors)
                }
                _ => Vec::new(),
            };
            highlights.push(parts);
        }
        self.row_cache
            .insert(file_index, CachedFileRows { rows, highlights });
        self.clear_diff_render_cache_for_file(file_index);
    }

    pub(super) fn clear_diff_render_cache(&mut self) {
        self.diff_render_cache.clear();
        self.diff_render_cache_order.clear();
    }

    pub(super) fn clear_diff_render_cache_for_file(&mut self, file_index: usize) {
        self.diff_render_cache
            .retain(|key, _| key.file_index != file_index);
        self.diff_render_cache_order
            .retain(|key| key.file_index != file_index);
    }

    pub(super) fn get_diff_render_cache(
        &self,
        key: &DiffRenderCacheKey,
    ) -> Option<DiffRenderCacheEntry> {
        self.diff_render_cache.get(key).cloned()
    }

    pub(super) fn insert_diff_render_cache(
        &mut self,
        key: DiffRenderCacheKey,
        entry: DiffRenderCacheEntry,
    ) {
        if self.diff_render_cache.contains_key(&key) {
            self.diff_render_cache_order
                .retain(|existing| existing != &key);
        }
        self.diff_render_cache.insert(key.clone(), entry);
        self.diff_render_cache_order.push_back(key);

        while self.diff_render_cache_order.len() > DIFF_RENDER_CACHE_MAX_ENTRIES {
            if let Some(evicted) = self.diff_render_cache_order.pop_front() {
                self.diff_render_cache.remove(&evicted);
            }
        }
    }

    pub(super) async fn set_state(
        &mut self,
        service: &ReviewService,
        next: ReviewState,
    ) -> Result<()> {
        service
            .set_state(&self.review_name, next.clone())
            .await
            .with_context(|| format!("failed to set state to {next:?}"))?;
        self.reload_review(service).await?;
        self.status_line = format!("review state set to {next:?}");
        Ok(())
    }

    pub(super) async fn reload_review(&mut self, service: &ReviewService) -> Result<()> {
        let selected_line = self.selected_line;
        let secondary_selected_line = self.secondary_selected_line;
        let selected_comment = self.selected_comment;
        self.review = service.load_review(&self.review_name).await?;
        self.expanded_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.collapsed_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.clear_diff_render_cache();
        self.selected_line = selected_line;
        self.secondary_selected_line = secondary_selected_line;
        self.selected_comment = selected_comment;
        self.constrain_selection();
        Ok(())
    }

    pub(super) fn dismiss_ai_progress_popup(&mut self) {
        self.ai_progress_visible = false;
    }

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

    pub(super) fn toggle_ai_progress_popup(&mut self) {
        if self.ai_progress_visible {
            self.ai_progress_visible = false;
            self.status_line = "ai progress popup hidden".into();
            return;
        }

        self.dismiss_blocking_overlays();
        self.ai_progress_visible = true;
        self.ai_progress_scroll_end();
        self.status_line = "ai progress popup visible".into();
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

    pub(super) async fn cycle_ai_provider(&mut self, service: &ReviewService) -> Result<()> {
        self.ai_provider = match self.ai_provider {
            AiProvider::Codex => AiProvider::Claude,
            AiProvider::Claude => AiProvider::Opencode,
            AiProvider::Opencode => AiProvider::Codex,
        };
        self.config.ai.default_provider = self.ai_provider;
        service.save_config(&self.config).await?;
        self.status_line = format!("ai provider set to {}", self.ai_provider.as_str());
        Ok(())
    }

    pub(super) async fn start_ai_session(
        &mut self,
        service: &ReviewService,
        selected_only: bool,
        mode: AiSessionMode,
    ) -> Result<()> {
        if self.ai_task.is_some() {
            self.status_line = "ai session already running".into();
            return Ok(());
        }

        if matches!(self.review.state, ReviewState::Done) {
            self.status_line = "review is done; reopen it before running ai".into();
            return Ok(());
        }

        let mut comment_ids = Vec::new();
        if selected_only {
            let Some(comment) = self.selected_comment_details() else {
                self.status_line = "no thread selected".into();
                return Ok(());
            };
            let targetable = match mode {
                AiSessionMode::Reply => true,
                AiSessionMode::Refactor => matches!(comment.status, CommentStatus::Open),
            };
            if !targetable {
                let status_label = match comment.status {
                    CommentStatus::Open => "open",
                    CommentStatus::Pending => "pending",
                    CommentStatus::Addressed => "addressed",
                };
                self.status_line = format!(
                    "thread #{} is {}; ai {} mode skips non-open threads",
                    comment.id,
                    status_label,
                    mode.as_str()
                );
                return Ok(());
            }
            comment_ids.push(comment.id);
        }

        let provider = self.ai_provider;
        let input = RunAiSessionInput {
            review_name: self.review_name.clone(),
            provider,
            comment_ids,
            mode,
            diff_source: self.diff_source.clone(),
        };
        let (progress_tx, progress_rx) = mpsc::channel();
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            run_ai_session_with_progress(&service_clone, input, progress_tx).await
        });

        self.ai_task = Some(AiRunTask {
            started_at: Instant::now(),
            provider,
            mode,
            handle,
            progress_rx,
        });
        self.push_ai_progress_line(format!(
            "[{}] {} system: started session ({})",
            format_timestamp_utc(now_ms_utc()),
            provider.as_str(),
            mode.as_str()
        ));
        self.last_ai_detail = Some(if selected_only {
            format!(
                "ai is processing selected thread with {} ({})",
                provider.as_str(),
                mode.as_str()
            )
        } else {
            format!(
                "ai is processing unresolved threads with {} ({})",
                provider.as_str(),
                mode.as_str()
            )
        });
        self.status_line = format!(
            "ai session started: provider={} scope={} mode={}",
            provider.as_str(),
            if selected_only { "thread" } else { "review" },
            mode.as_str()
        );
        Ok(())
    }

    pub(super) fn cancel_ai_task(&mut self) {
        let Some(task) = self.ai_task.take() else {
            self.status_line = "no ai session running".into();
            return;
        };

        while let Ok(event) = task.progress_rx.try_recv() {
            self.record_ai_progress(event);
        }
        let provider = task.provider;
        let mode = task.mode;
        let elapsed_ms = task.started_at.elapsed().as_millis();
        task.handle.abort();
        self.push_ai_progress_line(format!(
            "[{}] {} system: cancelled after {}ms",
            format_timestamp_utc(now_ms_utc()),
            provider.as_str(),
            elapsed_ms
        ));
        self.last_ai_detail = Some(format!(
            "ai session cancelled: {} ({}) after {}ms",
            provider.as_str(),
            mode.as_str(),
            elapsed_ms
        ));
        self.status_line = format!(
            "ai session cancelled: provider={} mode={}",
            provider.as_str(),
            mode.as_str()
        );
    }

    pub(super) async fn poll_ai_task(&mut self, service: &ReviewService) -> Result<bool> {
        let mut changed = self.drain_ai_progress();

        let Some(task) = self.ai_task.as_ref() else {
            return Ok(changed);
        };
        if !task.handle.is_finished() {
            return Ok(changed);
        }

        let task = self.ai_task.take().expect("checked as some");
        while let Ok(event) = task.progress_rx.try_recv() {
            self.record_ai_progress(event);
        }
        match task.handle.await {
            Ok(Ok(result)) => {
                self.refresh_review_and_diff(service).await?;
                let failed = result.items.iter().find(|item| item.status == "failed");
                self.status_line = if let Some(item) = failed {
                    format!("ai failed on #{}: {}", item.comment_id, item.message)
                } else {
                    format!(
                        "ai session {} ({}) processed {} | skipped {} | failed {}",
                        result.provider,
                        result.mode,
                        result.processed,
                        result.skipped,
                        result.failed
                    )
                };
                self.last_ai_detail = Some(if result.processed > 0 {
                    format!("ai processed {} thread(s)", result.processed)
                } else {
                    "ai session had no actionable threads".to_string()
                });
                self.push_ai_progress_line(format!(
                    "[{}] {} system: finished (processed={} skipped={} failed={})",
                    format_timestamp_utc(now_ms_utc()),
                    result.provider,
                    result.processed,
                    result.skipped,
                    result.failed
                ));
                changed = true;
            }
            Ok(Err(error)) => {
                self.last_ai_detail = Some(format!("ai run failed: {error}"));
                self.status_line = format!("run ai session failed: {error}");
                self.push_ai_progress_line(format!(
                    "[{}] system: run failed: {error}",
                    format_timestamp_utc(now_ms_utc())
                ));
                changed = true;
            }
            Err(error) => {
                self.last_ai_detail = Some(format!("ai task join failed: {error}"));
                self.status_line = format!("run ai session failed: {error}");
                self.push_ai_progress_line(format!(
                    "[{}] system: task join failed: {error}",
                    format_timestamp_utc(now_ms_utc())
                ));
                changed = true;
            }
        }
        Ok(changed)
    }

    pub(super) fn drain_ai_progress(&mut self) -> bool {
        let mut changed = false;
        let mut events = Vec::new();
        if let Some(task) = self.ai_task.as_mut() {
            while let Ok(event) = task.progress_rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            changed |= self.record_ai_progress(event);
        }
        changed
    }

    pub(super) fn record_ai_progress(&mut self, event: AiProgressEvent) -> bool {
        let Some(message) = Self::normalized_ai_stream_message(&event.stream, &event.message)
        else {
            return false;
        };

        let line = match event.stream.as_str() {
            "stdout" => message,
            "stderr" => format!("stderr: {message}"),
            _ => format!(
                "[{}] {} {}: {}",
                format_timestamp_utc(event.timestamp_ms),
                event.provider,
                event.stream,
                message
            ),
        };
        self.push_ai_progress_line(line);
        true
    }

    pub(super) fn push_ai_progress_line(&mut self, line: String) {
        self.ai_progress_lines.push_back(line);
        while self.ai_progress_lines.len() > AI_PROGRESS_MAX_LINES {
            self.ai_progress_lines.pop_front();
        }
        if self.ai_progress_follow_tail {
            self.ai_progress_scroll = usize::MAX;
        }
    }

    pub(super) fn ai_progress_scroll_up(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        self.ai_progress_follow_tail = false;
        self.ai_progress_scroll = self.ai_progress_scroll.saturating_sub(lines);
    }

    pub(super) fn ai_progress_scroll_down(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        self.ai_progress_scroll = self.ai_progress_scroll.saturating_add(lines);
    }

    pub(super) fn ai_progress_scroll_home(&mut self) {
        self.ai_progress_follow_tail = false;
        self.ai_progress_scroll = 0;
    }

    pub(super) fn ai_progress_scroll_end(&mut self) {
        self.ai_progress_follow_tail = true;
        self.ai_progress_scroll = usize::MAX;
    }

    pub(super) fn ai_progress_resolved_scroll(&mut self, max_scroll: usize) -> usize {
        if self.ai_progress_follow_tail {
            self.ai_progress_scroll = max_scroll;
            return max_scroll;
        }

        let clamped = self.ai_progress_scroll.min(max_scroll);
        self.ai_progress_scroll = clamped;
        if clamped >= max_scroll {
            self.ai_progress_follow_tail = true;
        }
        clamped
    }

    pub(super) fn requires_periodic_redraw(&self) -> bool {
        self.ai_task.is_some()
            || self.pending_z_prefix_at.is_some()
            || self
                .status_toast_until
                .is_some_and(|deadline| Instant::now() < deadline)
    }

    pub(super) fn refresh_status_toast(&mut self) {
        let now = Instant::now();
        if self.status_line != self.last_status_line_snapshot {
            self.last_status_line_snapshot = self.status_line.clone();
            if self.status_line.trim().is_empty() || self.status_line == "ready" {
                self.status_toast_message = None;
                self.status_toast_until = None;
            } else {
                self.status_toast_message = Some(self.status_line.clone());
                self.status_toast_until = now.checked_add(Duration::from_secs(4));
            }
        }

        if self
            .status_toast_until
            .is_some_and(|deadline| now >= deadline)
        {
            self.status_toast_until = None;
            self.status_toast_message = None;
        }
    }

    pub(super) fn invalidate_redraw(&mut self) {
        self.redraw_invalidated = true;
    }

    pub(super) fn take_redraw_invalidation(&mut self) -> bool {
        std::mem::take(&mut self.redraw_invalidated)
    }

    fn normalized_ai_stream_message(stream: &str, message: &str) -> Option<String> {
        if !matches!(stream, "stdout" | "stderr") {
            return Some(message.to_string());
        }

        let trimmed = message.trim();
        if trimmed.is_empty() {
            return None;
        }

        let json_candidate = trimmed
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(trimmed);
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_candidate) {
            let mut parts = Vec::new();
            Self::collect_ai_text_fragments(&value, None, &mut parts);
            let merged = parts.join("");
            let normalized = merged.trim();
            if !normalized.is_empty() {
                return Some(normalized.to_string());
            }

            let tag = Self::extract_ai_activity_tag(&value).unwrap_or_else(|| "event".to_string());
            if tag.ends_with(".started") || tag.ends_with(".completed") {
                return None;
            }
            return Some(format!("[{tag}]"));
        }

        Some(message.to_string())
    }

    fn collect_ai_text_fragments(
        value: &serde_json::Value,
        parent_key: Option<&str>,
        out: &mut Vec<String>,
    ) {
        match value {
            serde_json::Value::String(text) => {
                if Self::is_text_field(parent_key) && !text.trim().is_empty() {
                    out.push(text.to_string());
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_ai_text_fragments(item, parent_key, out);
                }
            }
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    Self::collect_ai_text_fragments(value, Some(key.as_str()), out);
                }
            }
            _ => {}
        }
    }

    fn is_text_field(parent_key: Option<&str>) -> bool {
        matches!(
            parent_key,
            Some(
                "text"
                    | "output_text"
                    | "delta"
                    | "message"
                    | "content"
                    | "error"
                    | "reasoning"
                    | "reasoning_text"
                    | "summary"
                    | "explanation"
                    | "final"
            )
        )
    }

    fn extract_ai_activity_tag(value: &serde_json::Value) -> Option<String> {
        let map = value.as_object()?;
        for key in ["type", "event", "status", "phase", "kind", "name"] {
            let Some(raw) = map.get(key) else {
                continue;
            };
            let Some(text) = raw.as_str() else {
                continue;
            };
            let normalized = text.trim();
            if !normalized.is_empty() {
                return Some(normalized.to_string());
            }
        }
        None
    }

    pub(super) fn focus_selected_comment_line(&mut self) {
        self.ensure_row_cache();
        let comments = self.comments_for_selected_file();
        let Some(comment) = comments.get(self.selected_comment).copied() else {
            return;
        };
        if let Some((row_index, _)) = self
            .current_rows()
            .iter()
            .enumerate()
            .find(|(_, row)| comment_matches_display_row(comment, row))
        {
            self.set_active_line_index(row_index);
        }
    }

    pub(super) fn request_scroll_to_thread_tail(
        &mut self,
        pane: DiffPane,
        source_row_index: usize,
    ) {
        let target = match pane {
            DiffPane::Primary => self
                .last_diff_row_map
                .iter()
                .enumerate()
                .filter_map(|(visual_row, &row_index)| {
                    (row_index == source_row_index).then_some(visual_row)
                })
                .next_back(),
            DiffPane::Secondary => self
                .last_diff_row_map_secondary
                .iter()
                .enumerate()
                .filter_map(|(visual_row, &row_index)| {
                    (row_index == source_row_index).then_some(visual_row)
                })
                .next_back(),
        };

        match pane {
            DiffPane::Primary => {
                self.pending_scroll_anchor_row = target;
            }
            DiffPane::Secondary => {
                self.pending_scroll_anchor_row_secondary = target;
            }
        }
    }

    fn remap_comment_anchors(&mut self) -> bool {
        let mut changed = false;
        let remap_timestamp = now_ms_utc();

        for index in 0..self.review.comments.len() {
            let snapshot = self.review.comments[index].clone();
            let resolved = self.resolve_comment_anchor(&snapshot);
            let comment = &mut self.review.comments[index];

            match resolved {
                Some(target) => {
                    let needs_update = comment.side != target.side
                        || comment.old_line != target.old_line
                        || comment.new_line != target.new_line
                        || comment.detached
                        || comment.line_anchor.as_ref() != Some(&target.line_anchor);
                    if needs_update {
                        comment.side = target.side;
                        comment.old_line = target.old_line;
                        comment.new_line = target.new_line;
                        comment.detached = false;
                        comment.line_anchor = Some(target.line_anchor);
                        comment.updated_at_ms = remap_timestamp;
                        changed = true;
                    }
                }
                None => {
                    if !comment.detached {
                        comment.detached = true;
                        comment.updated_at_ms = remap_timestamp;
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.review.updated_at_ms = remap_timestamp;
        }
        changed
    }

    fn resolve_comment_anchor(&mut self, comment: &LineComment) -> Option<ResolvedLineAnchor> {
        let file_index = self
            .diff
            .files
            .iter()
            .position(|file| file.path == comment.file_path)?;
        self.ensure_row_cache_for_file(file_index);
        let rows = self.row_cache.get(&file_index)?.rows.as_slice();

        if let Some((row_index, _)) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| is_commentable_row(row) && row_matches_exact_anchor(comment, row))
        {
            return Some(ResolvedLineAnchor::from_row(rows, row_index));
        }

        let snapshot = comment.line_anchor.as_ref()?;
        if snapshot.target_code.trim().is_empty() {
            return None;
        }

        let mut best_match: Option<(i32, usize)> = None;
        for (row_index, row) in rows.iter().enumerate() {
            if !is_commentable_row(row) {
                continue;
            }
            let score =
                score_anchor_candidate(comment.side.clone(), snapshot, rows, row_index, row);
            if let Some((best_score, _)) = best_match
                && score <= best_score
            {
                continue;
            }
            best_match = Some((score, row_index));
        }

        let (score, row_index) = best_match?;
        (score >= 90).then(|| ResolvedLineAnchor::from_row(rows, row_index))
    }

    pub(super) async fn refresh_review_and_diff(&mut self, service: &ReviewService) -> Result<()> {
        let previous_primary_path = self
            .file_for_pane(DiffPane::Primary)
            .map(|f| f.path.clone());
        let previous_secondary_path = self
            .file_for_pane(DiffPane::Secondary)
            .map(|f| f.path.clone());
        let selected_line = self.selected_line;
        let secondary_selected_line = self.secondary_selected_line;
        let selected_comment = self.selected_comment;
        self.review = service.load_review(&self.review_name).await?;
        self.expanded_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.collapsed_threads
            .retain(|id| self.review.comments.iter().any(|comment| comment.id == *id));
        self.diff = load_git_diff(&self.config, &self.diff_source).await?;
        self.row_cache.clear();
        self.clear_diff_render_cache();
        if self.remap_comment_anchors() {
            service.save_review(&self.review).await?;
        }
        self.selected_file = previous_primary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(0);
        self.secondary_selected_file = previous_secondary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(self.selected_file);

        self.selected_line = selected_line;
        self.secondary_selected_line = secondary_selected_line;
        self.selected_comment = selected_comment;
        self.ensure_row_cache_for_file(self.selected_file);
        if self.split_diff_view {
            self.ensure_row_cache_for_file(self.secondary_selected_file);
        }
        self.constrain_selection();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLineAnchor {
    side: DiffSide,
    old_line: Option<u32>,
    new_line: Option<u32>,
    line_anchor: LineAnchorSnapshot,
}

impl ResolvedLineAnchor {
    fn from_row(rows: &[DisplayRow], row_index: usize) -> Self {
        let row = &rows[row_index];
        let (side, old_line, new_line) = row_to_comment_anchor(row);
        Self {
            side,
            old_line,
            new_line,
            line_anchor: build_line_anchor_snapshot(rows, row_index),
        }
    }
}

fn build_line_anchor_snapshot(rows: &[DisplayRow], row_index: usize) -> LineAnchorSnapshot {
    let row = &rows[row_index];
    let (before_context, after_context) = row_context_windows(rows, row_index, 2);
    LineAnchorSnapshot {
        target_code: normalize_anchor_text(&row.code),
        before_context,
        after_context,
    }
}

fn row_matches_exact_anchor(comment: &LineComment, row: &DisplayRow) -> bool {
    match (comment.old_line, comment.new_line) {
        (Some(old), Some(new)) => row.old_line == Some(old) && row.new_line == Some(new),
        (Some(old), None) => row.old_line == Some(old),
        (None, Some(new)) => row.new_line == Some(new),
        (None, None) => false,
    }
}

fn score_anchor_candidate(
    preferred_side: DiffSide,
    snapshot: &LineAnchorSnapshot,
    rows: &[DisplayRow],
    row_index: usize,
    row: &DisplayRow,
) -> i32 {
    let row_text = normalize_anchor_text(&row.code);
    let target_text = normalize_anchor_text(&snapshot.target_code);
    if row_text.is_empty() || target_text.is_empty() {
        return i32::MIN;
    }

    let mut score = 0;
    if row_text == target_text {
        score += 100;
    } else if normalize_ws(&row_text) == normalize_ws(&target_text) {
        score += 80;
    } else if row_text.contains(&target_text) || target_text.contains(&row_text) {
        score += 40;
    }

    let (before_context, after_context) = row_context_windows(rows, row_index, 2);
    score += score_context_side(&snapshot.before_context, &before_context);
    score += score_context_side(&snapshot.after_context, &after_context);

    if (matches!(preferred_side, DiffSide::Left) && row.old_line.is_some())
        || (matches!(preferred_side, DiffSide::Right) && row.new_line.is_some())
    {
        score += 5;
    }

    score
}

fn score_context_side(expected: &[String], actual: &[String]) -> i32 {
    expected
        .iter()
        .zip(actual.iter())
        .map(|(left, right)| {
            if left == right {
                25
            } else if normalize_ws(left) == normalize_ws(right) {
                10
            } else {
                0
            }
        })
        .sum()
}

fn row_context_windows(
    rows: &[DisplayRow],
    row_index: usize,
    max_lines: usize,
) -> (Vec<String>, Vec<String>) {
    let mut before = Vec::new();
    let mut cursor = row_index;
    while cursor > 0 && before.len() < max_lines {
        cursor -= 1;
        let row = &rows[cursor];
        if !is_commentable_row(row) {
            continue;
        }
        before.push(normalize_anchor_text(&row.code));
    }

    let mut after = Vec::new();
    let mut cursor = row_index + 1;
    while cursor < rows.len() && after.len() < max_lines {
        let row = &rows[cursor];
        cursor += 1;
        if !is_commentable_row(row) {
            continue;
        }
        after.push(normalize_anchor_text(&row.code));
    }

    (before, after)
}

fn normalize_anchor_text(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_commentable_row(row: &DisplayRow) -> bool {
    matches!(
        row.kind,
        DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
    )
}

fn row_to_comment_anchor(row: &DisplayRow) -> (DiffSide, Option<u32>, Option<u32>) {
    match row.kind {
        DiffLineKind::Added => (DiffSide::Right, None, row.new_line),
        DiffLineKind::Removed => (DiffSide::Left, row.old_line, None),
        DiffLineKind::Context => (DiffSide::Right, row.old_line, row.new_line),
        _ => (DiffSide::Right, None, None),
    }
}

pub(super) fn now_ms_utc() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::config::AppConfig;
    use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crate::domain::review::{
        Author, CommentStatus, DiffSide, LineAnchorSnapshot, LineComment, ReviewSession,
        ReviewState,
    };
    use crate::tui::theme::load_themes;

    use super::*;

    #[test]
    fn visible_file_indices_respects_filter_sort_and_search_query() {
        let mut app = make_test_app(
            vec!["src/a.rs", "src/b.rs", "tests/c.rs"],
            vec![
                make_comment(1, "src/a.rs", CommentStatus::Open),
                make_comment(2, "src/a.rs", CommentStatus::Pending),
                make_comment(3, "src/b.rs", CommentStatus::Pending),
            ],
        );

        assert_eq!(app.visible_file_indices(), vec![0, 1, 2]);

        app.set_file_sort_mode(FileSortMode::OpenCountDesc);
        assert_eq!(app.visible_file_indices(), vec![0, 1, 2]);

        app.set_file_filter_mode(FileFilterMode::Pending);
        assert_eq!(app.visible_file_indices(), vec![0, 1]);

        app.file_search.query = "src/b".to_string();
        app.file_search.cursor_col = app.file_search.query.chars().count();
        assert_eq!(app.visible_file_indices(), vec![1]);
    }

    #[test]
    fn file_filter_constrains_selection_to_visible_files() {
        let mut app = make_test_app(
            vec!["src/a.rs", "tests/c.rs"],
            vec![make_comment(1, "src/a.rs", CommentStatus::Open)],
        );
        app.selected_file = 1;

        app.set_file_filter_mode(FileFilterMode::Open);

        assert_eq!(app.selected_file, 0);
    }

    #[test]
    fn collapse_all_visible_file_groups_only_collapses_current_filter_scope() {
        let mut app = make_test_app(
            vec!["src/a.rs", "src/b.rs", "tests/c.rs"],
            vec![make_comment(1, "src/a.rs", CommentStatus::Open)],
        );
        app.set_file_filter_mode(FileFilterMode::Open);

        app.collapse_all_visible_file_groups();

        assert!(app.collapsed_file_groups.contains("src"));
        assert!(!app.collapsed_file_groups.contains("tests"));
    }

    #[test]
    fn move_file_selection_follows_rendered_sidebar_order() {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs", "tests/c.rs"], Vec::new());
        app.last_file_row_map = vec![None, Some(2), None, Some(0), Some(1)];
        app.selected_file = 2;

        app.move_file_selection(1);
        assert_eq!(app.selected_file, 0);

        app.move_file_selection(1);
        assert_eq!(app.selected_file, 1);

        app.move_file_selection(-1);
        assert_eq!(app.selected_file, 0);
    }

    #[test]
    fn text_buffer_move_word_left_skips_whitespace_and_previous_word() {
        let mut buffer = TextBuffer {
            lines: vec!["alpha  beta".into()],
            cursor_line: 0,
            cursor_col: "alpha  beta".chars().count(),
        };

        buffer.move_word_left();
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, "alpha  ".chars().count());

        buffer.move_word_left();
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, 0);
    }

    #[test]
    fn text_buffer_delete_word_right_removes_next_word_across_newline() {
        let mut buffer = TextBuffer {
            lines: vec!["alpha".into(), "beta gamma".into()],
            cursor_line: 0,
            cursor_col: "alpha".chars().count(),
        };

        buffer.delete_word_right();

        assert_eq!(buffer.lines, vec!["alpha gamma"]);
        assert_eq!(buffer.cursor_line, 0);
        assert_eq!(buffer.cursor_col, "alpha".chars().count());
    }

    #[test]
    fn redraw_invalidation_roundtrip_is_explicit() {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new());

        assert!(app.take_redraw_invalidation());
        assert!(!app.take_redraw_invalidation());

        app.invalidate_redraw();
        assert!(app.take_redraw_invalidation());
        assert!(!app.take_redraw_invalidation());
    }

    #[test]
    fn clear_diff_render_cache_for_file_is_scoped() {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], Vec::new());
        app.insert_diff_render_cache(cache_key(0), cache_entry());
        app.insert_diff_render_cache(cache_key(1), cache_entry());

        app.clear_diff_render_cache_for_file(0);

        assert!(app.get_diff_render_cache(&cache_key(0)).is_none());
        assert!(app.get_diff_render_cache(&cache_key(1)).is_some());
    }

    #[test]
    fn opening_modal_overlays_hides_ai_progress_popup() {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new());
        app.ai_progress_visible = true;

        app.open_command_prompt(CommandPromptMode::GotoLine);
        assert!(!app.ai_progress_visible);
        assert!(app.command_prompt.is_some());

        app.ai_progress_visible = true;
        app.open_help_docs();
        assert!(!app.ai_progress_visible);
        assert!(app.shortcuts_modal_visible);

        app.ai_progress_visible = true;
        app.open_user_name_editor();
        assert!(!app.ai_progress_visible);
        assert!(app.settings_editor.is_some());

        app.ai_progress_visible = true;
        app.open_create_review_editor();
        assert!(!app.ai_progress_visible);
        assert!(app.settings_editor.is_some());

        app.ai_progress_visible = true;
        app.open_theme_picker();
        assert!(!app.ai_progress_visible);
        assert!(app.theme_picker.is_some());
    }

    #[test]
    fn review_picker_filter_matches_name_and_state() {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new());
        app.review_picker = Some(ReviewPickerState {
            reviews: vec![
                ReviewPickerEntry {
                    name: "parser-cleanup".into(),
                    state: ReviewState::Open,
                    open_count: 1,
                    pending_count: 0,
                    addressed_count: 0,
                },
                ReviewPickerEntry {
                    name: "release-check".into(),
                    state: ReviewState::Done,
                    open_count: 0,
                    pending_count: 0,
                    addressed_count: 3,
                },
            ],
            query: "done".into(),
            cursor_col: 4,
            selected_index: 0,
            scroll: 0,
        });

        assert_eq!(app.review_picker_filtered_indices(), vec![1]);

        let picker = app
            .review_picker
            .as_mut()
            .expect("review picker should be open");
        picker.query = "parser".into();
        picker.cursor_col = 6;

        assert_eq!(app.review_picker_filtered_indices(), vec![0]);
    }

    #[test]
    fn showing_ai_progress_popup_closes_other_blocking_overlays() {
        let mut app = make_test_app(vec!["src/a.rs"], Vec::new());
        app.command_palette = Some(CommandPaletteState {
            query: "theme".into(),
            cursor_col: 5,
            selected_index: 0,
            scroll: 0,
        });
        app.command_prompt = Some(CommandPromptState {
            mode: CommandPromptMode::Search,
            value: "needle".into(),
            cursor_col: 6,
        });
        app.settings_editor = Some(SettingsEditorState {
            kind: SettingsEditorKind::UserName,
            value: "Vic".into(),
            cursor_col: 3,
        });
        app.theme_picker = Some(ThemePickerState {
            selected_index: 0,
            scroll: 0,
        });
        app.commit_picker = Some(CommitPickerState {
            commits: Vec::new(),
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        app.review_picker = Some(ReviewPickerState {
            reviews: Vec::new(),
            query: String::new(),
            cursor_col: 0,
            selected_index: 0,
            scroll: 0,
        });
        app.shortcuts_modal_visible = true;

        app.toggle_ai_progress_popup();

        assert!(app.ai_progress_visible);
        assert!(app.command_palette.is_none());
        assert!(app.command_prompt.is_none());
        assert!(app.settings_editor.is_none());
        assert!(app.theme_picker.is_none());
        assert!(app.commit_picker.is_none());
        assert!(app.review_picker.is_none());
        assert!(!app.shortcuts_modal_visible);
    }

    #[test]
    fn remap_comment_anchors_keeps_exact_and_detaches_missing_anchors() {
        let mut app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[(10, "fn keep() {}"), (11, "let value = 1;")],
            )],
            vec![
                make_comment_with_anchor(1, "src/a.rs", Some(10), Some(10), None),
                make_comment_with_anchor(2, "src/a.rs", Some(99), Some(99), None),
            ],
        );

        assert!(app.remap_comment_anchors());

        assert!(!app.review.comments[0].detached);
        assert_eq!(app.review.comments[0].old_line, Some(10));
        assert_eq!(app.review.comments[0].new_line, Some(10));
        assert!(app.review.comments[0].line_anchor.is_some());

        assert!(app.review.comments[1].detached);
        assert_eq!(app.review.comments[1].old_line, Some(99));
        assert_eq!(app.review.comments[1].new_line, Some(99));
    }

    #[test]
    fn remap_comment_anchors_prefers_context_when_target_text_repeats() {
        let mut app = make_test_app_with_files_and_comments(
            vec![diff_file_with_context_lines(
                "src/a.rs",
                &[
                    (100, "let before_one = true;"),
                    (101, "let value = do_work();"),
                    (102, "let after_one = true;"),
                    (200, "let before_two = true;"),
                    (201, "let value = do_work();"),
                    (202, "let after_two = true;"),
                ],
            )],
            vec![make_comment_with_anchor(
                1,
                "src/a.rs",
                Some(999),
                Some(999),
                Some(LineAnchorSnapshot {
                    target_code: "let value = do_work();".to_string(),
                    before_context: vec!["let before_two = true;".to_string()],
                    after_context: vec!["let after_two = true;".to_string()],
                }),
            )],
        );

        assert!(app.remap_comment_anchors());

        let comment = &app.review.comments[0];
        assert!(!comment.detached);
        assert_eq!(comment.old_line, Some(201));
        assert_eq!(comment.new_line, Some(201));
        assert_eq!(comment.side, DiffSide::Right);
    }

    fn make_test_app(paths: Vec<&str>, comments: Vec<LineComment>) -> TuiApp {
        let files = paths
            .into_iter()
            .map(|path| DiffFile {
                path: path.to_string(),
                header_lines: Vec::new(),
                hunks: Vec::new(),
            })
            .collect();
        make_test_app_with_files_and_comments(files, comments)
    }

    fn make_test_app_with_files_and_comments(
        files: Vec<DiffFile>,
        comments: Vec<LineComment>,
    ) -> TuiApp {
        let review = ReviewSession {
            name: "test-review".to_string(),
            state: ReviewState::Open,
            created_at_ms: 0,
            updated_at_ms: 0,
            done_at_ms: None,
            comments,
            next_comment_id: 100,
            next_reply_id: 1,
        };
        let diff = DiffDocument { files };
        let themes = load_themes().expect("embedded themes should load");
        TuiApp::new(TuiAppInit {
            review_name: review.name.clone(),
            review,
            diff,
            diff_source: DiffSource::WorkingTree,
            config: AppConfig::default(),
            themes,
            theme_index: 0,
            log_path: PathBuf::from("test.log"),
        })
    }

    fn diff_file_with_context_lines(path: &str, lines: &[(u32, &str)]) -> DiffFile {
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

    fn make_comment(id: u64, file_path: &str, status: CommentStatus) -> LineComment {
        LineComment {
            id,
            file_path: file_path.to_string(),
            old_line: None,
            new_line: Some(1),
            side: DiffSide::Right,
            line_anchor: None,
            detached: false,
            body: format!("comment {id}"),
            author: Author::User,
            status,
            replies: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
            addressed_at_ms: None,
        }
    }

    fn make_comment_with_anchor(
        id: u64,
        file_path: &str,
        old_line: Option<u32>,
        new_line: Option<u32>,
        line_anchor: Option<LineAnchorSnapshot>,
    ) -> LineComment {
        LineComment {
            id,
            file_path: file_path.to_string(),
            old_line,
            new_line,
            side: DiffSide::Right,
            line_anchor,
            detached: false,
            body: format!("comment {id}"),
            author: Author::User,
            status: CommentStatus::Open,
            replies: Vec::new(),
            created_at_ms: 0,
            updated_at_ms: 0,
            addressed_at_ms: None,
        }
    }

    fn cache_key(file_index: usize) -> DiffRenderCacheKey {
        DiffRenderCacheKey {
            file_index,
            pane_inner_width: 80,
            side_by_side_diff: false,
            search_query: None,
            thread_density_mode: ThreadDensityMode::Compact,
            selected_line: 0,
            selected_comment_id: None,
            expanded_thread_ids: Vec::new(),
            review_state_code: 0,
            is_active: true,
        }
    }

    fn cache_entry() -> DiffRenderCacheEntry {
        DiffRenderCacheEntry {
            lines: Vec::new(),
            row_map: Vec::new(),
            link_hits: Vec::new(),
        }
    }
}
