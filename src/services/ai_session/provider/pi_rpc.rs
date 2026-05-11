use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, OnceCell, mpsc};
use tokio::time::timeout;
use tracing::{info, warn};

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::AiProviderConfig;
use crate::services::ai_session::AiProgressEvent;
use crate::services::ai_session::progress::emit_progress;

use super::{ProviderInvocation, detect_model_from_text};

type SharedPiClient = Arc<Mutex<PiRpcClient>>;

static PI_CLIENTS: OnceCell<Mutex<HashMap<String, SharedPiClient>>> = OnceCell::const_new();
const PI_PROGRESS_HEARTBEAT: Duration = Duration::from_secs(5);

struct PiRpcClient {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::UnboundedReceiver<Value>,
}

pub(super) async fn invoke_pi_rpc_provider(
    provider_cfg: &AiProviderConfig,
    mode: AiSessionMode,
    prompt: &str,
    timeout_seconds: u64,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
) -> Result<ProviderInvocation> {
    let client = client_for(provider_cfg, progress_sender.as_ref()).await?;
    let mut client = client.lock().await;
    client
        .prompt(
            mode,
            prompt,
            Duration::from_secs(timeout_seconds),
            progress_sender.as_ref(),
        )
        .await
}

async fn client_for(
    provider_cfg: &AiProviderConfig,
    progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
) -> Result<SharedPiClient> {
    if provider_cfg.client.trim().is_empty() {
        return Err(anyhow!(
            "provider pi has no configured RPC client in config.toml"
        ));
    }
    let cwd = std::env::current_dir().context("failed to resolve current directory for Pi RPC")?;
    let key = format!(
        "{}:{}:{}",
        cwd.display(),
        provider_cfg.client,
        provider_cfg.args.join("\u{1f}")
    );
    let clients = PI_CLIENTS
        .get_or_init(|| async { Mutex::new(HashMap::new()) })
        .await;
    let mut clients = clients.lock().await;
    if let Some(client) = clients.get(&key) {
        return Ok(client.clone());
    }
    let client = Arc::new(Mutex::new(
        PiRpcClient::spawn(provider_cfg, cwd, progress_sender).await?,
    ));
    clients.insert(key, client.clone());
    Ok(client)
}

