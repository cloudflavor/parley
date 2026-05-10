//! AI session state and operations.
//!
//! Handles AI task lifecycle, progress tracking, and session management.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;

use super::*;

const AI_TASK_LOG_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

impl TuiApp {
    pub(crate) fn dismiss_ai_progress_popup(&mut self) {
        self.ai_progress_visible = false;
    }

    pub(crate) fn dismiss_ai_activity_overlay(&mut self) {
        self.ai_activity_visible = false;
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
        self.mark_ai_sessions_for_file_read(&self.ai_log_file_path());
        self.status_line = "ai progress popup visible".into();
    }

    pub(crate) fn toggle_ai_activity_overlay(&mut self) {
        if self.ai_activity_visible {
            self.ai_activity_visible = false;
            self.status_line = "ai activity hidden".into();
            return;
        }

        self.dismiss_blocking_overlays();
        self.ai_activity_visible = true;
        self.ai_activity_selected = self
            .ai_activity_selected
            .min(self.ai_activity_entries().len().saturating_sub(1));
        self.status_line = "ai activity visible".into();
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

    pub(crate) fn ai_activity_scroll_up(&mut self, lines: usize) {
        self.ai_activity_scroll = self.ai_activity_scroll.saturating_sub(lines);
    }

    pub(crate) fn ai_activity_scroll_down(&mut self, lines: usize) {
        self.ai_activity_scroll = self.ai_activity_scroll.saturating_add(lines);
    }

    pub(crate) fn ai_activity_select_previous(&mut self) {
        self.ai_activity_selected = self.ai_activity_selected.saturating_sub(1);
    }

    pub(crate) fn ai_activity_select_next(&mut self) {
        let max_index = self.ai_activity_entries().len().saturating_sub(1);
        self.ai_activity_selected = (self.ai_activity_selected + 1).min(max_index);
    }

    pub(crate) fn ai_activity_jump_selected(&mut self) {
        let Some(entry) = self
            .ai_activity_entries()
            .get(self.ai_activity_selected)
            .cloned()
        else {
            self.status_line = "no ai activity selected".into();
            return;
        };
        let Some(file_index) = self
            .diff
            .files
            .iter()
            .position(|file| file.path == entry.file_path)
        else {
            self.status_line = format!("ai activity file no longer visible: {}", entry.file_path);
            return;
        };
        self.select_file(file_index);
        self.mark_ai_sessions_for_file_read(&entry.file_path);
        self.ai_activity_visible = false;
        self.ai_progress_visible = true;
        self.ai_progress_scroll_end();
        self.status_line = format!("opened ai logs for {}", entry.file_path);
    }

    pub(crate) fn ai_activity_entries(&self) -> Vec<AiActivityEntry> {
        let mut entries = self
            .ai_log_sessions_by_file
            .values()
            .flat_map(|sessions| sessions.iter())
            .map(|session| AiActivityEntry {
                session_id: session.id,
                file_path: session.file_path.clone(),
                provider: session.provider,
                mode: session.mode,
                status: session.status,
                started_at_ms: session.started_at_ms,
                finished_at_ms: session.finished_at_ms,
                unread_events: session.unread_events,
                event_count: session.events.len(),
                last_event: session.events.back().cloned(),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .started_at_ms
                .cmp(&left.started_at_ms)
                .then_with(|| right.session_id.cmp(&left.session_id))
        });
        entries
    }

    pub(crate) fn ai_activity_unread_count(&self) -> usize {
        self.ai_log_sessions_by_file
            .values()
            .flat_map(|sessions| sessions.iter())
            .map(|session| session.unread_events)
            .sum()
    }

    pub(crate) fn ai_activity_running_count(&self) -> usize {
        self.ai_log_sessions_by_file
            .values()
            .flat_map(|sessions| sessions.iter())
            .filter(|session| matches!(session.status, AiLogSessionStatus::Running))
            .count()
    }

    pub(crate) fn start_ai_log_session(
        &mut self,
        file_path: &str,
        provider: AiProvider,
        mode: AiSessionMode,
    ) -> u64 {
        let id = self.next_ai_log_session_id;
        self.next_ai_log_session_id = self.next_ai_log_session_id.saturating_add(1);
        let session = AiLogSession {
            id,
            file_path: file_path.to_string(),
            provider,
            mode,
            started_at: Instant::now(),
            started_at_ms: anchor::now_ms_utc(),
            finished_at_ms: None,
            status: AiLogSessionStatus::Running,
            unread_events: 0,
            events: VecDeque::with_capacity(AI_PROGRESS_MAX_LINES),
        };
        let sessions = self
            .ai_log_sessions_by_file
            .entry(file_path.to_string())
            .or_insert_with(|| VecDeque::with_capacity(AI_LOG_MAX_SESSIONS_PER_FILE));
        sessions.push_back(session);
        while sessions.len() > AI_LOG_MAX_SESSIONS_PER_FILE {
            sessions.pop_front();
        }
        self.push_ai_event_for_session(
            id,
            "system",
            format!("started session ({})", mode.as_str()),
        );
        id
    }

    pub(crate) fn push_ai_event_for_session(
        &mut self,
        session_id: u64,
        stream: &str,
        message: impl Into<String>,
    ) {
        let event = AiLogEvent {
            timestamp_ms: anchor::now_ms_utc(),
            stream: stream.to_string(),
            message: message.into(),
        };
        self.push_ai_log_event(session_id, event);
    }

    pub(crate) fn push_ai_log_event(&mut self, session_id: u64, event: AiLogEvent) -> bool {
        let current_file_path = self.ai_log_file_path();
        for sessions in self.ai_log_sessions_by_file.values_mut() {
            let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) else {
                continue;
            };
            session.events.push_back(event);
            while session.events.len() > AI_PROGRESS_MAX_LINES {
                session.events.pop_front();
            }
            if !(self.ai_progress_visible && session.file_path == current_file_path) {
                session.unread_events = session.unread_events.saturating_add(1);
            }
            if self.ai_progress_follow_tail {
                self.ai_progress_scroll = usize::MAX;
            }
            return true;
        }
        false
    }

