use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use include_dir::{Dir, include_dir};
use serde::Serialize;
use serde_json::Value;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::{AppConfig, PromptTransport};
use crate::domain::diff::{DiffDocument, DiffFile, DiffHunk};
use crate::domain::reference::parse_file_references;
use crate::domain::review::{Author, CommentStatus, LineComment, ReviewState};
use crate::git::diff::{DiffSource, load_git_diff};
use crate::services::review_service::{AddReplyInput, ReviewService};

static AI_SESSION_PROMPTS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/prompts/ai_session");

#[derive(Debug, Clone)]
pub struct RunAiSessionInput {
    pub review_name: String,
    pub provider: AiProvider,
    pub comment_ids: Vec<u64>,
    pub mode: AiSessionMode,
    pub diff_source: DiffSource,
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

#[derive(Debug, Clone)]
struct ProviderInvocation {
    reply: String,
    model: Option<String>,
}

pub fn default_ai_session_mode(comment_ids: &[u64]) -> AiSessionMode {
    if comment_ids.is_empty() {
        AiSessionMode::Refactor
    } else {
        AiSessionMode::Reply
    }
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
    let diff_document = match load_git_diff(&config, &input.diff_source).await {
        Ok(document) => Some(document),
        Err(error) => {
            warn!(error = %error, "ai session prompt context: unable to load git diff");
            None
        }
    };
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

    let target_ids: Vec<u64> = if input.comment_ids.is_empty() {
        review
            .comments
            .iter()
            .filter(|comment| comment_is_targetable(comment.status.clone(), input.mode))
            .map(|comment| comment.id)
            .collect()
    } else {
        input.comment_ids.clone()
    };
    let total_targets = target_ids.len();
    if total_targets == 0 {
        result.items.push(AiSessionItemResult {
            comment_id: 0,
            status: "skipped".to_string(),
            message: match input.mode {
                AiSessionMode::Reply => "no replyable threads to process".to_string(),
                AiSessionMode::Refactor => "no open threads to process".to_string(),
            },
        });
        result.skipped = 1;
        emit_progress(
            progress_sender.as_ref(),
            input.provider,
            "system",
            "no open threads to process",
        );
        return Ok(result);
    }

    let explicit_selection = !input.comment_ids.is_empty();
    for (step_index, comment_id) in target_ids.into_iter().enumerate() {
        emit_progress(
            progress_sender.as_ref(),
            input.provider,
            "system",
            format!(
                "thread #{comment_id}: start ({}/{})",
                step_index + 1,
                total_targets
            ),
        );
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
            emit_progress(
                progress_sender.as_ref(),
                input.provider,
                "system",
                format!("thread #{comment_id}: failed (comment not found)"),
            );
            continue;
        };

        let allow_selected_reply = explicit_selection && matches!(input.mode, AiSessionMode::Reply);
        if !comment_is_targetable(comment.status.clone(), input.mode) && !allow_selected_reply {
            debug!(
                review = %input.review_name,
                provider = %input.provider.as_str(),
                comment_id,
                status = ?comment.status,
                "skipping non-targetable comment for selected mode"
            );
            result.skipped += 1;
            result.items.push(AiSessionItemResult {
                comment_id,
                status: "skipped".to_string(),
                message: format!(
                    "comment status {:?} is not targetable for {} mode",
                    comment.status,
                    input.mode.as_str()
                ),
            });
            emit_progress(
                progress_sender.as_ref(),
                input.provider,
                "system",
                format!(
                    "thread #{comment_id}: skipped (status={:?})",
                    comment.status
                ),
            );
            continue;
        }

        let prompt = build_thread_prompt(
            &input.review_name,
            comment_id,
            &review,
            diff_document.as_ref(),
            input.mode,
        );
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
                emit_progress(
                    progress_sender.as_ref(),
                    input.provider,
                    "system",
                    format!("thread #{comment_id}: failed ({error})"),
                );
                continue;
            }
        };
        let reply_body =
            format_ai_reply_body(provider_reply.model.as_deref(), &provider_reply.reply);

        let updated = match service
            .add_reply(
                &input.review_name,
                AddReplyInput {
                    comment_id,
                    author: Author::Ai,
                    body: reply_body,
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
                emit_progress(
                    progress_sender.as_ref(),
                    input.provider,
                    "system",
                    format!("thread #{comment_id}: failed (persist reply: {error})"),
                );
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
            message: match input.mode {
                AiSessionMode::Reply => "ai reply added".to_string(),
                AiSessionMode::Refactor => {
                    "ai reply added; thread status moved to pending_human".to_string()
                }
            },
        });
        emit_progress(
            progress_sender.as_ref(),
            input.provider,
            "system",
            format!(
                "thread #{comment_id}: done ({}/{})",
                step_index + 1,
                total_targets
            ),
        );
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
    diff_document: Option<&DiffDocument>,
    mode: AiSessionMode,
) -> String {
    let Some(comment) = review
        .comments
        .iter()
        .find(|comment| comment.id == comment_id)
    else {
        return missing_comment_prompt(review_name, comment_id);
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
    append_target_file_and_diff_context(&mut thread, comment, diff_document);
    append_referenced_files_context(&mut thread, comment);

    match mode {
        AiSessionMode::Reply => {
            thread.push_str(prompt_template("reply_task.md"));
        }
        AiSessionMode::Refactor => {
            thread.push_str(prompt_template("refactor_task.md"));
        }
    }
    thread
}

fn append_target_file_and_diff_context(
    prompt: &mut String,
    comment: &LineComment,
    diff_document: Option<&DiffDocument>,
) {
    prompt.push_str("\n\nPrimary target context:\n");
    let target_line = comment.new_line.or(comment.old_line);
    match target_line {
        Some(line) => {
            prompt.push_str(&format!(
                "- thread anchor: {}:{}\n",
                comment.file_path, line
            ));
            if let Some(resolved) = resolve_workspace_path(&comment.file_path) {
                if let Some(snippet) = file_line_snippet(&resolved, line) {
                    prompt.push_str(&format!(
                        "  file snippet around {}:{}:\n{}",
                        comment.file_path, line, snippet
                    ));
                } else {
                    prompt.push_str("  file snippet: unavailable for requested line\n");
                }
            } else {
                prompt.push_str("  file snippet: file not found in workspace\n");
            }
        }
        None => {
            prompt.push_str(&format!(
                "- thread anchor: {} (line unavailable)\n",
                comment.file_path
            ));
        }
    }

    if let Some(document) = diff_document {
        if let Some(file) = find_diff_file(document, &comment.file_path) {
            if let Some(hunk) = choose_best_hunk(file, comment.old_line, comment.new_line) {
                let excerpt = format_hunk_excerpt(hunk, comment.old_line, comment.new_line, 28);
                prompt.push_str("  nearest diff hunk:\n");
                prompt.push_str(&excerpt);
            } else {
                prompt.push_str("  nearest diff hunk: none for this file\n");
            }
        } else {
            prompt.push_str("  nearest diff hunk: file not present in current git diff\n");
        }
    } else {
        prompt.push_str("  nearest diff hunk: unavailable (failed to load git diff)\n");
    }
}

fn append_referenced_files_context(
    prompt: &mut String,
    comment: &crate::domain::review::LineComment,
) {
    let mut ordered = BTreeSet::new();
    for reference in parse_file_references(&comment.body) {
        ordered.insert((reference.path, reference.line));
    }
    for reply in &comment.replies {
        for reference in parse_file_references(&reply.body) {
            ordered.insert((reference.path, reference.line));
        }
    }
    if ordered.is_empty() {
        return;
    }

    prompt.push_str("\n\nReferenced files from thread mentions:\n");
    for (path, line) in ordered.into_iter().take(8) {
        let marker = if let Some(value) = line {
            format!("{path}:{value}")
        } else {
            path.clone()
        };
        prompt.push_str(&format!("- {marker}\n"));
        if let (Some(value), Some(resolved)) = (line, resolve_workspace_path(&path))
            && let Some(snippet) = file_line_snippet(&resolved, value)
        {
            prompt.push_str(&format!("  context from {}:\n", resolved.display()));
            prompt.push_str(&snippet);
        }
    }
}

fn find_diff_file<'a>(document: &'a DiffDocument, path: &str) -> Option<&'a DiffFile> {
    document.files.iter().find(|file| file.path == path)
}

