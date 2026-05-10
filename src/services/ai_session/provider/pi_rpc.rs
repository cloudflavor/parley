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
        let mut model = None;
        let started_at = Instant::now();
        loop {
            if started_at.elapsed() >= full_timeout {
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
                    if let Some(text) = extract_pi_reply_text(&event) {
                        emit_progress(progress_sender, AiProvider::Pi, "agent", text.as_str());
                        reply.push_str(&text);
                    }
                }
                Some("agent_end") => break,
                Some("tool_call") => {
                    emit_progress(progress_sender, AiProvider::Pi, "tool", event.to_string());
                }
                Some("error") => {
                    return Err(anyhow!("Pi RPC error: {event}"));
                }
                Some(other) => {
                    emit_progress(
                        progress_sender,
                        AiProvider::Pi,
                        "pi",
                        format!("event: {other}"),
                    );
                }
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

    use super::extract_pi_reply_text;

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
}
