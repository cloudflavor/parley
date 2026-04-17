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

    pub(super) fn line_len(&self, idx: usize) -> usize {
        self.lines[idx].chars().count()
    }
}

impl TuiApp {
    pub(super) fn new(
        review_name: String,
        review: ReviewSession,
        diff: DiffDocument,
        config: AppConfig,
        themes: Vec<UiTheme>,
        theme_index: usize,
        log_path: PathBuf,
    ) -> Self {
        let ai_provider = config.ai.default_provider;
        let side_by_side_diff = config.diff_view.is_side_by_side();
        Self {
            review_name,
            review,
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
            last_ai_detail: None,
            inline_comment: None,
            command_palette: None,
            theme_picker: None,
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
        let visible = self.visible_file_indices();
        if visible.is_empty() {
            self.set_active_file_index(0);
            return;
        }

        let current_pos = visible
            .iter()
            .position(|index| *index == self.active_file_index())
            .unwrap_or(0);
        let max = visible.len().saturating_sub(1) as isize;
        let next_pos = (current_pos as isize + delta).clamp(0, max) as usize;
        self.select_file(visible[next_pos]);
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

    pub(super) fn open_user_name_editor(&mut self) {
        let value = self.config.user_name.clone();
        let cursor_col = value.chars().count();
        self.settings_editor = Some(SettingsEditorState {
            kind: SettingsEditorKind::UserName,
            value,
            cursor_col,
        });
        self.status_line = "editing user name".into();
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
        }
        Ok(())
    }

    pub(super) fn open_theme_picker(&mut self) {
        if self.themes.is_empty() {
            self.status_line = "no themes loaded".into();
            return;
        }
        self.theme_picker = Some(super::ThemePickerState {
            selected_index: self.theme_index,
            scroll: self.theme_index.saturating_sub(3),
        });
        self.status_line = "theme picker opened".into();
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
            changed |= self.record_ai_progress(event);
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
        self.ai_task.is_some() || self.pending_z_prefix_at.is_some()
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
        self.diff = load_git_diff_head().await?;
        self.selected_file = previous_primary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(0);
        self.secondary_selected_file = previous_secondary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(self.selected_file);

        self.row_cache.clear();
        self.clear_diff_render_cache();
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

pub(super) fn now_ms_utc() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::{
        config::AppConfig,
        diff::{DiffDocument, DiffFile},
        review::{Author, CommentStatus, DiffSide, LineComment, ReviewSession, ReviewState},
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

    fn make_test_app(paths: Vec<&str>, comments: Vec<LineComment>) -> TuiApp {
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
        let diff = DiffDocument {
            files: paths
                .into_iter()
                .map(|path| DiffFile {
                    path: path.to_string(),
                    header_lines: Vec::new(),
                    hunks: Vec::new(),
                })
                .collect(),
        };
        let themes = load_themes().expect("embedded themes should load");
        TuiApp::new(
            review.name.clone(),
            review,
            diff,
            AppConfig::default(),
            themes,
            0,
            PathBuf::from("test.log"),
        )
    }

    fn make_comment(id: u64, file_path: &str, status: CommentStatus) -> LineComment {
        LineComment {
            id,
            file_path: file_path.to_string(),
            old_line: None,
            new_line: Some(1),
            side: DiffSide::Right,
            body: format!("comment {id}"),
            author: Author::User,
            status,
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
