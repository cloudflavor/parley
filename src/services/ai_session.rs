use std::sync::mpsc;

use anyhow::Result;
use serde::Serialize;
use tracing::{debug, error, info, warn};

use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::review::{Author, CommentStatus, ReviewState};
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

    let task_prompt_override = load_task_prompt_override(&config, input.mode).await?;
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
            task_prompt_override.as_deref(),
        )
        .await?;
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

fn comment_is_targetable(status: CommentStatus, mode: AiSessionMode) -> bool {
    match mode {
        AiSessionMode::Reply => {
            matches!(status, CommentStatus::Open | CommentStatus::Pending)
        }
        AiSessionMode::Refactor => matches!(status, CommentStatus::Open),
    }
}
