//! AI session state and operations.
//!
//! Handles AI task lifecycle, progress tracking, and session management.

use std::time::Instant;

use super::*;

impl TuiApp {
    pub(crate) fn dismiss_ai_progress_popup(&mut self) {
        self.ai_progress_visible = false;
    }

    pub(crate) fn toggle_ai_progress_popup(&mut self) {
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

    pub(crate) fn ai_progress_scroll_up(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        self.ai_progress_follow_tail = false;
        self.ai_progress_scroll = self.ai_progress_scroll.saturating_sub(lines);
    }

    pub(crate) fn ai_progress_scroll_down(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        self.ai_progress_scroll = self.ai_progress_scroll.saturating_add(lines);
    }

    pub(crate) fn ai_progress_scroll_home(&mut self) {
        self.ai_progress_follow_tail = false;
        self.ai_progress_scroll = 0;
    }

    pub(crate) fn ai_progress_scroll_end(&mut self) {
        self.ai_progress_follow_tail = true;
        self.ai_progress_scroll = usize::MAX;
    }

    pub(crate) fn ai_progress_resolved_scroll(&mut self, max_scroll: usize) -> usize {
        if self.ai_progress_follow_tail {
            self.ai_progress_scroll = max_scroll;
            return max_scroll;
        }

        let clamped = self.ai_progress_scroll.min(max_scroll);
        self.ai_progress_scroll = clamped;
        clamped
    }

    pub(crate) fn push_ai_progress_line(&mut self, line: String) {
        self.ai_progress_lines.push_back(line);
        while self.ai_progress_lines.len() > AI_PROGRESS_MAX_LINES {
            self.ai_progress_lines.pop_front();
        }
        if self.ai_progress_follow_tail {
            self.ai_progress_scroll = usize::MAX;
        }
    }

    pub(crate) fn record_ai_progress(&mut self, event: AiProgressEvent) -> bool {
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

    pub(crate) fn drain_ai_progress(&mut self) -> bool {
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

    pub(crate) fn cancel_ai_task(&mut self) {
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
            format_timestamp_utc(anchor::now_ms_utc()),
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

    pub(crate) async fn poll_ai_task(&mut self, service: &ReviewService) -> Result<bool> {
        let mut changed = self.drain_ai_progress();

        let Some(task) = self.ai_task.as_ref() else {
            return Ok(changed);
        };
        if !task.handle.is_finished() {
            return Ok(changed);
        }

        let task = self
            .ai_task
            .take()
            .ok_or_else(|| anyhow!("ai task vanished after check"))?;
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
                    format_timestamp_utc(anchor::now_ms_utc()),
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
                    format_timestamp_utc(anchor::now_ms_utc())
                ));
                changed = true;
            }
            Err(error) => {
                self.last_ai_detail = Some(format!("ai task join failed: {error}"));
                self.status_line = format!("run ai session failed: {error}");
                self.push_ai_progress_line(format!(
                    "[{}] system: task join failed: {error}",
                    format_timestamp_utc(anchor::now_ms_utc())
                ));
                changed = true;
            }
        }
        Ok(changed)
    }

    pub(crate) async fn start_ai_session(
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
            format_timestamp_utc(anchor::now_ms_utc()),
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

    pub(crate) async fn cycle_ai_provider(&mut self, service: &ReviewService) -> Result<()> {
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
}