fn choose_best_hunk(
    file: &DiffFile,
    old_line: Option<u32>,
    new_line: Option<u32>,
) -> Option<&DiffHunk> {
    if file.hunks.is_empty() {
        return None;
    }

    for hunk in &file.hunks {
        if hunk_contains_anchor(hunk, old_line, new_line) {
            return Some(hunk);
        }
    }

    let mut scored = file
        .hunks
        .iter()
        .map(|hunk| (hunk_distance_to_anchor(hunk, old_line, new_line), hunk))
        .collect::<Vec<_>>();
    scored.sort_by_key(|(distance, _)| *distance);
    scored.first().map(|(_, hunk)| *hunk)
}

fn hunk_contains_anchor(hunk: &DiffHunk, old_line: Option<u32>, new_line: Option<u32>) -> bool {
    hunk.lines.iter().any(|line| {
        old_line.is_some() && line.old_line == old_line
            || new_line.is_some() && line.new_line == new_line
    })
}

fn hunk_distance_to_anchor(hunk: &DiffHunk, old_line: Option<u32>, new_line: Option<u32>) -> u32 {
    let mut best = u32::MAX;
    if let Some(target_old) = old_line {
        best = best.min(line_distance(hunk.old_start, target_old));
    }
    if let Some(target_new) = new_line {
        best = best.min(line_distance(hunk.new_start, target_new));
    }
    if best == u32::MAX { 0 } else { best }
}

