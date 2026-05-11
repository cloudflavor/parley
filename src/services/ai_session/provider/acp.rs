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
use crate::utils::time::now_ms;

use super::{ProviderInvocation, detect_model_from_text};
use crate::services::ai_session::AiProgressEvent;
use crate::services::ai_session::progress::emit_progress;

type SharedAcpClient = Arc<Mutex<AcpClient>>;

static ACP_CLIENTS: OnceCell<Mutex<HashMap<String, SharedAcpClient>>> = OnceCell::const_new();
const ACP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const ACP_PROGRESS_HEARTBEAT: Duration = Duration::from_secs(5);

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
    timeout_seconds: u64,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
) -> Result<ProviderInvocation> {
    let client = client_for(provider, provider_cfg, progress_sender.as_ref()).await?;
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
        .prompt(
            &session_id,
            mode,
            prompt,
            Duration::from_secs(timeout_seconds),
            progress_sender.as_ref(),
        )
        .await
}

async fn client_for(
    provider: AiProvider,
    provider_cfg: &AiProviderConfig,
    progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
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
        AcpClient::spawn(provider, provider_cfg, cwd, progress_sender).await?,
    ));
    clients.insert(key, client.clone());
    Ok(client)
}

impl AcpClient {
    async fn spawn(
        provider: AiProvider,
        provider_cfg: &AiProviderConfig,
        cwd: PathBuf,
        progress_sender: Option<&mpsc::UnboundedSender<AiProgressEvent>>,
    ) -> Result<Self> {
        emit_progress(
            progress_sender,
            provider,
            "system",
            format!(
                "starting ACP client: {} {}",
                provider_cfg.client,
                provider_cfg.args.join(" ")
            ),
        );
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
        emit_progress(
            progress_sender,
            provider,
            "system",
            format!("ACP process spawned pid={:?}", child.id()),
        );
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("ACP client stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("ACP client stdout unavailable"))?;
        if let Some(stderr) = child.stderr.take() {
            let progress_sender = progress_sender.cloned();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    info!(provider = %provider.as_str(), stream = "stderr", payload = %line, "acp_stream");
                    emit_progress(progress_sender.as_ref(), provider, "stderr", line);
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
                        warn!(error = %error, payload = %line, "failed to parse ACP stdout JSON");
                        emit_progress(
                            parse_progress_sender.as_ref(),
                            provider,
                            "stderr",
                            format!("ACP stdout was not JSON: {line}"),
                        );
                    }
                }
            }
            emit_progress(
                parse_progress_sender.as_ref(),
                provider,
                "system",
                "ACP stdout closed",
            );
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
        emit_progress(
            progress_sender,
            self.provider,
            "system",
            "ACP initialize started",
        );
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
        full_timeout: Duration,
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
        emit_progress(
            progress_sender,
            self.provider,
            "acp",
            format!("-> {}", compact_json_for_log(&request)),
        );
        self.write_json(request).await?;
        emit_progress(
            progress_sender,
            self.provider,
            "system",
            format!("ACP prompt sent (mode={})", mode.as_str()),
        );

        let mut reply = String::new();
        let mut model = None;
        let started_at = Instant::now();
        loop {
            if started_at.elapsed() >= full_timeout {
                return Err(anyhow!(
                    "ACP prompt timed out after {}s",
                    full_timeout.as_secs()
                ));
            }
            let remaining = full_timeout.saturating_sub(started_at.elapsed());
            let wait_for = remaining.min(ACP_PROGRESS_HEARTBEAT);
            let message = match timeout(wait_for, self.rx.recv()).await {
                Ok(Some(message)) => message,
                Ok(None) => return Err(anyhow!("ACP client stdout closed")),
                Err(_) => {
                    emit_progress(
                        progress_sender,
                        self.provider,
                        "system",
                        format!(
                            "waiting for ACP prompt response ({}s elapsed)",
                            started_at.elapsed().as_secs()
                        ),
                    );
                    continue;
                }
            };
            emit_progress(
                progress_sender,
                self.provider,
                "acp",
                format!("<- {}", compact_json_for_log(&message)),
            );
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
        emit_progress(
            progress_sender,
            self.provider,
            "system",
            format!("ACP request started: {method}"),
        );
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        emit_progress(
            progress_sender,
            self.provider,
            "acp",
            format!("-> {}", compact_json_for_log(&request)),
        );
        self.write_json(request).await?;
        loop {
            let message = match timeout(ACP_REQUEST_TIMEOUT, self.rx.recv()).await {
                Ok(Some(message)) => message,
                Ok(None) => return Err(anyhow!("ACP client stdout closed during {method}")),
                Err(_) => {
                    return Err(anyhow!(
                        "ACP request {method} timed out after {}s",
                        ACP_REQUEST_TIMEOUT.as_secs()
                    ));
                }
            };
            emit_progress(
                progress_sender,
                self.provider,
                "acp",
                format!("<- {}", compact_json_for_log(&message)),
            );
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
                let text = update
                    .get("content")
                    .and_then(extract_text)
                    .or_else(|| extract_text(update))
                    .unwrap_or_else(|| compact_json_for_log(update));
                emit_progress(progress_sender, self.provider, "thought", text);
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

fn compact_json_for_log(value: &Value) -> String {
    let mut redacted = value.clone();
    redact_prompt_text(&mut redacted);
    serde_json::to_string(&redacted).unwrap_or_else(|_| "<invalid json>".to_string())
}

fn redact_prompt_text(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let looks_like_prompt_item = map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "text")
                && map.contains_key("text");
            if looks_like_prompt_item && let Some(Value::String(text)) = map.get_mut("text") {
                let chars = text.chars().count();
                *text = format!("<redacted prompt: {chars} chars>");
            }
            for nested in map.values_mut() {
                redact_prompt_text(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_prompt_text(item);
            }
        }
        _ => {}
    }
}

#[allow(dead_code)]
fn session_name(provider: AiProvider) -> Result<String> {
    Ok(format!("parley-{}-{}", provider.as_str(), now_ms()?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::extract_text;

    #[test]
    fn extracts_text_from_acp_content_chunk() {
        let update = json!({
            "sessionUpdate": "thought_chunk",
            "content": {
                "type": "text",
                "text": "checking imports"
            }
        });

        assert_eq!(
            update.get("content").and_then(extract_text),
            Some("checking imports".to_string())
        );
    }
}