impl PiRpcClient {
    async fn spawn(
        provider_cfg: &AiProviderConfig,
        cwd: PathBuf,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<Self> {
        emit_progress(
            progress_sender,
            AiProvider::Pi,
            "system",
            format!(
                "starting Pi RPC client: {} {}",
                provider_cfg.client,
                provider_cfg.args.join(" ")
            ),
        );
        let mut command = Command::new(&provider_cfg.client);
        command.args(&provider_cfg.args);
        command.current_dir(cwd);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.kill_on_drop(true);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start Pi RPC client '{}'", provider_cfg.client))?;
        emit_progress(
            progress_sender,
            AiProvider::Pi,
            "system",
            format!("Pi RPC process spawned pid={:?}", child.id()),
        );
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Pi RPC stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Pi RPC stdout unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            let progress_sender = progress_sender.cloned();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!(provider = "pi", stream = "stderr", payload = %line, "pi_rpc_stream");
                    emit_progress(progress_sender.as_ref(), AiProvider::Pi, "stderr", line);
                }
            });
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let parse_progress_sender = progress_sender.cloned();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(&line) {
                    Ok(value) => {
                        let _ = tx.send(value);
                    }
                    Err(error) => {
                        warn!(error = %error, payload = %line, "failed to parse Pi RPC JSON");
                        emit_progress(
                            parse_progress_sender.as_ref(),
                            AiProvider::Pi,
                            "stderr",
                            format!("Pi RPC stdout was not JSON: {line}"),
                        );
                    }
                }
            }
            emit_progress(
                parse_progress_sender.as_ref(),
                AiProvider::Pi,
                "system",
                "Pi RPC stdout closed",
            );
        });
        Ok(Self { child, stdin, rx })
    }

    async fn prompt(
        &mut self,
        mode: AiSessionMode,
        prompt: &str,
        full_timeout: Duration,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<ProviderInvocation> {
        let request = json!({
            "type": "prompt",
            "message": prompt
        });
        self.write_json(request).await?;
        emit_progress(
            progress_sender,
            AiProvider::Pi,
            "system",
            format!("Pi RPC prompt sent (mode={})", mode.as_str()),
        );
        let mut reply = String::new();
        let mut current_message_reply = String::new();
        let mut pending_agent_log = String::new();
        let mut model = None;
        let started_at = Instant::now();
        loop {
            if started_at.elapsed() >= full_timeout {
                if !reply.trim().is_empty() {
                    emit_progress(
                        progress_sender,
                        AiProvider::Pi,
                        "system",
                        "Pi RPC timed out after final message; returning last assistant reply",
                    );
                    return Ok(ProviderInvocation {
                        reply: reply.trim().to_string(),
                        model,
                    });
                }
                return Err(anyhow!(
                    "Pi RPC prompt timed out after {}s",
                    full_timeout.as_secs()
                ));
            }
            let remaining = full_timeout.saturating_sub(started_at.elapsed());
            let wait_for = remaining.min(PI_PROGRESS_HEARTBEAT);
            let event = match timeout(wait_for, self.rx.recv()).await {
                Ok(Some(event)) => event,
                Ok(None) => return Err(anyhow!("Pi RPC stdout closed")),
                Err(_) => {
                    if !reply.trim().is_empty() {
                        emit_progress(
                            progress_sender,
                            AiProvider::Pi,
                            "system",
                            format!(
                                "waiting for Pi RPC end event after final reply ({}s elapsed)",
                                started_at.elapsed().as_secs()
                            ),
                        );
                        continue;
                    }
                    emit_progress(
                        progress_sender,
                        AiProvider::Pi,
                        "system",
                        format!(
                            "waiting for Pi RPC response ({}s elapsed)",
                            started_at.elapsed().as_secs()
                        ),
                    );
                    continue;
                }
            };
            if model.is_none() {
                model = detect_model_from_text(&event.to_string());
            }
            match event.get("type").and_then(Value::as_str) {
                Some("message_update") => {
                    if let Some(thought) = extract_pi_thought_text(&event) {
                        emit_progress(progress_sender, AiProvider::Pi, "thought", thought);
                    }
                    if let Some(text) = extract_pi_reply_text(&event) {
                        current_message_reply.push_str(&text);
                        pending_agent_log.push_str(&text);
                        if should_flush_pi_agent_log(&pending_agent_log) {
                            flush_pi_agent_log(progress_sender, &mut pending_agent_log);
                        }
                    }
                }
                Some("message_start") => {
                    current_message_reply.clear();
                    pending_agent_log.clear();
                    emit_progress(
                        progress_sender,
                        AiProvider::Pi,
                        "system",
                        "Pi message started",
                    );
                }
                Some("message_end") => {
                    flush_pi_agent_log(progress_sender, &mut pending_agent_log);
                    finish_pi_message_reply(&mut reply, &mut current_message_reply);
                }
                Some("agent_end") => {
                    flush_pi_agent_log(progress_sender, &mut pending_agent_log);
                    finish_pi_message_reply(&mut reply, &mut current_message_reply);
                    break;
                }
                Some("tool_call") => {
                    emit_pi_log_entries(progress_sender, &event);
                }
                Some("error") => {
                    return Err(anyhow!("Pi RPC error: {event}"));
                }
                Some(other) if should_log_pi_event(other) => {
                    if !emit_pi_log_entries(progress_sender, &event) {
                        emit_progress(
                            progress_sender,
                            AiProvider::Pi,
                            "pi",
                            format!("{other}: {}", compact_pi_event_json(&event)),
                        );
                    }
                }
                Some(_) => {}
                None => {}
            }
        }
        let reply = reply.trim().to_string();
        if reply.is_empty() {
            return Err(anyhow!("Pi RPC provider returned empty output"));
        }
        Ok(ProviderInvocation { reply, model })
    }

    async fn write_json(&mut self, value: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&value).context("failed to encode Pi RPC request")?;
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .await
            .context("failed to write Pi RPC request")?;
        self.stdin.flush().await.ok();
        Ok(())
    }
}