fn line_distance(base: u32, target: u32) -> u32 {
    base.abs_diff(target)
}

fn format_hunk_excerpt(
    hunk: &DiffHunk,
    old_line: Option<u32>,
    new_line: Option<u32>,
    max_lines: usize,
) -> String {
    if hunk.lines.is_empty() || max_lines == 0 {
        return String::new();
    }
    let center = hunk
        .lines
        .iter()
        .position(|line| {
            old_line.is_some() && line.old_line == old_line
                || new_line.is_some() && line.new_line == new_line
        })
        .unwrap_or(0);
    let half_window = max_lines / 2;
    let mut start = center.saturating_sub(half_window);
    let end = (start + max_lines).min(hunk.lines.len());
    if end - start < max_lines && end == hunk.lines.len() {
        start = end.saturating_sub(max_lines);
    }

    let mut out = String::new();
    for line in &hunk.lines[start..end] {
        out.push_str("    ");
        out.push_str(&line.raw);
        out.push('\n');
    }
    out
}

fn resolve_workspace_path(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        std::env::current_dir().ok()?.join(trimmed)
    };
    if !candidate.is_file() {
        return None;
    }
    Some(candidate)
}

fn file_line_snippet(path: &Path, line: u32) -> Option<String> {
    if line == 0 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let target = usize::try_from(line.saturating_sub(1)).ok()?;
    if target >= lines.len() {
        return None;
    }

    let start = target.saturating_sub(2);
    let end = (target + 3).min(lines.len());
    let mut out = String::new();
    for (idx, content) in lines[start..end].iter().enumerate() {
        let absolute = start + idx + 1;
        out.push_str(&format!("    {absolute:>5} | {content}\n"));
    }
    Some(out)
}

fn prompt_template(path: &str) -> &'static str {
    AI_SESSION_PROMPTS_DIR
        .get_file(path)
        .unwrap_or_else(|| panic!("missing ai session prompt template: {path}"))
        .contents_utf8()
        .unwrap_or_else(|| panic!("invalid utf-8 in ai session prompt template: {path}"))
}

fn missing_comment_prompt(review_name: &str, comment_id: u64) -> String {
    prompt_template("comment_not_found.md")
        .replace("{review_name}", review_name)
        .replace("{comment_id}", &comment_id.to_string())
}

async fn invoke_provider(
    config: &AppConfig,
    provider: AiProvider,
    mode: AiSessionMode,
    prompt: &str,
    progress_sender: Option<mpsc::Sender<AiProgressEvent>>,
) -> Result<ProviderInvocation> {
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

fn format_ai_reply_body(model: Option<&str>, reply: &str) -> String {
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
        AiProvider::Claude | AiProvider::Opencode => {
            detect_model_from_text(stdout).or_else(|| detect_model_from_text(stderr))
        }
    }
}

