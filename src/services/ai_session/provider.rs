use super::AiProgressEvent;
use super::progress::emit_progress;
use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::{
    AgentTransport, AppConfig, ProviderTransport, acp_command_replacement,
};
use crate::utils::time::now_ms;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{debug, info, warn};

mod acp;
mod pi_rpc;

#[derive(Debug, Clone)]
pub(super) struct ProviderInvocation {
    pub(super) reply: String,
    pub(super) model: Option<String>,
}

pub(super) async fn invoke_provider(
    config: &AppConfig,
    provider: AiProvider,
    transport: Option<AgentTransport>,
    mode: AiSessionMode,
    prompt: &str,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
) -> Result<ProviderInvocation> {
    let effective_transport = transport.or(config.ai.default_transport);
    let provider_cfg = config
        .ai
        .provider_config_for_transport(provider, effective_transport);
    if provider_cfg.client.trim().is_empty() {
        return Err(anyhow!(
            "provider {} has no configured client in config.toml",
            provider.as_str()
        ));
    }

    match provider_cfg.transport {
        ProviderTransport::Acp => {
            validate_acp_provider_command(provider, &provider_cfg)?;
            return acp::invoke_acp_provider(
                provider,
                &provider_cfg,
                mode,
                prompt,
                effective_timeout_seconds(config, mode),
                progress_sender,
            )
            .await;
        }
        ProviderTransport::PiRpc => {
            return pi_rpc::invoke_pi_rpc_provider(
                &provider_cfg,
                mode,
                prompt,
                effective_timeout_seconds(config, mode),
                progress_sender,
            )
            .await;
        }
        ProviderTransport::Cli => {}
    }

    let mut command = Command::new(&provider_cfg.client);
    command.kill_on_drop(true);
    let args = normalized_provider_args(provider, &provider_cfg, mode);
    command.args(&args);
    let codex_output_path = codex_output_path(provider)?;
    if let Some(path) = codex_output_path.as_ref() {
        if !args.iter().any(|arg| arg == "--json") {
            command.arg("--json");
        }
        command.arg("--output-last-message");
        command.arg(path);
    }
    let configured_model = provider_cfg
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(model_value) = configured_model.as_deref() {
        match provider_cfg.model_arg.as_deref().map(str::trim) {
            Some(model_arg) if !model_arg.is_empty() => {
                command.arg(model_arg);
                command.arg(model_value);
            }
            _ => {
                command.arg(model_value);
            }
        }
    }
    command
        .arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start provider client '{}'", provider_cfg.client))?;
    debug!(
        provider = %provider.as_str(),
        client = %provider_cfg.client,
        prompt_chars = prompt.chars().count(),
        "provider process spawned"
    );
    emit_progress(
        progress_sender.as_ref(),
        provider,
        "system",
        format!(
            "spawned {} (mode={}, transport={})",
            provider_cfg.client,
            mode.as_str(),
            "argv"
        ),
    );

    let stdout_task = child.stdout.take().map(|stdout| {
        tokio::spawn(read_stream(
            stdout,
            provider,
            "stdout",
            progress_sender.clone(),
        ))
    });
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(read_stream(
            stderr,
            provider,
            "stderr",
            progress_sender.clone(),
        ))
    });

    let timeout_seconds = effective_timeout_seconds(config, mode);
    let wait_result = timeout(Duration::from_secs(timeout_seconds), child.wait()).await;
    let mut timed_out = false;
    let status = match wait_result {
        Ok(Ok(status)) => Some(status),
        Ok(Err(error)) => return Err(anyhow!("provider process wait failed: {error}")),
        Err(_) => {
            timed_out = true;
            let _ = child.kill().await;
            None
        }
    };

    let stdout = collect_stream_output(stdout_task).await;
    let stderr = collect_stream_output(stderr_task).await;
    let stderr_trimmed = stderr.trim().to_string();
    let maybe_codex_reply = read_codex_output_last_message(codex_output_path.as_deref()).await?;

    if timed_out {
        let reply = maybe_codex_reply
            .as_deref()
            .unwrap_or(stdout.trim())
            .trim()
            .to_string();
        if !reply.is_empty() {
            warn!(
                provider = %provider.as_str(),
                mode = %mode.as_str(),
                timeout_seconds,
                "provider timed out but returned partial output"
            );
            emit_progress(
                progress_sender.as_ref(),
                provider,
                "system",
                format!("timeout after {timeout_seconds}s, returning partial output"),
            );
            return Ok(ProviderInvocation {
                reply,
                model: detect_runtime_model(provider, &stdout, &stderr)
                    .or(configured_model.clone()),
            });
        }

        emit_progress(
            progress_sender.as_ref(),
            provider,
            "system",
            format!("timeout after {timeout_seconds}s with no output"),
        );
        return Err(anyhow!(
            "provider {} timed out after {}s{}",
            provider.as_str(),
            timeout_seconds,
            if stderr_trimmed.is_empty() {
                "".to_string()
            } else {
                format!(": {stderr_trimmed}")
            }
        ));
    }
    let status = status.ok_or_else(|| anyhow!("provider status unavailable"))?;

    if !status.success() {
        warn!(
            provider = %provider.as_str(),
            status = %status,
            stderr = %stderr_trimmed,
            "provider exited with non-zero status"
        );
        emit_progress(
            progress_sender.as_ref(),
            provider,
            "system",
            format!("provider exited with {status}: {stderr_trimmed}"),
        );
        return Err(anyhow!(
            "provider exited with {}: {}",
            status,
            if stderr_trimmed.is_empty() {
                "no stderr output".to_string()
            } else {
                stderr_trimmed
            }
        ));
    }

    let reply = maybe_codex_reply.unwrap_or_else(|| stdout.trim().to_string());
    if reply.is_empty() {
        warn!(provider = %provider.as_str(), "provider returned empty output");
        emit_progress(
            progress_sender.as_ref(),
            provider,
            "system",
            "provider returned empty output",
        );
        return Err(anyhow!("provider returned empty output"));
    }

    emit_progress(
        progress_sender.as_ref(),
        provider,
        "system",
        "provider completed successfully",
    );
    Ok(ProviderInvocation {
        reply,
        model: detect_runtime_model(provider, &stdout, &stderr).or(configured_model),
    })
}

