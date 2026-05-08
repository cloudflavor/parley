use super::*;

impl TuiApp {
    pub(super) async fn handle_inline_comment_key(
        &mut self,
        key: KeyEvent,
        service: &ReviewService,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::Esc) {
            if self.clear_inline_file_reference_picker() {
                self.status_line = "line picker cancelled".into();
                return Ok(());
            }
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

        if self.inline_file_reference_picker_active() {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.ensure_row_cache();
                    self.set_active_line_index(self.active_line_index().saturating_sub(1));
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.ensure_row_cache();
                    let max = self.current_rows().len().saturating_sub(1);
                    self.set_active_line_index((self.active_line_index() + 1).min(max));
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::PageUp => {
                    self.scroll_active_pane_page(false, false);
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.scroll_active_pane_page(true, false);
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    self.set_active_line_index(0);
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::End => {
                    self.ensure_row_cache();
                    self.set_active_line_index(self.current_rows().len().saturating_sub(1));
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::Char('G') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.ensure_row_cache();
                    self.set_active_line_index(self.current_rows().len().saturating_sub(1));
                    self.constrain_selection();
                    return Ok(());
                }
                KeyCode::Enter | KeyCode::Tab => {
                    if self.accept_inline_file_reference_line_selection() {
                        return Ok(());
                    }
                }
                _ => {}
            }
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
                    let _ = self.begin_inline_file_reference_line_picker();
                    return Ok(());
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
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::ALT) => {
                inline.buffer.move_word_left();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::ALT) => {
                inline.buffer.delete_word_right();
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

    pub(super) fn inline_file_reference_picker_active(&self) -> bool {
        self.inline_comment
            .as_ref()
            .and_then(|inline| inline.file_reference_picker.as_ref())
            .is_some()
    }

    fn clear_inline_file_mention_picker(&mut self) -> bool {
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline.file_mention.take().is_some()
    }

    fn clear_inline_file_reference_picker(&mut self) -> bool {
        let Some(inline) = self.inline_comment.as_mut() else {
            return false;
        };
        inline.file_reference_picker.take().is_some()
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
            .map(|inline| inline.row_index)
            .unwrap_or(0);
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
        let line_selected = target_line
            .map(|line| self.goto_line_number(line))
            .unwrap_or(false)
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
        let line_anchor = self.line_anchor_snapshot_for_row(row_index)?;

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
            line_anchor,
        })
    }

    pub(super) fn toggle_inline_comment_for_selected_line(&mut self) {
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
            file_reference_picker: None,
        });
        self.status_line = "comment box expanded".into();
    }

    pub(super) fn start_inline_reply_for_selected_comment(&mut self) {
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
            file_reference_picker: None,
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

        let mut select_comment_id = None;
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
                            line_anchor: Some(target.line_anchor),
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
                select_comment_id = Some(comment_id);
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
        if let Some(comment_id) = select_comment_id {
            self.select_comment_by_id(comment_id);
        }
        Ok(())
    }

    pub(super) async fn reanchor_selected_comment(
        &mut self,
        service: &ReviewService,
    ) -> Result<()> {
        let Some(comment) = self.selected_comment_details().cloned() else {
            self.status_line = "no selected thread to re-anchor".into();
            return Ok(());
        };
        let Some(target) = self.comment_target_for_row(self.active_line_index()) else {
            self.status_line = "selected line cannot receive a thread anchor".into();
            return Ok(());
        };

        service
            .reanchor_comment(
                &self.review_name,
                ReanchorCommentInput {
                    comment_id: comment.id,
                    file_path: target.file_path,
                    old_line: target.old_line,
                    new_line: target.new_line,
                    side: target.side,
                    line_anchor: Some(target.line_anchor),
                },
            )
            .await
            .context("failed to persist thread re-anchor")?;
        self.reload_review(service).await?;
        self.status_line = format!(
            "thread #{} re-anchored to {}",
            comment.id,
            format_line_reference(target.old_line, target.new_line)
        );
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