    pub(crate) fn mark_ai_sessions_for_file_read(&mut self, file_path: &str) {
        if let Some(sessions) = self.ai_log_sessions_by_file.get_mut(file_path) {
            for session in sessions {
                session.unread_events = 0;
            }
        }
    }

    pub(crate) fn update_ai_log_session_status(
        &mut self,
        session_id: u64,
        status: AiLogSessionStatus,
    ) {
        for sessions in self.ai_log_sessions_by_file.values_mut() {
            let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) else {
                continue;
            };
            session.status = status;
            if !matches!(status, AiLogSessionStatus::Running) {
                session.finished_at_ms = Some(anchor::now_ms_utc());
            }
            return;
        }
    }

    pub(crate) fn ai_log_sessions_for_file(
        &self,
        file_path: &str,
    ) -> Option<&VecDeque<AiLogSession>> {
        self.ai_log_sessions_by_file.get(file_path)
    }

    pub(crate) fn ai_log_file_path(&self) -> String {
        self.current_file()
            .map(|file| file.path.clone())
            .unwrap_or_else(|| "(no file)".to_string())
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
        session_id: u64,
        event: AiProgressEvent,
    ) -> bool {
        let Some(message) = Self::normalized_ai_stream_message(&event.stream, &event.message)
        else {
            return false;
        };
        let stream = match event.stream.as_str() {
            "stdout" => "agent",
            other => other,
        };
        let message = if event.stream == "stderr" {
            format!("stderr: {message}")
        } else {
            message
        };
        let pushed = self.push_ai_log_event(
            session_id,
            AiLogEvent {
                timestamp_ms: event.timestamp_ms,
                stream: stream.to_string(),
                message,
            },
        );
        if self.ai_progress_visible && self.ai_log_file_path() == file_path {
            self.mark_ai_sessions_for_file_read(file_path);
        }
        pushed
    }

    pub(crate) fn handle_ai_activity_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('L') => {
                self.dismiss_ai_activity_overlay();
                self.status_line = "ai activity hidden".into();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.ai_activity_select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.ai_activity_select_next();
            }
            KeyCode::PageUp => {
                self.ai_activity_selected = self.ai_activity_selected.saturating_sub(8);
            }
            KeyCode::PageDown => {
                let max_index = self.ai_activity_entries().len().saturating_sub(1);
                self.ai_activity_selected = (self.ai_activity_selected + 8).min(max_index);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.ai_activity_selected = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.ai_activity_selected = self.ai_activity_entries().len().saturating_sub(1);
            }
            KeyCode::Enter => {
                self.ai_activity_jump_selected();
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn drain_ai_progress(&mut self) -> bool {
        let mut changed = false;
        let mut events = Vec::new();
        let mut heartbeats = Vec::new();
        let now = Instant::now();
        for task in &mut self.ai_tasks {
            let file_path = task.file_path.clone();
            let session_id = task.log_session_id;
            while let Ok(event) = task.progress_rx.try_recv() {
                events.push((file_path.clone(), session_id, event));
            }
            if now.duration_since(task.last_log_heartbeat_at) >= AI_TASK_LOG_HEARTBEAT_INTERVAL {
                task.last_log_heartbeat_at = now;
                heartbeats.push((
                    session_id,
                    format!(
                        "waiting for {} {} response ({}s elapsed)",
                        task.provider.as_str(),
                        task.mode.as_str(),
                        task.started_at.elapsed().as_secs()
                    ),
                ));
            }
        }
        for (file_path, session_id, event) in events {
            changed |= self.record_ai_progress_for_file(&file_path, session_id, event);
        }
        for (session_id, message) in heartbeats {
            self.push_ai_event_for_session(session_id, "system", message);
            changed = true;
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
                self.record_ai_progress_for_file(&file_path, task.log_session_id, event);
            }
            let provider = task.provider;
            let elapsed_ms = task.started_at.elapsed().as_millis();
            task.handle.abort();
            self.update_ai_log_session_status(task.log_session_id, AiLogSessionStatus::Cancelled);
            self.push_ai_event_for_session(
                task.log_session_id,
                "system",
                format!("{} cancelled after {}ms", provider.as_str(), elapsed_ms),
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
                self.record_ai_progress_for_file(&file_path, task.log_session_id, event);
            }
            match task.handle.await {
                Ok(Ok(result)) => {
                    refresh_needed = true;
                    let failed = result.items.iter().find(|item| item.status == "failed");
                    let session_status = if result.failed > 0 {
                        AiLogSessionStatus::Failed
                    } else {
                        AiLogSessionStatus::Finished
                    };
                    self.update_ai_log_session_status(task.log_session_id, session_status);
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
                    self.push_ai_event_for_session(
                        task.log_session_id,
                        "system",
                        format!(
                            "{} finished (processed={} skipped={} failed={})",
                            result.provider, result.processed, result.skipped, result.failed
                        ),
                    );
                    changed = true;
                }
                Ok(Err(error)) => {
                    self.update_ai_log_session_status(
                        task.log_session_id,
                        AiLogSessionStatus::Failed,
                    );
                    self.ai_progress_visible = true;
                    self.ai_progress_scroll_end();
                    self.last_ai_detail = Some(format!("ai run failed: {error}"));
                    self.status_line = format!("run ai session failed: {error}");
                    self.push_ai_event_for_session(
                        task.log_session_id,
                        "system",
                        format!("run failed: {error}"),
                    );
                    changed = true;
                }
                Err(error) => {
                    self.update_ai_log_session_status(
                        task.log_session_id,
                        AiLogSessionStatus::Failed,
                    );
                    self.ai_progress_visible = true;
                    self.ai_progress_scroll_end();
                    self.last_ai_detail = Some(format!("ai task join failed: {error}"));
                    self.status_line = format!("run ai session failed: {error}");
                    self.push_ai_event_for_session(
                        task.log_session_id,
                        "system",
                        format!("task join failed: {error}"),
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
        let file_path = self.ai_log_file_path();
        let mut comment_ids = Vec::new();
        if selected_only {
            let Some(comment) = self.selected_comment_details() else {
                self.status_line = "no thread selected".into();
                return Ok(());
            };
            comment_ids.push(comment.id);
        }

        let provider = self.ai_provider;
        let transport = self.ai_transport;
        let log_session_id = self.start_ai_log_session(&file_path, provider, mode);
        self.ai_progress_visible = true;
        self.ai_progress_scroll_end();
        self.mark_ai_sessions_for_file_read(&file_path);
        let provider_config = self
            .config
            .ai
            .provider_config_for_transport(provider, transport);
        self.push_ai_event_for_session(
            log_session_id,
            "system",
            format!(
                "provider={} client={} transport={} model={}",
                provider.as_str(),
                provider_config.client,
                provider_config.transport.as_str(),
                provider_config.model.as_deref().unwrap_or("-")
            ),
        );
        let input = RunAiSessionInput {
            review_name: self.review_name.clone(),
            provider,
            transport,
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
            log_session_id,
            started_at: Instant::now(),
            last_log_heartbeat_at: Instant::now(),
            file_path: file_path.clone(),
            provider,
            mode,
            handle,
            progress_rx,
        });
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
        self.status_line = format!(
            "ai provider set to {} ({})",
            self.ai_provider.as_str(),
            self.effective_ai_transport().as_str()
        );
        Ok(())
    }

    pub(crate) async fn toggle_ai_transport(&mut self, service: &ReviewService) -> Result<()> {
        if matches!(self.ai_provider, AiProvider::Pi) {
            self.ai_transport = None;
            self.config.ai.default_transport = None;
            service.save_config(&self.config).await?;
            self.status_line = "pi uses pi_rpc; ACP/CLI toggle unavailable".into();
            return Ok(());
        }

        let next = match self.effective_ai_transport() {
            AgentTransport::Acp => AgentTransport::Cli,
            AgentTransport::Cli | AgentTransport::PiRpc => AgentTransport::Acp,
        };
        self.ai_transport = Some(next);
        self.config.ai.default_transport = Some(next);
        service.save_config(&self.config).await?;
        let provider_config = self
            .config
            .ai
            .provider_config_for_transport(self.ai_provider, self.ai_transport);
        self.status_line = format!(
            "ai transport set to {} for {} ({})",
            next.as_str(),
            self.ai_provider.as_str(),
            provider_config.client
        );
        Ok(())
    }

    pub(crate) fn effective_ai_transport(&self) -> AgentTransport {
        self.config
            .ai
            .provider_config_for_transport(self.ai_provider, self.ai_transport)
            .transport
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

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;
    use crate::tui::app::state::tests::make_test_app;

    #[test]
    fn ai_log_sessions_keep_events_scoped_to_file_and_session() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], Vec::new())?;

        let session_id =
            app.start_ai_log_session("src/a.rs", AiProvider::Codex, AiSessionMode::Reply);
        app.push_ai_log_event(
            session_id,
            AiLogEvent {
                timestamp_ms: 10,
                stream: "agent".to_string(),
                message: "answer".to_string(),
            },
        );

        let sessions = app
            .ai_log_sessions_for_file("src/a.rs")
            .ok_or_else(|| anyhow::anyhow!("missing file sessions"))?;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].events.len(), 2);
        assert!(app.ai_log_sessions_for_file("src/b.rs").is_none());
        assert_eq!(app.ai_activity_entries()[0].session_id, session_id);
        assert_eq!(app.ai_activity_unread_count(), 2);

        app.mark_ai_sessions_for_file_read("src/a.rs");

        assert_eq!(app.ai_activity_unread_count(), 0);
        Ok(())
    }

    #[test]
    fn ai_activity_jump_opens_selected_file_logs() -> Result<()> {
        let mut app = make_test_app(vec!["src/a.rs", "src/b.rs"], Vec::new())?;
        app.start_ai_log_session("src/b.rs", AiProvider::Opencode, AiSessionMode::Refactor);
        app.ai_activity_visible = true;
        app.ai_activity_selected = 0;

        app.ai_activity_jump_selected();

        assert_eq!(app.active_file_index(), 1);
        assert!(app.ai_progress_visible);
        assert!(!app.ai_activity_visible);
        assert_eq!(app.ai_activity_unread_count(), 0);
        Ok(())
    }
}