fn validate_acp_provider_command(
    provider: AiProvider,
    provider_cfg: &crate::domain::config::AiProviderConfig,
) -> Result<()> {
    if let Some(expected_command) = acp_command_replacement(provider, provider_cfg) {
        return Err(anyhow!(
            "provider {} is configured for ACP but '{}' is not an ACP command; use '{}' for ACP or set transport = \"cli\" for one-shot CLI mode",
            provider.as_str(),
            provider_cfg.command_label(),
            expected_command
        ));
    }

    Ok(())
}

pub(super) fn format_ai_reply_body(model: Option<&str>, reply: &str) -> String {
    let mut out = String::new();
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        out.push_str(&format!("Model: {model}\n\n"));
    }
    out.push_str(reply.trim_end());
    out
}

fn detect_runtime_model(provider: AiProvider, stdout: &str, stderr: &str) -> Option<String> {
    match provider {
        AiProvider::Codex => detect_model_from_json_stream(stdout)
            .or_else(|| detect_model_from_json_stream(stderr))
            .or_else(|| detect_model_from_text(stdout))
            .or_else(|| detect_model_from_text(stderr)),
        AiProvider::Claude | AiProvider::Opencode | AiProvider::Pi => {
            detect_model_from_text(stdout).or_else(|| detect_model_from_text(stderr))
        }
    }
}

pub(super) fn detect_model_from_json_stream(stream: &str) -> Option<String> {
    for line in stream.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(model) = extract_model_from_json(&value) {
            return Some(model);
        }
    }
    None
}

