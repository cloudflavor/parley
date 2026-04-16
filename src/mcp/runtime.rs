use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::review::{Author, CommentStatus, ReviewState};
use crate::git::review_name::resolve_tui_review_name;
use crate::services::ai_session::{RunAiSessionInput, run_ai_session};
use crate::services::review_service::{AddReplyInput, ReviewService};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, from_str, json, to_vec};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter, stdin, stdout,
};

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

pub async fn run_mcp(service: ReviewService) -> Result<()> {
    let stdin = stdin();
    let stdout = stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);

    while let Some(body) = read_message_body(&mut reader).await? {
        let request: RpcRequest = match from_str(&body) {
            Ok(request) => request,
            Err(error) => {
                let payload = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {error}")}
                });
                write_response(&mut writer, &payload).await?;
                continue;
            }
        };

        if request.jsonrpc.as_deref() != Some("2.0") {
            let payload = json!({
                "jsonrpc": "2.0",
                "id": request.id.unwrap_or(Value::Null),
                "error": {"code": -32600, "message": "invalid jsonrpc version"}
            });
            write_response(&mut writer, &payload).await?;
            continue;
        }

        let response = handle_request(&service, request).await;
        if let Some(payload) = response {
            write_response(&mut writer, &payload).await?;
        }
    }

    Ok(())
}

async fn read_message_body<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .await
            .context("failed to read MCP header line")?;
        if bytes_read == 0 {
            if content_length.is_none() {
                return Ok(None);
            }
            return Err(anyhow!("unexpected EOF while reading MCP headers"));
        }

        if line == "\r\n" || line == "\n" {
            break;
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        let (name, value) = trimmed
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid MCP header line: {trimmed}"))?;
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .trim()
                .parse::<usize>()
                .with_context(|| format!("invalid Content-Length value: {}", value.trim()))?;
            content_length = Some(parsed);
        }
    }

    let content_length = content_length.ok_or_else(|| anyhow!("missing Content-Length header"))?;
    let mut body = vec![0u8; content_length];
    reader
        .read_exact(&mut body)
        .await
        .context("failed to read MCP message body")?;
    let text = String::from_utf8(body).context("MCP message body is not utf-8")?;
    Ok(Some(text))
}

async fn write_response<W: AsyncWriteExt + Unpin>(writer: &mut W, payload: &Value) -> Result<()> {
    let body = to_vec(payload)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
    writer
        .write_all(&body)
        .await
        .context("failed to write MCP response body")?;
    writer.flush().await?;
    Ok(())
}

async fn handle_request(service: &ReviewService, request: RpcRequest) -> Option<Value> {
    let id = request.id;
    let result = match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {"name": "parlar", "version": "0.1.0"},
            "capabilities": {"tools": {}}
        })),
        "notifications/initialized" => return None,
        "tools/list" => Ok(json!({
            "tools": [
                {"name": "list_reviews", "description": "List review session names", "inputSchema": {"type": "object", "properties": {}}},
                {"name": "get_review", "description": "Get a review by name (defaults to current branch review)", "inputSchema": {"type": "object", "properties": {"review_name": {"type": "string"}}}},
                {"name": "list_open_comments", "description": "List open comments for a review (defaults to current branch review)", "inputSchema": {"type": "object", "properties": {"review_name": {"type": "string"}}}},
                {"name": "add_reply", "description": "Add a reply to a comment", "inputSchema": {"type": "object", "required": ["comment_id", "body"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "body": {"type": "string"}, "author": {"type": "string"}}}},
                {"name": "mark_comment_addressed", "description": "Mark a comment as addressed", "inputSchema": {"type": "object", "required": ["comment_id"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "author": {"type": "string"}}}},
                {"name": "mark_comment_open", "description": "Mark a comment as open", "inputSchema": {"type": "object", "required": ["comment_id"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "author": {"type": "string"}}}},
                {"name": "run_ai_session", "description": "Run AI against unresolved comments in a review session", "inputSchema": {"type": "object", "required": ["provider"], "properties": {"review_name": {"type": "string"}, "provider": {"type": "string", "enum": ["codex", "claude", "opencode"]}, "mode": {"type": "string", "enum": ["reply", "refactor"]}, "comment_ids": {"type": "array", "items": {"type": "integer"}}}}},
                {"name": "set_review_state", "description": "Set review state", "inputSchema": {"type": "object", "required": ["state"], "properties": {"review_name": {"type": "string"}, "state": {"type": "string"}}}}
            ]
        })),
        "tools/call" => handle_tools_call(service, request.params.unwrap_or(Value::Null)).await,
        _ => Err(anyhow!("method not found: {}", request.method)),
    };

    let id = id?;

    match result {
        Ok(value) => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": value,
        })),
        Err(error) => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": error.to_string(),
            }
        })),
    }
}

