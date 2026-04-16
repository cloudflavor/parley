use std::{process::Stdio, sync::mpsc, time::Duration};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    task::JoinHandle,
    time::timeout,
};
use tracing::{debug, error, info, warn};

use crate::{
    domain::{
        ai::{AiProvider, AiSessionMode},
        config::{AppConfig, PromptTransport},
        review::{Author, CommentStatus, ReviewState},
    },
    services::review_service::{AddReplyInput, ReviewService},
};

#[derive(Debug, Clone)]
pub struct RunAiSessionInput {
    pub review_name: String,
    pub provider: AiProvider,
    pub comment_ids: Vec<u64>,
    pub mode: AiSessionMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AiSessionResult {
    pub review_name: String,
    pub provider: String,
    pub mode: String,
    pub client: String,
    pub model: Option<String>,
    pub session_id: String,
    pub processed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<AiSessionItemResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AiSessionItemResult {
    pub comment_id: u64,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AiProgressEvent {
    pub timestamp_ms: u64,
    pub provider: String,
    pub stream: String,
    pub message: String,
}

pub async fn run_ai_session(
    service: &ReviewService,
    input: RunAiSessionInput,
) -> Result<AiSessionResult> {
    run_ai_session_inner(service, input, None).await
}

pub async fn run_ai_session_with_progress(
    service: &ReviewService,
    input: RunAiSessionInput,
    progress_sender: mpsc::Sender<AiProgressEvent>,
) -> Result<AiSessionResult> {
    run_ai_session_inner(service, input, Some(progress_sender)).await
}

async fn run_ai_session_inner(
    service: &ReviewService,
    input: RunAiSessionInput,
    progress_sender: Option<mpsc::Sender<AiProgressEvent>>,
) -> Result<AiSessionResult> {
    info!(
        review = %input.review_name,
        provider = %input.provider.as_str(),
        requested_comments = input.comment_ids.len(),
        "starting ai session"
    );
    let config = service.load_config().await?;
    let mut review = service.load_review(&input.review_name).await?;
    let now_ms = now_ms()?;
    let provider_cfg = config.ai.provider_config(input.provider);
    let mut result = AiSessionResult {
        review_name: input.review_name.clone(),
        provider: input.provider.as_str().to_string(),
        mode: input.mode.as_str().to_string(),
        client: provider_cfg.client.clone(),
        model: provider_cfg.model.clone(),
        session_id: format!("{}-{}-{now_ms}", input.review_name, input.provider.as_str()),
        processed: 0,
        skipped: 0,
        failed: 0,
        items: Vec::new(),
    };

    if matches!(review.state, ReviewState::Done) {
        warn!(
            review = %input.review_name,
            provider = %input.provider.as_str(),
            "ai session skipped because review is done"
        );
        result.items.push(AiSessionItemResult {
            comment_id: 0,
            status: "skipped".to_string(),
            message: "review is done; ai session ignored".to_string(),
        });
        result.skipped = 1;
        return Ok(result);
    }
    if matches!(review.state, ReviewState::Draft) {
        warn!(
            review = %input.review_name,
            provider = %input.provider.as_str(),
            "ai session rejected because review is draft"
        );
        return Err(anyhow!(
            "review is in draft; set review state to pending before running ai session"
        ));
    }
    if matches!(review.state, ReviewState::WaitingForResponse) {
        warn!(
            review = %input.review_name,
            provider = %input.provider.as_str(),
            "ai session skipped because review is waiting_for_response"
        );
        result.items.push(AiSessionItemResult {
            comment_id: 0,
            status: "skipped".to_string(),
            message: "review is waiting_for_response; set it to pending before requesting ai again"
                .to_string(),
        });
        result.skipped = 1;
        return Ok(result);
    }

    let target_ids: Vec<u64> = if input.comment_ids.is_empty() {
        review
            .comments
            .iter()
            .filter(|comment| !matches!(comment.status, CommentStatus::Addressed))
            .map(|comment| comment.id)
            .collect()
    } else {
        input.comment_ids.clone()
    };

    for comment_id in target_ids {
        debug!(
            review = %input.review_name,
            provider = %input.provider.as_str(),
            comment_id,
            "processing ai thread"
        );
        let maybe_comment = review
            .comments
            .iter()
            .find(|comment| comment.id == comment_id);
        let Some(comment) = maybe_comment else {
            warn!(
                review = %input.review_name,
                provider = %input.provider.as_str(),
                comment_id,
                "ai session target comment not found"
            );
            result.failed += 1;
            result.items.push(AiSessionItemResult {
                comment_id,
                status: "failed".to_string(),
                message: "comment not found in review".to_string(),
            });
            continue;
        };

        if matches!(comment.status, CommentStatus::Addressed) {
            debug!(
                review = %input.review_name,
                provider = %input.provider.as_str(),
                comment_id,
                "skipping addressed comment"
            );
            result.skipped += 1;
            result.items.push(AiSessionItemResult {
                comment_id,
                status: "skipped".to_string(),
                message: "comment is resolved/addressed".to_string(),
            });
            continue;
        }

        let prompt = build_thread_prompt(&input.review_name, comment_id, &review, input.mode);
        let provider_reply = match invoke_provider(
            &config,
            input.provider,
            input.mode,
            &prompt,
            progress_sender.clone(),
        )
        .await
        {
            Ok(reply) => reply,
            Err(error) => {
                error!(
                    review = %input.review_name,
                    provider = %input.provider.as_str(),
                    comment_id,
                    error = %error,
                    "provider invocation failed"
                );
                result.failed += 1;
                result.items.push(AiSessionItemResult {
                    comment_id,
                    status: "failed".to_string(),
                    message: format!("provider failed: {error}"),
                });
                continue;
            }
        };

        let updated = match service
            .add_reply(
                &input.review_name,
                AddReplyInput {
                    comment_id,
                    author: Author::Ai,
                    body: provider_reply,
                },
            )
            .await
        {
            Ok(value) => value,
            Err(error) => {
                error!(
                    review = %input.review_name,
                    provider = %input.provider.as_str(),
                    comment_id,
                    error = %error,
                    "failed to persist ai reply"
                );
                result.failed += 1;
                result.items.push(AiSessionItemResult {
                    comment_id,
                    status: "failed".to_string(),
                    message: format!("failed to persist ai reply: {error}"),
                });
                continue;
            }
        };

        review = updated;
        result.processed += 1;
        info!(
            review = %input.review_name,
            provider = %input.provider.as_str(),
            comment_id,
            "ai reply persisted"
        );
        result.items.push(AiSessionItemResult {
            comment_id,
            status: "processed".to_string(),
            message: "ai reply added; thread is pending and review moved to waiting_for_response"
                .to_string(),
        });
    }

    info!(
        review = %input.review_name,
        provider = %input.provider.as_str(),
        processed = result.processed,
        skipped = result.skipped,
        failed = result.failed,
        "ai session completed"
    );
    Ok(result)
}

fn build_thread_prompt(
    review_name: &str,
    comment_id: u64,
    review: &crate::domain::review::ReviewSession,
    mode: AiSessionMode,
) -> String {
    let Some(comment) = review
        .comments
        .iter()
        .find(|comment| comment.id == comment_id)
    else {
        return format!(
            "Review: {review_name}\nComment #{comment_id} was not found. Reply with a brief blocker message."
        );
    };

    let mut thread = String::new();
    thread.push_str(&format!("Review: {review_name}\n"));
    thread.push_str(&format!(
        "Thread comment id: {}\nFile: {}\nLine: {}:{}\nStatus: {:?}\n",
        comment.id,
        comment.file_path,
        comment
            .old_line
            .map(|value| value.to_string())
            .unwrap_or_else(|| "_".to_string()),
        comment
            .new_line
            .map(|value| value.to_string())
            .unwrap_or_else(|| "_".to_string()),
        comment.status
    ));
    thread.push_str("\nOriginal comment:\n");
    thread.push_str(&comment.body);
    thread.push_str("\n\nReplies so far:\n");
    if comment.replies.is_empty() {
        thread.push_str("- (none)\n");
    } else {
        for reply in &comment.replies {
            let author = match reply.author {
                Author::User => "user",
                Author::Ai => "ai",
            };
            thread.push_str(&format!("- {}: {}\n", author, reply.body));
        }
    }

    match mode {
        AiSessionMode::Reply => thread.push_str(
            "\nTask:\n\
             - Address the thread as a code author.\n\
             - Provide a concise markdown reply only (no JSON, no tool output).\n\
             - Do not run commands or inspect files; reply from this thread context only.\n\
             - Do not claim status changes; status is set explicitly by the requester.\n\
             - If blocked, explain exactly what input is missing.\n",
        ),
        AiSessionMode::Refactor => thread.push_str(
            "\nTask:\n\
             - Address this thread by editing code in this workspace.\n\
             - Scope: only the files directly needed for this thread. Do not perform repo-wide cleanup or unrelated refactors.\n\
             - Preserve existing behavior unless the thread explicitly asks for behavior changes.\n\
             - Do not run destructive recovery/version-control commands (`git reset`, `git checkout`, `git clean`, `git fsck`, history rewriting).\n\
             - Do not revert unrelated local changes. Work with the current working tree.\n\
             - Stop after implementing the smallest complete fix for this thread.\n\
             - Reply in concise markdown with exactly these sections:\n\
               1) Changed files\n\
               2) What changed\n\
               3) Validation run\n\
               4) Blockers (only if any)\n\
             - Do not claim status changes; status is set explicitly by the requester.\n\
             - If blocked, explain exactly what input is missing.\n",
        ),
    }
    thread
}

async fn invoke_provider(
    config: &AppConfig,
    provider: AiProvider,
    mode: AiSessionMode,
    prompt: &str,
    progress_sender: Option<mpsc::Sender<AiProgressEvent>>,
) -> Result<String> {
    let provider_cfg = config.ai.provider_config(provider);
    if provider_cfg.client.trim().is_empty() {
        return Err(anyhow!(
            "provider {} has no configured client in config.toml",
            provider.as_str()
        ));
    }

    let mut command = Command::new(&provider_cfg.client);
    command.kill_on_drop(true);
    let args = normalized_provider_args(provider, provider_cfg, mode);
    command.args(&args);
    let codex_output_path = codex_output_path(provider)?;
    if let Some(path) = codex_output_path.as_ref() {
        if !args.iter().any(|arg| arg == "--json") {
            command.arg("--json");
        }
        command.arg("--output-last-message");
        command.arg(path);
    }
    if let Some(model) = provider_cfg.model.as_deref().map(str::trim)
        && !model.is_empty()
    {
        match provider_cfg.model_arg.as_deref().map(str::trim) {
            Some(model_arg) if !model_arg.is_empty() => {
                command.arg(model_arg);
                command.arg(model);
            }
            _ => {
                command.arg(model);
            }
        }
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let prompt_transport = normalized_prompt_transport(provider, &provider_cfg.prompt_transport);
    match prompt_transport {
        PromptTransport::Stdin => {
            command.stdin(Stdio::piped());
        }
        PromptTransport::Argv => {
            command.arg(prompt);
            command.stdin(Stdio::null());
        }
    }

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
            match prompt_transport {
                PromptTransport::Stdin => "stdin",
                PromptTransport::Argv => "argv",
            }
        ),
    );

    if matches!(prompt_transport, PromptTransport::Stdin)
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin
            .write_all(prompt.as_bytes())
            .await
            .context("failed to send prompt to provider stdin")?;
        stdin.flush().await.ok();
    }

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
                format!(
                    "timeout after {}s, returning partial output",
                    timeout_seconds
                ),
            );
            return Ok(reply);
        }

        emit_progress(
            progress_sender.as_ref(),
            provider,
            "system",
            format!("timeout after {}s with no output", timeout_seconds),
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
    let status = status.expect("status is present when not timed out");

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
    Ok(reply)
}

