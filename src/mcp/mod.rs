use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{
    domain::review::{Author, CommentStatus, ReviewState},
    services::review_service::{AddReplyInput, ReviewService},
};

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

pub async fn run_mcp(service: ReviewService) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    let mut writer = tokio::io::BufWriter::new(stdout);

    while let Some(line) = lines.next_line().await.context("failed to read stdin")? {
        if line.trim().is_empty() {
            continue;
        }

        let request: RpcRequest = match serde_json::from_str(&line) {
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

async fn write_response<W: AsyncWriteExt + Unpin>(writer: &mut W, payload: &Value) -> Result<()> {
    writer
        .write_all(serde_json::to_string(payload)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn handle_request(service: &ReviewService, request: RpcRequest) -> Option<Value> {
    let id = request.id.unwrap_or(Value::Null);
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
                {"name": "get_review", "description": "Get a review by name", "inputSchema": {"type": "object", "required": ["review_name"], "properties": {"review_name": {"type": "string"}}}},
                {"name": "list_open_comments", "description": "List open comments for a review", "inputSchema": {"type": "object", "required": ["review_name"], "properties": {"review_name": {"type": "string"}}}},
                {"name": "add_reply", "description": "Add a reply to a comment", "inputSchema": {"type": "object", "required": ["review_name", "comment_id", "body"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "body": {"type": "string"}, "author": {"type": "string"}}}},
                {"name": "mark_comment_addressed", "description": "Mark a comment as addressed", "inputSchema": {"type": "object", "required": ["review_name", "comment_id"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "author": {"type": "string"}}}},
                {"name": "mark_comment_open", "description": "Mark a comment as open", "inputSchema": {"type": "object", "required": ["review_name", "comment_id"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "author": {"type": "string"}}}},
                {"name": "set_review_state", "description": "Set review state", "inputSchema": {"type": "object", "required": ["review_name", "state"], "properties": {"review_name": {"type": "string"}, "state": {"type": "string"}}}}
            ]
        })),
        "tools/call" => handle_tools_call(service, request.params.unwrap_or(Value::Null)).await,
        _ => Err(anyhow!("method not found: {}", request.method)),
    };

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
            let review_name = required_string(&arguments, "review_name")?;
            let review = service.load_review(review_name).await?;
            serde_json::to_value(review)?
        }
        "list_open_comments" => {
            let review_name = required_string(&arguments, "review_name")?;
            let review = service.load_review(review_name).await?;
            let comments: Vec<_> = review
                .comments
                .into_iter()
                .filter(|comment| matches!(comment.status, CommentStatus::Open))
                .collect();
            json!({ "comments": comments })
        }
        "add_reply" => {
            let review_name = required_string(&arguments, "review_name")?;
            let comment_id = required_u64(&arguments, "comment_id")?;
            let body = required_string(&arguments, "body")?.to_string();
            let author = parse_author_with_default(&arguments, "author", Author::Ai)?;
            let updated = service
                .add_reply(
                    review_name,
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
            let review_name = required_string(&arguments, "review_name")?;
            let comment_id = required_u64(&arguments, "comment_id")?;
            let author = parse_author_with_default(&arguments, "author", Author::Ai)?;
            let updated = service
                .mark_addressed(review_name, comment_id, author)
                .await?;
            serde_json::to_value(updated)?
        }
        "mark_comment_open" => {
            let review_name = required_string(&arguments, "review_name")?;
            let comment_id = required_u64(&arguments, "comment_id")?;
            let author = parse_author_with_default(&arguments, "author", Author::User)?;
            let updated = service.mark_open(review_name, comment_id, author).await?;
            serde_json::to_value(updated)?
        }
        "set_review_state" => {
            let review_name = required_string(&arguments, "review_name")?;
            let state_value = required_string(&arguments, "state")?;
            let next_state = parse_state(state_value)?;
            let updated = service.set_state(review_name, next_state).await?;
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
