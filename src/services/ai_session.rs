use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::AgentTransport;
use crate::domain::review::{Author, CommentStatus};
use crate::git::diff::{DiffSource, load_git_diff};
use crate::services::review_service::{AddReplyInput, ReviewService};
use crate::utils::time::now_ms;

mod progress;
mod prompt;
mod provider;

#[cfg(test)]
mod tests;

use progress::emit_progress;
use prompt::{build_thread_prompt, load_task_prompt_override};
use provider::{format_ai_reply_body, invoke_provider};

#[derive(Debug, Clone)]
pub struct RunAiSessionInput {
    pub review_name: String,
    pub provider: AiProvider,
    pub transport: Option<AgentTransport>,
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
    pub transport: String,
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

#[must_use]
pub fn default_ai_session_mode(comment_ids: &[u64]) -> AiSessionMode {
    if comment_ids.is_empty() {
        AiSessionMode::Refactor
    } else {
        AiSessionMode::Reply
    }
}

/// # Errors
///
/// Returns an error when the review/config cannot be loaded, the clock is invalid, provider
/// invocation fails, or review updates cannot be persisted.
pub async fn run_ai_session(
    service: &ReviewService,
    input: RunAiSessionInput,
) -> Result<AiSessionResult> {
    run_ai_session_inner(service, input, None).await
}

/// # Errors
///
/// Returns an error for the same load, provider, clock, and persistence failures as
/// [`run_ai_session`].
pub async fn run_ai_session_with_progress(
    service: &ReviewService,
    input: RunAiSessionInput,
    progress_sender: mpsc::UnboundedSender<AiProgressEvent>,
) -> Result<AiSessionResult> {
    run_ai_session_inner(service, input, Some(progress_sender)).await
}

async fn run_ai_session_inner(
    service: &ReviewService,
    input: RunAiSessionInput,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
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
    let effective_transport = input.transport.or(config.ai.default_transport);
    let provider_cfg = config
        .ai
        .provider_config_for_transport(input.provider, effective_transport);
    let mut result = AiSessionResult {
        review_name: input.review_name.clone(),
        provider: input.provider.as_str().to_string(),
        mode: input.mode.as_str().to_string(),
        transport: provider_cfg.transport.as_str().to_string(),
        client: provider_cfg.client.clone(),
        model: provider_cfg.model.clone(),
        session_id: format!("{}-{}-{now_ms}", input.review_name, input.provider.as_str()),
        processed: 0,
        skipped: 0,
        failed: 0,
        items: Vec::new(),
    };

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

    let task_prompt_override = load_task_prompt_override(&config, input.mode).await?;
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

        if !comment_is_targetable(comment.status.clone(), input.mode) {
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
            task_prompt_override.as_deref(),
        )
        .await?;
        let provider_reply = match invoke_provider(
            &config,
            input.provider,
            input.transport,
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
        let parsed_reply = match parse_ai_thread_reply_json(&provider_reply.reply, comment_id) {
            Ok(parsed_reply) => parsed_reply,
            Err(error) => {
                result.failed += 1;
                result.items.push(AiSessionItemResult {
                    comment_id,
                    status: "failed".to_string(),
                    message: format!("invalid AI reply JSON: {error}"),
                });
                emit_progress(
                    progress_sender.as_ref(),
                    input.provider,
                    "system",
                    format!("thread #{comment_id}: failed (invalid AI reply JSON: {error})"),
                );
                continue;
            }
        };
        let reply_body = format_ai_reply_body(provider_reply.model.as_deref(), &parsed_reply.reply);

        let updated = match service
            .add_reply(
                &input.review_name,
                AddReplyInput {
                    comment_id: parsed_reply.thread_id,
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
                "thread #{comment_id}: reply persisted; status pending_human ({}/{})",
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

#[derive(Debug)]
struct ParsedAiThreadReply {
    thread_id: u64,
    reply: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AiThreadReplyJson {
    thread_id: u64,
    reply: String,
    status: String,
}

fn parse_ai_thread_reply_json(
    raw_reply: &str,
    expected_thread_id: u64,
) -> Result<ParsedAiThreadReply> {
    let json = strip_json_code_fence(raw_reply).trim();
    let parsed: AiThreadReplyJson = match serde_json::from_str(json) {
        Ok(parsed) => parsed,
        Err(error) => {
            let Some(candidate) = embedded_ai_reply_json_candidate(json) else {
                return Err(invalid_ai_reply_json_error(error, json));
            };
            serde_json::from_str(candidate)
                .map_err(|error| invalid_ai_reply_json_error(error, candidate))?
        }
    };

    if parsed.thread_id != expected_thread_id {
        return Err(anyhow!(
            "thread_id {} did not match requested thread {}",
            parsed.thread_id,
            expected_thread_id
        ));
    }

    if parsed.status != "pending_human" {
        return Err(anyhow!(
            "status {:?} did not match required pending_human",
            parsed.status
        ));
    }

    let reply = parsed.reply.trim().to_string();
    if reply.is_empty() {
        return Err(anyhow!("reply must not be empty"));
    }

    Ok(ParsedAiThreadReply {
        thread_id: parsed.thread_id,
        reply,
    })
}

fn invalid_ai_reply_json_error(error: serde_json::Error, json: &str) -> anyhow::Error {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return anyhow!(
            "expected JSON object with thread_id, reply, status: {error}; response was empty"
        );
    }

    anyhow!(
        "expected JSON object with thread_id, reply, status: {error}; response preview: {}",
        ai_reply_preview(trimmed)
    )
}

fn ai_reply_preview(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 500;
    let mut preview = value
        .chars()
        .take(MAX_PREVIEW_CHARS)
        .collect::<String>()
        .replace('\r', "\\r")
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    if value.chars().count() > MAX_PREVIEW_CHARS {
        preview.push_str("...");
    }
    preview
}

fn strip_json_code_fence(raw_reply: &str) -> &str {
    let trimmed = raw_reply.trim();
    if !trimmed.starts_with("```") {
        return trimmed;
    }

    let without_start = if let Some(value) = trimmed.strip_prefix("```json") {
        value
    } else if let Some(value) = trimmed.strip_prefix("```") {
        value
    } else {
        trimmed
    };

    let without_start = without_start.trim_start();
    if let Some(value) = without_start.strip_suffix("```") {
        value.trim()
    } else {
        without_start
    }
}

fn embedded_ai_reply_json_candidate(value: &str) -> Option<&str> {
    let mut search_start = 0;
    while search_start < value.len() {
        let start = value.get(search_start..)?.find('{')? + search_start;
        let end = balanced_json_object_end(value, start)?;
        let candidate = &value[start..end];
        if has_ai_reply_schema_keys(candidate) {
            return Some(candidate);
        }
        search_start = end;
    }
    None
}

fn has_ai_reply_schema_keys(candidate: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("thread_id")
        && object.contains_key("reply")
        && object.contains_key("status")
}

fn balanced_json_object_end(value: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in value.get(start..)?.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(start + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    None
}

fn comment_is_targetable(status: CommentStatus, mode: AiSessionMode) -> bool {
    match mode {
        AiSessionMode::Reply => {
            matches!(status, CommentStatus::Open | CommentStatus::Pending)
        }
        AiSessionMode::Refactor => {
            matches!(status, CommentStatus::Open | CommentStatus::Pending)
        }
    }
}
