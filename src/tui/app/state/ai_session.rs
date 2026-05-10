//! AI session state and operations.
//!
//! Handles AI task lifecycle, progress tracking, and session management.

use std::collections::VecDeque;
use std::time::Instant;

use tokio::sync::mpsc;

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

    pub(crate) fn push_ai_progress_line_for_file(&mut self, file_path: &str, line: String) {
        let lines = self
            .ai_progress_lines_by_file
            .entry(file_path.to_string())
            .or_insert_with(|| VecDeque::with_capacity(AI_PROGRESS_MAX_LINES));
        lines.push_back(line);
        while lines.len() > AI_PROGRESS_MAX_LINES {
            lines.pop_front();
        }
        if self.ai_progress_follow_tail {
            self.ai_progress_scroll = usize::MAX;
        }
    }

    pub(crate) fn ai_log_file_path(&self) -> String {
        self.current_file()
            .map(|file| file.path.clone())
            .unwrap_or_else(|| "(no file)".to_string())
    }

    pub(crate) fn ai_progress_lines_for_file(&self, file_path: &str) -> Option<&VecDeque<String>> {
        self.ai_progress_lines_by_file.get(file_path)
    }

    pub(crate) fn running_ai_tasks_for_file(&self, file_path: &str) -> usize {
        self.ai_tasks
            .iter()
            .filter(|task| task.file_path == file_path)
            .count()
    }

    pub(crate) fn first_running_ai_task_for_file(&self, file_path: &str) -> Option<&AiRunTask> {
        self.ai_tasks
            .iter()
            .find(|task| task.file_path == file_path)
    }

    pub(crate) fn record_ai_progress_for_file(
        &mut self,
        file_path: &str,
        event: AiProgressEvent,
    ) -> bool {
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
        self.push_ai_progress_line_for_file(file_path, line);
        true
    }

    pub(crate) fn drain_ai_progress(&mut self) -> bool {
        let mut changed = false;
        let mut events = Vec::new();
        for task in &mut self.ai_tasks {
            let file_path = task.file_path.clone();
            while let Ok(event) = task.progress_rx.try_recv() {
                events.push((file_path.clone(), event));
            }
        }
        for (file_path, event) in events {
            changed |= self.record_ai_progress_for_file(&file_path, event);
        }
        changed
    }

    pub(crate) fn cancel_ai_task(&mut self) {
        let file_path = self.ai_log_file_path();
        let mut remaining = Vec::with_capacity(self.ai_tasks.len());
        let mut cancelled = Vec::new();
        for task in self.ai_tasks.drain(..) {
            if task.file_path == file_path {
                cancelled.push(task);
            } else {
                remaining.push(task);
            }
        }
        self.ai_tasks = remaining;

        if cancelled.is_empty() {
            self.status_line = "no ai sessions running for current file".into();
            return;
        }

        let cancelled_count = cancelled.len();
        for mut task in cancelled {
            while let Ok(event) = task.progress_rx.try_recv() {
                self.record_ai_progress_for_file(&file_path, event);
            }
            let provider = task.provider;
            let elapsed_ms = task.started_at.elapsed().as_millis();
            task.handle.abort();
            self.push_ai_progress_line_for_file(
                &file_path,
                format!(
                    "[{}] {} system: cancelled after {}ms",
                    format_timestamp_utc(anchor::now_ms_utc()),
                    provider.as_str(),
                    elapsed_ms
                ),
            );
        }
        self.status_line = format!("cancelled {cancelled_count} ai session(s) for current file");
    }

    pub(crate) async fn poll_ai_task(&mut self, service: &ReviewService) -> Result<bool> {
        let mut changed = self.drain_ai_progress();
        let mut finished_indices = self
            .ai_tasks
            .iter()
            .enumerate()
            .filter_map(|(index, task)| task.handle.is_finished().then_some(index))
            .collect::<Vec<_>>();
        if finished_indices.is_empty() {
            return Ok(changed);
        }

        let mut refresh_needed = false;
        finished_indices.reverse();
        for index in finished_indices {
            let mut task = self.ai_tasks.remove(index);
            let file_path = task.file_path.clone();
            while let Ok(event) = task.progress_rx.try_recv() {
                self.record_ai_progress_for_file(&file_path, event);
            }
            match task.handle.await {
                Ok(Ok(result)) => {
                    refresh_needed = true;
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
                    self.push_ai_progress_line_for_file(
                        &file_path,
                        format!(
                            "[{}] {} system: finished (processed={} skipped={} failed={})",
                            format_timestamp_utc(anchor::now_ms_utc()),
                            result.provider,
                            result.processed,
                            result.skipped,
                            result.failed
                        ),
                    );
                    changed = true;
                }
                Ok(Err(error)) => {
                    self.last_ai_detail = Some(format!("ai run failed: {error}"));
                    self.status_line = format!("run ai session failed: {error}");
                    self.push_ai_progress_line_for_file(
                        &file_path,
                        format!(
                            "[{}] system: run failed: {error}",
                            format_timestamp_utc(anchor::now_ms_utc())
                        ),
                    );
                    changed = true;
                }
                Err(error) => {
                    self.last_ai_detail = Some(format!("ai task join failed: {error}"));
                    self.status_line = format!("run ai session failed: {error}");
                    self.push_ai_progress_line_for_file(
                        &file_path,
                        format!(
                            "[{}] system: task join failed: {error}",
                            format_timestamp_utc(anchor::now_ms_utc())
                        ),
                    );
                    changed = true;
                }
            }
        }
        if refresh_needed {
            self.refresh_review_and_diff(service).await?;
        }
        Ok(changed)
    }

    pub(crate) async fn start_ai_session(
        &mut self,
        service: &ReviewService,
        selected_only: bool,
        mode: AiSessionMode,
    ) -> Result<()> {
        if matches!(self.review.state, ReviewState::Done) {
            self.status_line = "review is done; reopen it before running ai".into();
            return Ok(());
        }

        let file_path = self.ai_log_file_path();
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
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            run_ai_session_with_progress(&service_clone, input, progress_tx).await
        });

        self.ai_tasks.push(AiRunTask {
            started_at: Instant::now(),
            file_path: file_path.clone(),
            provider,
            mode,
            handle,
            progress_rx,
        });
        self.push_ai_progress_line_for_file(
            &file_path,
            format!(
                "[{}] {} system: started session ({})",
                format_timestamp_utc(anchor::now_ms_utc()),
                provider.as_str(),
                mode.as_str()
            ),
        );
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
            AiProvider::Opencode => AiProvider::Pi,
            AiProvider::Pi => AiProvider::Codex,
        };
        self.config.ai.default_provider = self.ai_provider;
        service.save_config(&self.config).await?;
        self.status_line = format!("ai provider set to {}", self.ai_provider.as_str());
        Ok(())
    }

    fn normalized_ai_stream_message(stream: &str, message: &str) -> Option<String> {
        if !matches!(stream, "stdout" | "stderr") {
            return Some(message.to_string());
        }

        let trimmed = message.trim();
        if trimmed.is_empty() {
            return None;
        }

        let json_candidate = trimmed.strip_prefix("data:").map_or(trimmed, str::trim);
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_candidate) {
            let parts = Self::collect_ai_text_fragments(&value, None);
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
    ) -> Vec<String> {
        let mut fragments = Vec::new();

        match value {
            serde_json::Value::String(s) => {
                if Self::is_text_field(parent_key) && !s.trim().is_empty() {
                    fragments.push(s.clone());
                }
            }
            serde_json::Value::Object(map) => {
                for (key, val) in map {
                    fragments.extend(Self::collect_ai_text_fragments(val, Some(key.as_str())));
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    fragments.extend(Self::collect_ai_text_fragments(item, parent_key));
                }
            }
            _ => {}
        }

        fragments
    }

    fn is_text_field(parent_key: Option<&str>) -> bool {
        matches!(
            parent_key,
            Some("content" | "text" | "body" | "message" | "reply" | "output" | "input")
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
}