async fn handle_tools_call(service: &ReviewService, params: Value) -> Result<Value> {
    let tool = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing tools/call params.name"))?;

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let output = match tool {
        "list_reviews" => json!({ "reviews": service.list_reviews().await? }),
        "get_review" => {
            let review_name = resolve_review_name(&arguments)?;
            let review = service.load_review(&review_name).await?;
            serde_json::to_value(review)?
        }
        "list_open_comments" => {
            let review_name = resolve_review_name(&arguments)?;
            let review = service.load_review(&review_name).await?;
            let comments: Vec<_> = review
                .comments
                .into_iter()
                .filter(|comment| !matches!(comment.status, CommentStatus::Addressed))
                .collect();
            json!({ "comments": comments })
        }
        "add_reply" => {
            let review_name = resolve_review_name(&arguments)?;
            let comment_id = required_u64(&arguments, "comment_id")?;
            let body = required_string(&arguments, "body")?.to_string();
            let author = parse_author_with_default(&arguments, "author", Author::Ai)?;
            let updated = service
                .add_reply(
                    &review_name,
                    AddReplyInput {
                        comment_id,
                        author,
                        body,
                    },
                )
                .await?;
            serde_json::to_value(updated)?
        }
        "mark_comment_addressed" => {
            let review_name = resolve_review_name(&arguments)?;
            let comment_id = required_u64(&arguments, "comment_id")?;
            let author = parse_author_with_default(&arguments, "author", Author::User)?;
            let updated = service
                .mark_addressed(&review_name, comment_id, author)
                .await?;
            serde_json::to_value(updated)?
        }
        "mark_comment_open" => {
            let review_name = resolve_review_name(&arguments)?;
            let comment_id = required_u64(&arguments, "comment_id")?;
            let author = parse_author_with_default(&arguments, "author", Author::User)?;
            let updated = service.mark_open(&review_name, comment_id, author).await?;
            serde_json::to_value(updated)?
        }
        "run_ai_session" => {
            let review_name = resolve_review_name(&arguments)?;
            let provider_value = required_string(&arguments, "provider")?;
            let provider = provider_value
                .parse::<AiProvider>()
                .map_err(|error| anyhow!(error))?;
            let mode = arguments
                .get("mode")
                .and_then(Value::as_str)
                .map(str::parse::<AiSessionMode>)
                .transpose()
                .map_err(|error| anyhow!(error))?
                .unwrap_or(AiSessionMode::Refactor);
            let comment_ids = required_u64_list(&arguments, "comment_ids")?;
            let output = run_ai_session(
                service,
                RunAiSessionInput {
                    review_name,
                    provider,
                    comment_ids,
                    mode,
                },
            )
            .await?;
            serde_json::to_value(output)?
        }
        "set_review_state" => {
            let review_name = resolve_review_name(&arguments)?;
            let state_value = required_string(&arguments, "state")?;
            let next_state = parse_state(state_value)?;
            let updated = service.set_state(&review_name, next_state).await?;
            serde_json::to_value(updated)?
        }
        _ => return Err(anyhow!("tool not found: {tool}")),
    };

    Ok(json!({
        "content": [
            {"type": "text", "text": serde_json::to_string_pretty(&output)?}
        ],
        "structuredContent": output
    }))
}

fn resolve_review_name(arguments: &Value) -> Result<String> {
    let explicit = arguments.get("review_name").and_then(Value::as_str);
    resolve_tui_review_name(explicit)
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing required string field: {key}"))
}

fn required_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("missing required integer field: {key}"))
}

fn required_u64_list(value: &Value, key: &str) -> Result<Vec<u64>> {
    let Some(raw) = value.get(key) else {
        return Ok(Vec::new());
    };

    let Some(items) = raw.as_array() else {
        return Err(anyhow!("field {key} must be an array of integers"));
    };

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(value) = item.as_u64() else {
            return Err(anyhow!("field {key} must contain only integers"));
        };
        out.push(value);
    }
    Ok(out)
}

fn parse_author_with_default(value: &Value, key: &str, default: Author) -> Result<Author> {
    match value.get(key).and_then(Value::as_str) {
        None => Ok(default),
        Some("user") => Ok(Author::User),
        Some("ai") => Ok(Author::Ai),
        Some(other) => Err(anyhow!("invalid author value: {other}")),
    }
}

fn parse_state(value: &str) -> Result<ReviewState> {
    match value {
        "draft" => Ok(ReviewState::Draft),
        "pending" => Ok(ReviewState::Pending),
        "waiting_for_response" => Ok(ReviewState::WaitingForResponse),
        "done" => Ok(ReviewState::Done),
        _ => Err(anyhow!("invalid state value: {value}")),
    }
}