fn normalized_provider_args(
    provider: AiProvider,
    provider_cfg: &crate::domain::config::AiProviderConfig,
    mode: AiSessionMode,
) -> Vec<String> {
    let mut args = provider_cfg.args.clone();
    match provider {
        AiProvider::Codex => {
            if !args.first().map(|value| value == "exec").unwrap_or(false) {
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
            if !args.first().map(|value| value == "run").unwrap_or(false) {
                args.insert(0, "run".to_string());
            }
        }
    }
    args
}

fn codex_output_path(provider: AiProvider) -> Result<Option<std::path::PathBuf>> {
    if !matches!(provider, AiProvider::Codex) {
        return Ok(None);
    }
    let file = format!("parlar-codex-last-{}-{}.txt", now_ms()?, std::process::id());
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
    progress_sender: Option<mpsc::Sender<AiProgressEvent>>,
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

fn normalized_prompt_transport(
    provider: AiProvider,
    configured: &PromptTransport,
) -> PromptTransport {
    let _ = configured;
    match provider {
        // Prefer explicit prompt argv for deterministic headless execution.
        AiProvider::Codex | AiProvider::Claude | AiProvider::Opencode => PromptTransport::Argv,
    }
}

fn emit_progress(
    progress_sender: Option<&mpsc::Sender<AiProgressEvent>>,
    provider: AiProvider,
    stream: &str,
    message: impl Into<String>,
) {
    let Some(progress_sender) = progress_sender else {
        return;
    };
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    let _ = progress_sender.send(AiProgressEvent {
        timestamp_ms,
        provider: provider.as_str().to_string(),
        stream: stream.to_string(),
        message: message.into(),
    });
}

fn effective_timeout_seconds(config: &AppConfig, mode: AiSessionMode) -> u64 {
    let configured = config.ai.timeout_seconds.max(1);
    match mode {
        AiSessionMode::Reply => configured,
        // Refactor mode can involve tool execution and file edits; keep a higher floor.
        AiSessionMode::Refactor => configured.max(600),
    }
}

fn now_ms() -> Result<u64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before unix epoch")?;
    Ok(elapsed.as_millis() as u64)
}
