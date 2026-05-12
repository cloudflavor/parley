use self::progress::emit_progress;
use self::prompt::{build_thread_prompt, load_task_prompt_override};
use self::provider::{format_ai_reply_body, invoke_provider};
use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::config::{AgentTransport, AiProviderConfig, AppConfig};
use crate::domain::diff::DiffDocument;
use crate::domain::review::{Author, CommentStatus, ReviewSession};
use crate::git::diff::{DiffSource, load_git_diff};
use crate::services::review_service::{AddReplyInput, ReviewService};
use crate::utils::time::now_ms;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

mod progress;
mod prompt;
mod provider;

#[cfg(test)]
mod tests;

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

impl AiSessionResult {
    fn new(input: &RunAiSessionInput, provider_cfg: &AiProviderConfig, now_ms: u64) -> Self {
        Self {
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
        }
    }

    fn push_processed(&mut self, comment_id: u64, message: impl Into<String>) {
        self.processed += 1;
        self.push_item(comment_id, "processed", message);
    }

    fn push_skipped(&mut self, comment_id: u64, message: impl Into<String>) {
        self.skipped += 1;
        self.push_item(comment_id, "skipped", message);
    }

    fn push_failed(&mut self, comment_id: u64, message: impl Into<String>) {
        self.failed += 1;
        self.push_item(comment_id, "failed", message);
    }

    fn push_item(&mut self, comment_id: u64, status: &str, message: impl Into<String>) {
        self.items.push(AiSessionItemResult {
            comment_id,
            status: status.to_string(),
            message: message.into(),
        });
    }
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
    let mut result = AiSessionResult::new(&input, &provider_cfg, now_ms);

    let target_ids = ai_session_target_ids(&review, &input.comment_ids, input.mode);
    let total_targets = target_ids.len();
    if total_targets == 0 {
        result.push_skipped(0, no_targets_message(input.mode));
        emit_progress(
            progress_sender.as_ref(),
            input.provider,
            "system",
            "no open threads to process",
        );
        return Ok(result);
    }

    let task_prompt_override = load_task_prompt_override(&config, input.mode).await?;
    let context = AiSessionExecutionContext {
        service,
        config: &config,
        input: &input,
        diff_document: diff_document.as_ref(),
        task_prompt_override: task_prompt_override.as_deref(),
        progress_sender,
    };
    process_ai_session_targets(&context, &mut review, &mut result, target_ids).await?;

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

struct AiSessionExecutionContext<'a> {
    service: &'a ReviewService,
    config: &'a AppConfig,
    input: &'a RunAiSessionInput,
    diff_document: Option<&'a DiffDocument>,
    task_prompt_override: Option<&'a str>,
    progress_sender: Option<mpsc::UnboundedSender<AiProgressEvent>>,
}

async fn process_ai_session_targets(
    context: &AiSessionExecutionContext<'_>,
    review: &mut ReviewSession,
    result: &mut AiSessionResult,
    target_ids: Vec<u64>,
) -> Result<()> {
    let total_targets = target_ids.len();
    for (step_index, comment_id) in target_ids.into_iter().enumerate() {
        let step_number = step_index + 1;
        emit_progress(
            context.progress_sender.as_ref(),
            context.input.provider,
            "system",
            format!(
                "thread #{comment_id}: start ({}/{})",
                step_number, total_targets
            ),
        );
        debug!(
            review = %context.input.review_name,
            provider = %context.input.provider.as_str(),
            comment_id,
            "processing ai thread"
        );
        process_ai_session_target(
            context,
            review,
            result,
            comment_id,
            step_number,
            total_targets,
        )
        .await?;
    }

    Ok(())
}

