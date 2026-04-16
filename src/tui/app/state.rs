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
            selected_comment: 0,
            status_line: "ready".to_string(),
            last_ai_detail: None,
            inline_comment: None,
            settings_editor: None,
            command_prompt: None,
            pending_action: None,
            ai_task: None,
            ai_progress_visible: false,
            ai_progress_lines: VecDeque::with_capacity(AI_PROGRESS_MAX_LINES),
            shortcuts_modal_visible: false,
            shortcuts_modal_scroll: 0,
            search_query: None,
            last_shortcuts_modal_area: None,
            last_file_area: None,
            last_file_scroll: 0,
            last_diff_area: None,
            last_diff_scroll: 0,
            last_diff_row_map: Vec::new(),
            last_diff_area_secondary: None,
            last_diff_scroll_secondary: 0,
            last_diff_row_map_secondary: Vec::new(),
            last_thread_nav_area: None,
            last_thread_nav_scroll: 0,
            last_thread_nav_row_map: Vec::new(),
            row_cache: HashMap::new(),
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
            self.secondary_selected_file = index;
        } else {
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
            self.secondary_selected_line = index;
        } else {
            self.selected_line = index;
        }
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
        if self.diff.files.is_empty() {
            self.set_active_file_index(0);
            return;
        }
        let max = self.diff.files.len().saturating_sub(1) as isize;
        let next = (self.active_file_index() as isize + delta).clamp(0, max) as usize;
        self.select_file(next);
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

    pub(super) fn file_comment_stats(&self) -> HashMap<String, (usize, usize)> {
        let mut stats = HashMap::new();
        for comment in &self.review.comments {
            let entry = stats.entry(comment.file_path.clone()).or_insert((0, 0));
            entry.0 += 1;
            if matches!(comment.status, CommentStatus::Open) {
                entry.1 += 1;
            }
        }
        stats
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

        let mut painter = SyntaxPainter::for_path(&file.path);
        let theme_colors = self.theme().colors.clone();
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
    }

    pub(super) async fn set_state(
        &mut self,
        service: &ReviewService,
        next: ReviewState,
    ) -> Result<()> {
        service
            .set_state(&self.review_name, next.clone())
            .await
            .with_context(|| format!("failed to set state to {:?}", next))?;
        self.reload_review(service).await?;
        self.status_line = format!("review state set to {:?}", next);
        Ok(())
    }

    pub(super) async fn reload_review(&mut self, service: &ReviewService) -> Result<()> {
        self.review = service.load_review(&self.review_name).await?;
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

    pub(super) async fn cycle_theme(&mut self, service: &ReviewService) -> Result<()> {
        if self.themes.is_empty() {
            self.status_line = "no themes loaded".into();
            return Ok(());
        }
        self.theme_index = (self.theme_index + 1) % self.themes.len();
        self.config.theme = self.theme().name.clone();
        service.save_config(&self.config).await?;
        self.row_cache.clear();
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

        if matches!(self.review.state, ReviewState::Draft) {
            self.status_line = "review is draft; press s to start review first".into();
            return Ok(());
        }

        let mut comment_ids = Vec::new();
        if selected_only {
            let Some(comment) = self.selected_comment_details() else {
                self.status_line = "no thread selected".into();
                return Ok(());
            };
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

    pub(super) async fn poll_ai_task(&mut self, service: &ReviewService) -> Result<()> {
        self.drain_ai_progress();

        let Some(task) = self.ai_task.as_ref() else {
            return Ok(());
        };
        if !task.handle.is_finished() {
            return Ok(());
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
            }
            Ok(Err(error)) => {
                self.last_ai_detail = Some(format!("ai run failed: {error}"));
                self.status_line = format!("run ai session failed: {error}");
                self.push_ai_progress_line(format!(
                    "[{}] system: run failed: {error}",
                    format_timestamp_utc(now_ms_utc())
                ));
            }
            Err(error) => {
                self.last_ai_detail = Some(format!("ai task join failed: {error}"));
                self.status_line = format!("run ai session failed: {error}");
                self.push_ai_progress_line(format!(
                    "[{}] system: task join failed: {error}",
                    format_timestamp_utc(now_ms_utc())
                ));
            }
        }
        Ok(())
    }

    pub(super) fn drain_ai_progress(&mut self) {
        let mut events = Vec::new();
        if let Some(task) = self.ai_task.as_mut() {
            while let Ok(event) = task.progress_rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.record_ai_progress(event);
        }
    }

    pub(super) fn record_ai_progress(&mut self, event: AiProgressEvent) {
        let line = format!(
            "[{}] {} {}: {}",
            format_timestamp_utc(event.timestamp_ms),
            event.provider,
            event.stream,
            event.message
        );
        self.push_ai_progress_line(line);
    }

    pub(super) fn push_ai_progress_line(&mut self, line: String) {
        self.ai_progress_lines.push_back(line);
        while self.ai_progress_lines.len() > AI_PROGRESS_MAX_LINES {
            self.ai_progress_lines.pop_front();
        }
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

    pub(super) async fn refresh_review_and_diff(&mut self, service: &ReviewService) -> Result<()> {
        let previous_primary_path = self
            .file_for_pane(DiffPane::Primary)
            .map(|f| f.path.clone());
        let previous_secondary_path = self
            .file_for_pane(DiffPane::Secondary)
            .map(|f| f.path.clone());
        self.review = service.load_review(&self.review_name).await?;
        self.diff = load_git_diff_head().await?;
        self.selected_file = previous_primary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(0);
        self.secondary_selected_file = previous_secondary_path
            .and_then(|path| self.diff.files.iter().position(|f| f.path == path))
            .unwrap_or(self.selected_file);

        self.row_cache.clear();
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
