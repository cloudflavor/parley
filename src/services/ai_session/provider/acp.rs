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
use crate::utils::time::now_ms;

use super::{ProviderInvocation, detect_model_from_text};
use crate::services::ai_session::AiProgressEvent;
use crate::services::ai_session::progress::emit_progress;

type SharedAcpClient = Arc<Mutex<AcpClient>>;

static ACP_CLIENTS: OnceCell<Mutex<HashMap<String, SharedAcpClient>>> = OnceCell::const_new();

struct AcpClient {
    provider: AiProvider,
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::UnboundedReceiver<Value>,
    next_id: u64,
    initialized: bool,
    cwd: PathBuf,
}

pub(super) async fn invoke_acp_provider(
    provider: AiProvider,
    provider_cfg: &AiProviderConfig,
    mode: AiSessionMode,
    prompt: &str,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
) -> Result<ProviderInvocation> {
    let client = client_for(provider, provider_cfg).await?;
    let mut client = client.lock().await;
    client
        .ensure_initialized(progress_sender.as_ref())
        .await
        .context("failed to initialize ACP agent")?;
    let session_id = client
        .new_session(progress_sender.as_ref())
        .await
        .context("failed to create ACP session")?;
    client
        .prompt(&session_id, mode, prompt, progress_sender.as_ref())
        .await
}

async fn client_for(
    provider: AiProvider,
    provider_cfg: &AiProviderConfig,
) -> Result<SharedAcpClient> {
    if provider_cfg.client.trim().is_empty() {
        return Err(anyhow!(
            "provider {} has no configured ACP client in config.toml",
            provider.as_str()
        ));
    }
    let cwd = std::env::current_dir().context("failed to resolve current directory for ACP")?;
    let key = format!(
        "{}:{}:{}:{}",
        provider.as_str(),
        cwd.display(),
        provider_cfg.client,
        provider_cfg.args.join("\u{1f}")
    );
    let clients = ACP_CLIENTS
        .get_or_init(|| async { Mutex::new(HashMap::new()) })
        .await;
    let mut clients = clients.lock().await;
    if let Some(client) = clients.get(&key) {
        return Ok(client.clone());
    }
    let client = Arc::new(Mutex::new(
        AcpClient::spawn(provider, provider_cfg, cwd).await?,
    ));
    clients.insert(key, client.clone());
    Ok(client)
}

impl AcpClient {
    async fn spawn(
        provider: AiProvider,
        provider_cfg: &AiProviderConfig,
        cwd: PathBuf,
    ) -> Result<Self> {
        let mut command = Command::new(&provider_cfg.client);
        command.args(&provider_cfg.args);
        command.current_dir(&cwd);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.kill_on_drop(true);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start ACP client '{}'", provider_cfg.client))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("ACP client stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("ACP client stdout unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!(provider = %provider.as_str(), stream = "stderr", payload = %line, "acp_stream");
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
                        warn!(error = %error, payload = %line, "failed to parse ACP stdout JSON");
                    }
                }
            }
        });
        Ok(Self {
            provider,
            child,
            stdin,
            rx,
            next_id: 1,
            initialized: false,
            cwd,
        })
    }

    async fn ensure_initialized(
        &mut self,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<()> {
        if self.initialized {
            return Ok(());
        }
        let params = json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {
                    "readTextFile": true,
                    "writeTextFile": true
                },
                "terminal": true
            },
            "clientInfo": {
                "name": "parley",
                "title": "Parley",
                "version": env!("CARGO_PKG_VERSION")
            }
        });
        let _ = self.request("initialize", params, progress_sender).await?;
        self.initialized = true;
        emit_progress(progress_sender, self.provider, "system", "ACP initialized");
        Ok(())
    }

    async fn new_session(
        &mut self,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<String> {
        let params = json!({
            "cwd": self.cwd,
            "mcpServers": []
        });
        let result = self.request("session/new", params, progress_sender).await?;
        result
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("ACP session/new response missing sessionId"))
    }

    async fn prompt(
        &mut self,
        session_id: &str,
        mode: AiSessionMode,
        prompt: &str,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<ProviderInvocation> {
        let id = self.next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{
                    "type": "text",
                    "text": prompt
                }]
            }
        });
        self.write_json(request).await?;
        emit_progress(
            progress_sender,
            self.provider,
            "system",
            format!("ACP prompt sent (mode={})", mode.as_str()),
        );

        let mut reply = String::new();
        let mut model = None;
        loop {
            let message = self
                .rx
                .recv()
                .await
                .ok_or_else(|| anyhow!("ACP client stdout closed"))?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("ACP prompt failed: {error}"));
                }
                if let Some(result) = message.get("result") {
                    model = model.or_else(|| detect_model_from_text(&result.to_string()));
                    if reply.trim().is_empty()
                        && let Some(text) = extract_text(result)
                    {
                        reply.push_str(&text);
                    }
                }
                break;
            }
            self.handle_notification(&message, &mut reply, &mut model, progress_sender);
        }

        let reply = reply.trim().to_string();
        if reply.is_empty() {
            return Err(anyhow!("ACP provider returned empty output"));
        }
        Ok(ProviderInvocation { reply, model })
    }

    async fn request(
        &mut self,
        method: &str,
        params: Value,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<Value> {
        let id = self.next_request_id();
        self.write_json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))
        .await?;
        loop {
            let message = self
                .rx
                .recv()
                .await
                .ok_or_else(|| anyhow!("ACP client stdout closed"))?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("ACP request {method} failed: {error}"));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            let mut sink = String::new();
            let mut model = None;
            self.handle_notification(&message, &mut sink, &mut model, progress_sender);
        }
    }

    fn handle_notification(
        &self,
        message: &Value,
        reply: &mut String,
        model: &mut Option<String>,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) {
        if message.get("method").and_then(Value::as_str) != Some("session/update") {
            return;
        }
        let update = message
            .get("params")
            .and_then(|params| params.get("update"))
            .unwrap_or(message);
        if model.is_none() {
            *model = detect_model_from_text(&update.to_string());
        }
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                if let Some(text) = update.get("content").and_then(extract_text) {
                    emit_progress(progress_sender, self.provider, "agent", text.as_str());
                    reply.push_str(&text);
                }
            }
            Some("thought_chunk") => {
                emit_progress(progress_sender, self.provider, "thought", "thought update");
            }
            Some("tool_call") => {
                emit_progress(progress_sender, self.provider, "tool", update.to_string());
            }
            Some("plan") => {
                emit_progress(progress_sender, self.provider, "plan", update.to_string());
            }
            Some(other) => {
                emit_progress(
                    progress_sender,
                    self.provider,
                    "acp",
                    format!("session update: {other}"),
                );
            }
            None => {}
        }
    }

    async fn write_json(&mut self, value: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&value).context("failed to encode ACP request")?;
        line.push(b'\n');
        self.stdin
            .write_all(&line)
            .await
            .context("failed to write ACP request")?;
        self.stdin.flush().await.ok();
        Ok(())
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => {
            if let Some(Value::String(text)) = map.get("text") {
                return Some(text.clone());
            }
            if let Some(Value::String(text)) = map.get("content") {
                return Some(text.clone());
            }
            for nested in map.values() {
                if let Some(text) = extract_text(nested) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(text) = extract_text(item) {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

#[allow(dead_code)]
fn session_name(provider: AiProvider) -> Result<String> {
    Ok(format!("parley-{}-{}", provider.as_str(), now_ms()?))
}
