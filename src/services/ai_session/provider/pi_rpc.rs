use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, OnceCell, mpsc};
use tracing::{info, warn};

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::AiProviderConfig;
use crate::services::ai_session::AiProgressEvent;
use crate::services::ai_session::progress::emit_progress;

use super::{ProviderInvocation, detect_model_from_text};

type SharedPiClient = Arc<Mutex<PiRpcClient>>;

static PI_CLIENTS: OnceCell<Mutex<HashMap<String, SharedPiClient>>> = OnceCell::const_new();

struct PiRpcClient {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::UnboundedReceiver<Value>,
}

pub(super) async fn invoke_pi_rpc_provider(
    provider_cfg: &AiProviderConfig,
    mode: AiSessionMode,
    prompt: &str,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
) -> Result<ProviderInvocation> {
    let client = client_for(provider_cfg).await?;
    let mut client = client.lock().await;
    client.prompt(mode, prompt, progress_sender.as_ref()).await
}

async fn client_for(provider_cfg: &AiProviderConfig) -> Result<SharedPiClient> {
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
    let client = Arc::new(Mutex::new(PiRpcClient::spawn(provider_cfg, cwd).await?));
    clients.insert(key, client.clone());
    Ok(client)
}

impl PiRpcClient {
    async fn spawn(provider_cfg: &AiProviderConfig, cwd: PathBuf) -> Result<Self> {
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Pi RPC stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Pi RPC stdout unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!(provider = "pi", stream = "stderr", payload = %line, "pi_rpc_stream");
                }
            });
        }
        let (tx, rx) = mpsc::unbounded_channel();
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
                    }
                }
            }
        });
        Ok(Self { child, stdin, rx })
    }

    async fn prompt(
        &mut self,
        mode: AiSessionMode,
        prompt: &str,
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
        loop {
            let event = self
                .rx
                .recv()
                .await
                .ok_or_else(|| anyhow!("Pi RPC stdout closed"))?;
            if model.is_none() {
                model = detect_model_from_text(&event.to_string());
            }
            match event.get("type").and_then(Value::as_str) {
                Some("message_update") => {
                    if let Some(delta) = event
                        .get("assistantMessageEvent")
                        .and_then(extract_text_delta)
                    {
                        emit_progress(progress_sender, AiProvider::Pi, "agent", delta.as_str());
                        reply.push_str(&delta);
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