fn extract_model_from_json(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in [
                "model",
                "model_id",
                "model_slug",
                "resolved_model",
                "selected_model",
            ] {
                if let Some(Value::String(found)) = map.get(key) {
                    let trimmed = found.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            for nested in map.values() {
                if let Some(found) = extract_model_from_json(nested) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(found) = extract_model_from_json(item) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

pub(super) fn detect_model_from_text(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(value) = extract_model_after_marker(line, "model:") {
            return Some(value);
        }
        if let Some(value) = extract_model_after_marker(line, "model=") {
            return Some(value);
        }
    }
    None
}

fn extract_model_after_marker(line: &str, marker: &str) -> Option<String> {
    let (_, right) = line.split_once(marker)?;
    let candidate = right.split_whitespace().next().map(|value| {
        value.trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == ',' || ch == ';')
    })?;
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

fn normalized_provider_args(
    provider: AiProvider,
    provider_cfg: &crate::domain::config::AiProviderConfig,
    mode: AiSessionMode,
) -> Vec<String> {
    let mut args = provider_cfg.args.clone();
    match provider {
        AiProvider::Codex => {
            if args.first().is_none_or(|value| value != "exec") {
                args.insert(0, "exec".to_string());
            }
            if !args.iter().any(|arg| arg == "--full-auto") {
                args.push("--full-auto".to_string());
            }
            let has_sandbox_flag = args.iter().any(|arg| arg == "--sandbox" || arg == "-s");
            if !has_sandbox_flag {
                args.push("--sandbox".to_string());
                args.push(match mode {
                    AiSessionMode::Reply => "read-only".to_string(),
                    AiSessionMode::Refactor => "workspace-write".to_string(),
                });
            }
        }
        AiProvider::Claude => {
            if !args.iter().any(|arg| arg == "-p" || arg == "--print") {
                args.insert(0, "-p".to_string());
            }
        }
        AiProvider::Opencode => {
            if args.first().is_none_or(|value| value != "run") {
                args.insert(0, "run".to_string());
            }
        }
        AiProvider::Pi => {}
    }
    args
}

fn codex_output_path(provider: AiProvider) -> Result<Option<std::path::PathBuf>> {
    if !matches!(provider, AiProvider::Codex) {
        return Ok(None);
    }
    let file = format!("parley-codex-last-{}-{}.txt", now_ms()?, std::process::id());
    Ok(Some(std::env::temp_dir().join(file)))
}

async fn read_codex_output_last_message(path: Option<&std::path::Path>) -> Result<Option<String>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = match fs::read_to_string(path).await {
        Ok(content) => content.trim().to_string(),
        Err(_) => String::new(),
    };
    let _ = fs::remove_file(path).await;
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

async fn read_stream<R>(
    reader: R,
    provider: AiProvider,
    stream: &'static str,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
) -> String
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    let mut out = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        info!(provider = %provider.as_str(), stream, payload = %line, "provider_stream");
        emit_progress(progress_sender.as_ref(), provider, stream, line.as_str());
        out.push_str(&line);
        out.push('\n');
    }
    out
}

async fn collect_stream_output(task: Option<JoinHandle<String>>) -> String {
    let Some(task) = task else {
        return String::new();
    };
    match task.await {
        Ok(content) => content,
        Err(error) => format!("<stream task join failed: {error}>"),
    }
}

fn effective_timeout_seconds(config: &AppConfig, mode: AiSessionMode) -> u64 {
    let configured = config.ai.timeout_seconds.max(1);
    match mode {
        AiSessionMode::Reply => configured,
        // Refactor mode can involve tool execution and file edits; keep a higher floor.
        AiSessionMode::Refactor => configured.max(600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::AiProviderConfig;

    #[test]
    fn acp_validation_rejects_cli_commands_for_acp_transport() {
        let mut codex = AiProviderConfig::with_client("codex");
        codex.args = vec!["exec".to_string()];

        let mut claude = AiProviderConfig::with_client("claude");
        claude.args = vec!["-p".to_string()];

        let mut opencode = AiProviderConfig::with_client("opencode");
        opencode.args = vec!["run".to_string()];

        assert!(validate_acp_provider_command(AiProvider::Codex, &codex).is_err());
        assert!(validate_acp_provider_command(AiProvider::Claude, &claude).is_err());
        assert!(validate_acp_provider_command(AiProvider::Opencode, &opencode).is_err());
    }

    #[test]
    fn acp_validation_accepts_documented_acp_commands() {
        let codex = AiProviderConfig::with_client("codex-acp");

        let claude = AiProviderConfig::with_client("claude-agent-acp");

        let mut opencode = AiProviderConfig::with_client("opencode");
        opencode.args = vec!["acp".to_string()];

        assert!(validate_acp_provider_command(AiProvider::Codex, &codex).is_ok());
        assert!(validate_acp_provider_command(AiProvider::Claude, &claude).is_ok());
        assert!(validate_acp_provider_command(AiProvider::Opencode, &opencode).is_ok());
    }
}
