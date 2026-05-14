use super::*;
use crate::utils::cast::usize_to_isize_saturating;

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
                KeyCode::Enter | KeyCode::Tab
                    if self.accept_inline_file_reference_line_selection() =>
                {
                    return Ok(());
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
                    self.move_inline_file_mention_selection(-usize_to_isize_saturating(
                        INLINE_FILE_MENTION_MAX_VISIBLE_ROWS,
                    ));
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.move_inline_file_mention_selection(usize_to_isize_saturating(
                        INLINE_FILE_MENTION_MAX_VISIBLE_ROWS,
                    ));
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
        let original_anchor = self.stored_anchor_snapshot_for_row_range(
            row_index, row_index, side, old_line, new_line, None,
        )?;

        Some(CommentTarget {
            side,
            old_line,
            new_line,
            line_range: None,
            file_path: file.path.clone(),
            line_anchor,
            original_anchor: Box::new(original_anchor),
        })
    }

    pub(super) fn toggle_inline_comment_for_selected_line(&mut self) {
        if let Some((start_row, end_row)) =
            self.comment_selection_row_range_for_pane(self.active_diff_pane)
        {
            self.toggle_inline_comment_for_row_range(start_row, end_row);
        } else {
            self.toggle_inline_comment_for_row(self.active_line_index());
        }
    }

    fn toggle_inline_comment_for_row(&mut self, row_index: usize) {
        self.toggle_inline_comment_for_row_range(row_index, row_index);
    }

    fn toggle_inline_comment_for_row_range(&mut self, start_row: usize, end_row: usize) {
        if let Some(inline) = self.inline_comment.as_ref()
            && inline.row_index == end_row
            && matches!(inline.mode, InlineDraftMode::Comment(_))
        {
            self.inline_comment = None;
            self.status_line = "comment box collapsed".into();
            return;
        }

        let Some(target) = self.comment_target_for_row_range(start_row, end_row) else {
            self.inline_comment = None;
            self.status_line = "selected line cannot receive comments".into();
            return;
        };
        let range_label = format_comment_target_reference(&target);

        self.inline_comment = Some(InlineCommentState {
            row_index: end_row,
            mode: InlineDraftMode::Comment(target),
            buffer: TextBuffer::new(),
            preview_mode: false,
            file_mention: None,
            file_reference_picker: None,
        });
        self.status_line = format!("comment box expanded at {range_label}");
    }

    fn comment_target_for_row_range(
        &self,
        start_row: usize,
        end_row: usize,
    ) -> Option<CommentTarget> {
        if start_row == end_row {
            return self.comment_target_for_row(start_row);
        }

        let file = self.current_file()?;
        let rows = self.current_rows();
        let range_start = start_row.min(end_row);
        let range_end = start_row.max(end_row);
        let mut first_target: Option<(usize, DiffSide, Option<u32>, Option<u32>)> = None;
        let mut old_lines = Vec::new();
        let mut new_lines = Vec::new();

        for row_index in range_start..=range_end {
            let Some(row) = rows.get(row_index) else {
                continue;
            };
            let Some((side, old_line, new_line)) = comment_anchor_for_row(row) else {
                continue;
            };
            if first_target.is_none() {
                first_target = Some((row_index, side, old_line, new_line));
            }
            if let Some(old_line) = old_line {
                old_lines.push(old_line);
            }
            if let Some(new_line) = new_line {
                new_lines.push(new_line);
            }
        }

        let (anchor_row, side, old_line, new_line) = first_target?;
        let line_anchor = self.line_anchor_snapshot_for_row(anchor_row)?;
        let line_range = Some(CommentLineRange {
            start_old_line: old_lines.iter().min().copied(),
            start_new_line: new_lines.iter().min().copied(),
            end_old_line: old_lines.iter().max().copied(),
            end_new_line: new_lines.iter().max().copied(),
        });
        let original_anchor = self.stored_anchor_snapshot_for_row_range(
            range_start,
            range_end,
            side,
            old_line,
            new_line,
            line_range.clone(),
        )?;
        Some(CommentTarget {
            side,
            old_line,
            new_line,
            line_range,
            file_path: file.path.clone(),
            line_anchor,
            original_anchor: Box::new(original_anchor),
        })
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
            .filter(|(_, comment)| {
                self.comment_matches_current_projection(comment, row)
                    || self.comment_line_range_contains_current_projection(comment, row)
            })
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
                            line_range: target.line_range,
                            side: target.side,
                            line_anchor: Some(target.line_anchor),
                            original_anchor: Some(*target.original_anchor),
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
                    old_line.map_or_else(|| "_".to_string(), |value| value.to_string()),
                    new_line.map_or_else(|| "_".to_string(), |value| value.to_string())
                );
            }
        }
        self.reload_review(service).await?;
        if let Some(comment_id) = select_comment_id {
            self.select_comment_by_id(comment_id);
        }
        self.clear_comment_line_selection();
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
                    line_range: None,
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

fn comment_anchor_for_row(row: &DisplayRow) -> Option<(DiffSide, Option<u32>, Option<u32>)> {
    match row.kind {
        DiffLineKind::Added => Some((DiffSide::Right, None, row.new_line)),
        DiffLineKind::Removed => Some((DiffSide::Left, row.old_line, None)),
        DiffLineKind::Context => Some((DiffSide::Right, row.old_line, row.new_line)),
        _ => None,
    }
}

fn format_comment_target_reference(target: &CommentTarget) -> String {
    target.line_range.as_ref().map_or_else(
        || format_line_reference(target.old_line, target.new_line),
        format_line_range_reference,
    )
}