async fn process_ai_session_target(
    context: &AiSessionExecutionContext<'_>,
    review: &mut ReviewSession,
    result: &mut AiSessionResult,
    comment_id: u64,
    step_number: usize,
    total_targets: usize,
) -> Result<()> {
    let Some(comment_status) = comment_status(review, comment_id) else {
        warn!(
            review = %context.input.review_name,
            provider = %context.input.provider.as_str(),
            comment_id,
            "ai session target comment not found"
        );
        result.push_failed(comment_id, "comment not found in review");
        emit_progress(
            context.progress_sender.as_ref(),
            context.input.provider,
            "system",
            format!("thread #{comment_id}: failed (comment not found)"),
        );
        return Ok(());
    };

    if !comment_is_targetable(comment_status.clone(), context.input.mode) {
        debug!(
            review = %context.input.review_name,
            provider = %context.input.provider.as_str(),
            comment_id,
            status = ?comment_status,
            "skipping non-targetable comment for selected mode"
        );
        result.push_skipped(
            comment_id,
            format!(
                "comment status {:?} is not targetable for {} mode",
                comment_status,
                context.input.mode.as_str()
            ),
        );
        emit_progress(
            context.progress_sender.as_ref(),
            context.input.provider,
            "system",
            format!("thread #{comment_id}: skipped (status={comment_status:?})"),
        );
        return Ok(());
    }

    let prompt = build_thread_prompt(
        &context.input.review_name,
        comment_id,
        review,
        context.diff_document,
        context.input.mode,
        context.task_prompt_override,
    )
    .await?;
    let provider_reply = match invoke_provider(
        context.config,
        context.input.provider,
        context.input.transport,
        context.input.mode,
        &prompt,
        context.progress_sender.clone(),
    )
    .await
    {
        Ok(reply) => reply,
        Err(error) => {
            error!(
                review = %context.input.review_name,
                provider = %context.input.provider.as_str(),
                comment_id,
                error = %error,
                "provider invocation failed"
            );
            result.push_failed(comment_id, format!("provider failed: {error}"));
            emit_progress(
                context.progress_sender.as_ref(),
                context.input.provider,
                "system",
                format!("thread #{comment_id}: failed ({error})"),
            );
            return Ok(());
        }
    };
    let parsed_reply = match parse_ai_thread_reply_json(&provider_reply.reply, comment_id) {
        Ok(parsed_reply) => parsed_reply,
        Err(error) => {
            result.push_failed(comment_id, format!("invalid AI reply JSON: {error}"));
            emit_progress(
                context.progress_sender.as_ref(),
                context.input.provider,
                "system",
                format!("thread #{comment_id}: failed (invalid AI reply JSON: {error})"),
            );
            return Ok(());
        }
    };
    let reply_body = format_ai_reply_body(provider_reply.model.as_deref(), &parsed_reply.reply);

    *review = match context
        .service
        .add_reply(
            &context.input.review_name,
            AddReplyInput {
                comment_id: parsed_reply.thread_id,
                author: Author::Ai,
                body: reply_body,
            },
        )
        .await
    {
        Ok(updated) => updated,
        Err(error) => {
            error!(
                review = %context.input.review_name,
                provider = %context.input.provider.as_str(),
                comment_id,
                error = %error,
                "failed to persist ai reply"
            );
            result.push_failed(comment_id, format!("failed to persist ai reply: {error}"));
            emit_progress(
                context.progress_sender.as_ref(),
                context.input.provider,
                "system",
                format!("thread #{comment_id}: failed (persist reply: {error})"),
            );
            return Ok(());
        }
    };

    info!(
        review = %context.input.review_name,
        provider = %context.input.provider.as_str(),
        comment_id,
        "ai reply persisted"
    );
    result.push_processed(comment_id, processed_target_message(context.input.mode));
    emit_progress(
        context.progress_sender.as_ref(),
        context.input.provider,
        "system",
        format!(
            "thread #{comment_id}: reply persisted; status pending_human ({step_number}/{total_targets})"
        ),
    );
    Ok(())
}

fn ai_session_target_ids(
    review: &ReviewSession,
    comment_ids: &[u64],
    mode: AiSessionMode,
) -> Vec<u64> {
    if !comment_ids.is_empty() {
        return comment_ids.to_vec();
    }

    review
        .comments
        .iter()
        .filter(|comment| comment_is_targetable(comment.status.clone(), mode))
        .map(|comment| comment.id)
        .collect()
}

fn comment_status(review: &ReviewSession, comment_id: u64) -> Option<CommentStatus> {
    review
        .comments
        .iter()
        .find(|comment| comment.id == comment_id)
        .map(|comment| comment.status.clone())
}

fn no_targets_message(mode: AiSessionMode) -> &'static str {
    match mode {
        AiSessionMode::Reply => "no replyable threads to process",
        AiSessionMode::Refactor => "no open threads to process",
    }
}

fn processed_target_message(mode: AiSessionMode) -> &'static str {
    match mode {
        AiSessionMode::Reply => "ai reply added",
        AiSessionMode::Refactor => "ai reply added; thread status moved to pending_human",
    }
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