impl Drop for PiRpcClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn extract_text_delta(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("text_delta")
                && let Some(delta) = map.get("delta").and_then(Value::as_str)
            {
                return Some(delta.to_string());
            }
            for nested in map.values() {
                if let Some(delta) = extract_text_delta(nested) {
                    return Some(delta);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(delta) = extract_text_delta(item) {
                    return Some(delta);
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_pi_reply_text(value: &Value) -> Option<String> {
    extract_text_delta(value).or_else(|| extract_assistant_text(value))
}

fn extract_pi_thought_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["thought", "thinking", "reasoning", "analysis"] {
                if let Some(text) = map.get(key).and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    return Some(text.to_string());
                }
            }
            for (key, nested) in map {
                if matches!(
                    key.as_str(),
                    "thought" | "thinking" | "reasoning" | "analysis"
                ) && let Some(text) = extract_assistant_text(nested)
                {
                    return Some(text);
                }
                if let Some(text) = extract_pi_thought_text(nested) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(text) = extract_pi_thought_text(item) {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

fn flush_pi_agent_log(
    progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    pending_agent_log: &mut String,
) {
    let text = pending_agent_log.trim();
    if !text.is_empty() {
        emit_progress(progress_sender, AiProvider::Pi, "agent", text);
    }
    pending_agent_log.clear();
}

fn finish_pi_message_reply(reply: &mut String, current_message_reply: &mut String) {
    let trimmed = current_message_reply.trim();
    if !trimmed.is_empty() {
        *reply = trimmed.to_string();
    }
    current_message_reply.clear();
}

fn should_flush_pi_agent_log(text: &str) -> bool {
    let trimmed = text.trim_end();
    text.ends_with('\n')
        || trimmed.chars().count() >= 120
        || trimmed.ends_with('.')
        || trimmed.ends_with('!')
        || trimmed.ends_with('?')
}

fn should_log_pi_event(event_type: &str) -> bool {
    !matches!(
        event_type,
        "turn_start" | "turn_end" | "message_start" | "message_end"
    )
}

fn emit_pi_log_entries(
    progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    event: &Value,
) -> bool {
    let entries = pi_log_entries(event);
    let emitted = !entries.is_empty();
    for entry in entries {
        emit_progress(progress_sender, AiProvider::Pi, entry.stream, entry.message);
    }
    emitted
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PiLogEntry {
    stream: &'static str,
    message: String,
}

fn pi_log_entries(event: &Value) -> Vec<PiLogEntry> {
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("event");
    let mut entries = Vec::new();

    for text in collect_named_text(event, &["thought", "thinking", "reasoning", "analysis"]) {
        entries.push(PiLogEntry {
            stream: "thought",
            message: format!("{event_type}: {text}"),
        });
    }
    for text in collect_named_text(event, &["stdout"]) {
        entries.push(PiLogEntry {
            stream: "stdout",
            message: format!("{event_type}: {text}"),
        });
    }
    for text in collect_named_text(event, &["stderr"]) {
        entries.push(PiLogEntry {
            stream: "stderr",
            message: format!("{event_type}: {text}"),
        });
    }
    if event_type.contains("tool") {
        let detail = first_named_text(
            event,
            &[
                "title",
                "name",
                "tool_name",
                "command",
                "input",
                "output",
                "result",
                "message",
                "content",
                "text",
                "error",
            ],
        )
        .unwrap_or_else(|| compact_pi_event_json(event));
        entries.push(PiLogEntry {
            stream: "tool",
            message: format!("{event_type}: {detail}"),
        });
    } else if event_type.contains("error") {
        let detail = first_named_text(event, &["error", "message", "text"])
            .unwrap_or_else(|| compact_pi_event_json(event));
        entries.push(PiLogEntry {
            stream: "stderr",
            message: format!("{event_type}: {detail}"),
        });
    } else if entries.is_empty()
        && let Some(detail) = first_named_text(event, &["message", "summary", "title"])
    {
        entries.push(PiLogEntry {
            stream: "pi",
            message: format!("{event_type}: {detail}"),
        });
    }

    entries.dedup();
    entries
}

fn collect_named_text(value: &Value, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    collect_named_text_into(value, keys, None, &mut out);
    out
}

fn first_named_text(value: &Value, keys: &[&str]) -> Option<String> {
    collect_named_text(value, keys).into_iter().next()
}

fn collect_named_text_into(
    value: &Value,
    keys: &[&str],
    current_key: Option<&str>,
    out: &mut Vec<String>,
) {
    match value {
        Value::String(text) => {
            let Some(current_key) = current_key else {
                return;
            };
            if keys.iter().any(|key| current_key.eq_ignore_ascii_case(key))
                && !text.trim().is_empty()
            {
                out.push(text.trim().to_string());
            }
        }
        Value::Object(map) => {
            for (key, nested) in map {
                collect_named_text_into(nested, keys, Some(key), out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_named_text_into(item, keys, current_key, out);
            }
        }
        _ => {}
    }
}

fn compact_pi_event_json(event: &Value) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| "<invalid pi event>".to_string())
}

fn extract_assistant_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(role) = map.get("role").and_then(Value::as_str)
                && role != "assistant"
            {
                return None;
            }
            for key in [
                "delta", "text", "content", "message", "body", "reply", "output",
            ] {
                if let Some(text) = map.get(key).and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    return Some(text.to_string());
                }
            }
            for nested in map.values() {
                if let Some(text) = extract_assistant_text(nested) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(text) = extract_assistant_text(item) {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        PiLogEntry, extract_pi_reply_text, extract_pi_thought_text, finish_pi_message_reply,
        pi_log_entries, should_flush_pi_agent_log, should_log_pi_event,
    };

    #[test]
    fn extracts_text_delta_from_pi_message_update() {
        let event = json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "type": "text_delta",
                "delta": "reply text"
            }
        });

        assert_eq!(
            extract_pi_reply_text(&event),
            Some("reply text".to_string())
        );
    }

    #[test]
    fn extracts_assistant_content_from_pi_message_update() {
        let event = json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "role": "assistant",
                "content": "full reply"
            }
        });

        assert_eq!(
            extract_pi_reply_text(&event),
            Some("full reply".to_string())
        );
    }

    #[test]
    fn ignores_user_content_when_extracting_pi_reply_text() {
        let event = json!({
            "type": "message_update",
            "message": {
                "role": "user",
                "content": "not an assistant reply"
            }
        });

        assert_eq!(extract_pi_reply_text(&event), None);
    }

    #[test]
    fn extracts_pi_thought_text_from_nested_event() {
        let event = json!({
            "type": "message_update",
            "assistantMessageEvent": {
                "thinking": {
                    "role": "assistant",
                    "content": "checking imports"
                }
            }
        });

        assert_eq!(
            extract_pi_thought_text(&event),
            Some("checking imports".to_string())
        );
    }

    #[test]
    fn flushes_pi_agent_log_on_sentence_boundary_or_size() {
        assert!(should_flush_pi_agent_log("The imports are already clean."));
        assert!(should_flush_pi_agent_log(&"x".repeat(120)));
        assert!(!should_flush_pi_agent_log("The imports are"));
    }

    #[test]
    fn suppresses_noisy_pi_lifecycle_events() {
        assert!(!should_log_pi_event("message_end"));
        assert!(should_log_pi_event("tool_execution_start"));
        assert!(should_log_pi_event("error"));
    }

    #[test]
    fn extracts_structured_pi_tool_logs() {
        let event = json!({
            "type": "tool_execution_end",
            "tool": {
                "name": "edit",
                "stdout": "patched file",
                "stderr": "warning text"
            }
        });

        assert_eq!(
            pi_log_entries(&event),
            vec![
                PiLogEntry {
                    stream: "stdout",
                    message: "tool_execution_end: patched file".to_string()
                },
                PiLogEntry {
                    stream: "stderr",
                    message: "tool_execution_end: warning text".to_string()
                },
                PiLogEntry {
                    stream: "tool",
                    message: "tool_execution_end: edit".to_string()
                }
            ]
        );
    }

    #[test]
    fn extracts_structured_pi_thought_logs() {
        let event = json!({
            "type": "message_update",
            "delta": {
                "thinking": "checking target thread"
            }
        });

        assert_eq!(
            pi_log_entries(&event),
            vec![PiLogEntry {
                stream: "thought",
                message: "message_update: checking target thread".to_string()
            }]
        );
    }

    #[test]
    fn unknown_significant_pi_event_keeps_payload() {
        let event = json!({
            "type": "custom_event",
            "payload": {
                "id": 7
            }
        });

        assert!(should_log_pi_event("custom_event"));
        assert!(pi_log_entries(&event).is_empty());
    }

    #[test]
    fn pi_reply_keeps_last_assistant_message_only() {
        let mut reply = String::new();
        let mut current = "first tool-planning message".to_string();
        finish_pi_message_reply(&mut reply, &mut current);
        current.push_str("final review reply");
        finish_pi_message_reply(&mut reply, &mut current);

        assert_eq!(reply, "final review reply");
        assert!(current.is_empty());
    }
}