fn detect_model_from_json_stream(stream: &str) -> Option<String> {
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

fn detect_model_from_text(text: &str) -> Option<String> {
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

fn comment_is_targetable(status: CommentStatus, mode: AiSessionMode) -> bool {
    match mode {
        AiSessionMode::Reply => {
            matches!(status, CommentStatus::Open | CommentStatus::Pending)
        }
        AiSessionMode::Refactor => matches!(status, CommentStatus::Open),
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

fn now_ms() -> Result<u64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before unix epoch")?;
    Ok(elapsed.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::{
        choose_best_hunk, comment_is_targetable, detect_model_from_json_stream,
        detect_model_from_text, format_ai_reply_body, format_hunk_excerpt, hunk_distance_to_anchor,
    };
    use crate::domain::ai::AiSessionMode;
    use crate::domain::diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind};
    use crate::domain::review::CommentStatus;

    #[test]
    fn reply_mode_excludes_addressed_threads() {
        assert!(comment_is_targetable(
            CommentStatus::Open,
            AiSessionMode::Reply
        ));
        assert!(comment_is_targetable(
            CommentStatus::Pending,
            AiSessionMode::Reply
        ));
        assert!(!comment_is_targetable(
            CommentStatus::Addressed,
            AiSessionMode::Reply
        ));
    }

    #[test]
    fn refactor_mode_targets_only_open_threads() {
        assert!(comment_is_targetable(
            CommentStatus::Open,
            AiSessionMode::Refactor
        ));
        assert!(!comment_is_targetable(
            CommentStatus::Pending,
            AiSessionMode::Refactor
        ));
        assert!(!comment_is_targetable(
            CommentStatus::Addressed,
            AiSessionMode::Refactor
        ));
    }

    #[test]
    fn choose_best_hunk_prefers_exact_anchor_match() {
        let file = DiffFile {
            path: "src/lib.rs".to_string(),
            header_lines: Vec::new(),
            hunks: vec![
                make_hunk(
                    "@@ -1,3 +1,3 @@",
                    1,
                    1,
                    vec![line_ctx(1, 1), line_ctx(2, 2)],
                ),
                make_hunk(
                    "@@ -40,3 +40,3 @@",
                    40,
                    40,
                    vec![line_ctx(40, 40), line_ctx(41, 41)],
                ),
            ],
        };

        let chosen = choose_best_hunk(&file, None, Some(41)).expect("hunk should be selected");
        assert_eq!(chosen.new_start, 40);
    }

    #[test]
    fn choose_best_hunk_falls_back_to_nearest_start() {
        let file = DiffFile {
            path: "src/lib.rs".to_string(),
            header_lines: Vec::new(),
            hunks: vec![
                make_hunk("@@ -10,2 +10,2 @@", 10, 10, vec![line_ctx(10, 10)]),
                make_hunk("@@ -80,2 +80,2 @@", 80, 80, vec![line_ctx(80, 80)]),
            ],
        };

        let chosen = choose_best_hunk(&file, None, Some(74)).expect("hunk should be selected");
        assert_eq!(chosen.new_start, 80);
        assert!(hunk_distance_to_anchor(chosen, None, Some(74)) < 10);
    }

    #[test]
    fn hunk_excerpt_contains_anchor_line() {
        let hunk = make_hunk(
            "@@ -20,4 +20,4 @@",
            20,
            20,
            vec![
                line_ctx(20, 20),
                line_add(0, 21, "+let value = 1;"),
                line_ctx(22, 22),
            ],
        );
        let excerpt = format_hunk_excerpt(&hunk, None, Some(21), 8);
        assert!(excerpt.contains("+let value = 1;"));
        assert!(excerpt.contains("@@ -20,4 +20,4 @@"));
    }

    #[test]
    fn ai_reply_body_includes_model_header() {
        let body = format_ai_reply_body(Some("gpt-5.4"), "Implemented fix.");
        assert!(body.starts_with("Model: gpt-5.4"));
        assert!(body.contains("Implemented fix."));
    }

    #[test]
    fn ai_reply_body_omits_header_when_model_unknown() {
        let body = format_ai_reply_body(None, "Implemented fix.");
        assert_eq!(body, "Implemented fix.");
    }

    #[test]
    fn detect_model_from_json_stream_reads_nested_model_slug() {
        let stream = r#"{"event":"meta","payload":{"session":{"model_slug":"gpt-5.4"}}}"#;
        let detected = detect_model_from_json_stream(stream).expect("model should be detected");
        assert_eq!(detected, "gpt-5.4");
    }

    #[test]
    fn detect_model_from_text_reads_model_marker() {
        let detected =
            detect_model_from_text("run complete; model=gpt-5.4; tokens=100").expect("model");
        assert_eq!(detected, "gpt-5.4");
    }

    fn make_hunk(
        header: &str,
        old_start: u32,
        new_start: u32,
        mut extra: Vec<DiffLine>,
    ) -> DiffHunk {
        let mut lines = vec![DiffLine {
            kind: DiffLineKind::HunkHeader,
            old_line: None,
            new_line: None,
            raw: header.to_string(),
            code: header.to_string(),
        }];
        lines.append(&mut extra);
        DiffHunk {
            old_start,
            old_count: 1,
            new_start,
            new_count: 1,
            header: header.to_string(),
            lines,
        }
    }

    fn line_ctx(old: u32, new: u32) -> DiffLine {
        DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(old),
            new_line: Some(new),
            raw: format!(" context {old}:{new}"),
            code: format!("context {old}:{new}"),
        }
    }

    fn line_add(old: u32, new: u32, raw: &str) -> DiffLine {
        DiffLine {
            kind: DiffLineKind::Added,
            old_line: if old == 0 { None } else { Some(old) },
            new_line: Some(new),
            raw: raw.to_string(),
            code: raw.trim_start_matches('+').to_string(),
        }
    }
}
