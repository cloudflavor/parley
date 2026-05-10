use crate::docs::{PARLEY_DOCS, ParleyDoc, find_doc};
use crate::domain::ai::{AiProvider, AiSessionMode};
use crate::domain::review::{Author, CommentStatus, ReviewState};
use crate::git::diff::DiffSource;
use crate::persistence::store::validate_review_name;
use crate::services::ai_session::{RunAiSessionInput, default_ai_session_mode, run_ai_session};
use crate::services::review_service::{AddReplyInput, ReviewService};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, from_str, json, to_vec};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter,
    stdin, stdout,
};

const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FramingMode {
    ContentLength,
    NewlineDelimited,
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: Option<String>,
}

/// # Errors
///
/// Returns an error when stdin/stdout framing, response serialization, or service operations fail.
pub async fn run_mcp(service: ReviewService) -> Result<()> {
    let stdin = stdin();
    let stdout = stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);
    let mut framing_mode = None;

    while let Some(body) = read_message_body(&mut reader, &mut framing_mode).await? {
        let request: RpcRequest = match from_str(&body) {
            Ok(request) => request,
            Err(error) => {
                let payload = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {error}")}
                });
                write_response(
                    &mut writer,
                    &payload,
                    framing_mode.unwrap_or(FramingMode::ContentLength),
                )
                .await?;
                continue;
            }
        };

        if request.jsonrpc.as_deref() != Some("2.0") {
            let payload = json!({
                "jsonrpc": "2.0",
                "id": request.id.unwrap_or(Value::Null),
                "error": {"code": -32600, "message": "invalid jsonrpc version"}
            });
            write_response(
                &mut writer,
                &payload,
                framing_mode.unwrap_or(FramingMode::ContentLength),
            )
            .await?;
            continue;
        }

        let response = handle_request(&service, request).await;
        if let Some(payload) = response {
            write_response(
                &mut writer,
                &payload,
                framing_mode.unwrap_or(FramingMode::ContentLength),
            )
            .await?;
        }
    }

    Ok(())
}

async fn read_message_body<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    framing_mode: &mut Option<FramingMode>,
) -> Result<Option<String>> {
    let mode = match framing_mode {
        Some(mode) => *mode,
        None => match detect_framing_mode(reader).await? {
            Some(mode) => {
                *framing_mode = Some(mode);
                mode
            }
            None => return Ok(None),
        },
    };

    match mode {
        FramingMode::ContentLength => read_content_length_message_body(reader).await,
        FramingMode::NewlineDelimited => read_newline_message_body(reader).await,
    }
}

async fn detect_framing_mode<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<FramingMode>> {
    loop {
        let buffer = reader
            .fill_buf()
            .await
            .context("failed to read MCP framing prefix")?;
        if buffer.is_empty() {
            return Ok(None);
        }

        match buffer[0] {
            b'{' => return Ok(Some(FramingMode::NewlineDelimited)),
            b'C' | b'c' => return Ok(Some(FramingMode::ContentLength)),
            b'\r' | b'\n' | b' ' | b'\t' => reader.consume(1),
            _ => return Ok(Some(FramingMode::NewlineDelimited)),
        }
    }
}

async fn read_content_length_message_body<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<String>> {
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

async fn read_newline_message_body<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<String>> {
    loop {
        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .await
            .context("failed to read newline-delimited MCP message")?;
        if bytes_read == 0 {
            return Ok(None);
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        return Ok(Some(trimmed.to_string()));
    }
}

async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &Value,
    framing_mode: FramingMode,
) -> Result<()> {
    let body = to_vec(payload)?;
    match framing_mode {
        FramingMode::ContentLength => {
            let header = format!("Content-Length: {}\r\n\r\n", body.len());
            writer.write_all(header.as_bytes()).await?;
            writer
                .write_all(&body)
                .await
                .context("failed to write MCP response body")?;
        }
        FramingMode::NewlineDelimited => {
            writer
                .write_all(&body)
                .await
                .context("failed to write MCP response body")?;
            writer
                .write_all(b"\n")
                .await
                .context("failed to write newline-delimited MCP separator")?;
        }
    }
    writer.flush().await?;
    Ok(())
}

async fn handle_request(service: &ReviewService, request: RpcRequest) -> Option<Value> {
    let id = request.id;
    let result = match request.method.as_str() {
        "initialize" => Ok(initialize_result(request.params.as_ref())),
        "notifications/initialized" | "initialized" | "notifications/cancelled" => return None,
        "ping" => Ok(json!({})),
        "resources/list" => Ok(list_documentation_resources()),
        "resources/read" => read_documentation_resource(request.params.unwrap_or(Value::Null)),
        "tools/list" => Ok(json!({
            "tools": [
                {"name": "list_reviews", "description": "List review session names", "inputSchema": {"type": "object", "properties": {}}},
                {"name": "get_review", "description": "Get a review by name (defaults to current branch review)", "inputSchema": {"type": "object", "properties": {"review_name": {"type": "string"}}}},
                {"name": "list_open_comments", "description": "List open comments for a review (defaults to current branch review)", "inputSchema": {"type": "object", "properties": {"review_name": {"type": "string"}}}},
                {"name": "get_documentation", "description": "Get embedded Parley documentation. Omit doc to list available docs.", "inputSchema": {"type": "object", "properties": {"doc": {"type": "string", "description": "Documentation slug, title, source path, or URI", "enum": ["keybindings", "overview", "quickstart", "review-workflow", "mcp"]}}}},
                {"name": "add_reply", "description": "Add a reply to a comment", "inputSchema": {"type": "object", "required": ["comment_id", "body"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "body": {"type": "string"}, "author": {"type": "string"}}}},
                {"name": "mark_comment_addressed", "description": "Mark a comment as addressed", "inputSchema": {"type": "object", "required": ["comment_id"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "author": {"type": "string"}}}},
                {"name": "mark_comment_open", "description": "Mark a comment as open", "inputSchema": {"type": "object", "required": ["comment_id"], "properties": {"review_name": {"type": "string"}, "comment_id": {"type": "integer"}, "author": {"type": "string"}}}},
                {"name": "run_ai_session", "description": "Run AI against review threads (default mode: reply when comment_ids are provided, refactor otherwise)", "inputSchema": {"type": "object", "required": ["provider"], "properties": {"review_name": {"type": "string"}, "provider": {"type": "string", "enum": ["codex", "claude", "opencode", "pi"]}, "mode": {"type": "string", "enum": ["reply", "refactor"]}, "comment_ids": {"type": "array", "items": {"type": "integer"}}}}},
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

fn initialize_result(params: Option<&Value>) -> Value {
    let protocol_version = negotiate_protocol_version(params);
    json!({
        "protocolVersion": protocol_version,
        "serverInfo": {"name": "parley", "version": env!("CARGO_PKG_VERSION")},
        "capabilities": {
            "resources": {
                "listChanged": false
            },
            "tools": {
                "listChanged": false
            }
        }
    })
}

fn negotiate_protocol_version(params: Option<&Value>) -> String {
    let Some(raw_params) = params else {
        return DEFAULT_PROTOCOL_VERSION.to_string();
    };

    let Some(requested) = serde_json::from_value::<InitializeParams>(raw_params.clone())
        .ok()
        .and_then(|parsed| parsed.protocol_version)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return DEFAULT_PROTOCOL_VERSION.to_string();
    };

    if SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .any(|supported| *supported == requested)
    {
        requested
    } else {
        DEFAULT_PROTOCOL_VERSION.to_string()
    }
}

fn list_documentation_resources() -> Value {
    json!({
        "resources": PARLEY_DOCS
            .iter()
            .map(documentation_resource_metadata)
            .collect::<Vec<_>>()
    })
}

fn documentation_resource_metadata(doc: &ParleyDoc) -> Value {
    json!({
        "uri": doc.uri,
        "name": doc.slug,
        "title": doc.title,
        "description": format!("Embedded Parley documentation from {}", doc.source_path),
        "mimeType": "text/markdown",
    })
}

fn read_documentation_resource(params: Value) -> Result<Value> {
    let uri = required_string(&params, "uri")?;
    let doc = find_doc(uri).ok_or_else(|| anyhow!("documentation resource not found: {uri}"))?;

    Ok(json!({
        "contents": [
            {
                "uri": doc.uri,
                "mimeType": "text/markdown",
                "text": doc.body,
            }
        ]
    }))
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
        "get_documentation" => get_documentation_tool_output(&arguments)?,
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
            let comment_ids = required_u64_list(&arguments, "comment_ids")?;
            let mode = arguments
                .get("mode")
                .and_then(Value::as_str)
                .map(str::parse::<AiSessionMode>)
                .transpose()
                .map_err(|error| anyhow!(error))?
                .unwrap_or_else(|| default_ai_session_mode(&comment_ids));
            let output = run_ai_session(
                service,
                RunAiSessionInput {
                    review_name,
                    provider,
                    transport: None,
                    comment_ids,
                    mode,
                    diff_source: DiffSource::WorkingTree,
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

fn get_documentation_tool_output(arguments: &Value) -> Result<Value> {
    let Some(doc_value) = arguments.get("doc").and_then(Value::as_str) else {
        return Ok(json!({
            "docs": PARLEY_DOCS
                .iter()
                .map(documentation_resource_metadata)
                .collect::<Vec<_>>()
        }));
    };

    let doc = find_doc(doc_value).ok_or_else(|| anyhow!("documentation not found: {doc_value}"))?;

    Ok(json!({
        "title": doc.title,
        "slug": doc.slug,
        "source_path": doc.source_path,
        "uri": doc.uri,
        "mime_type": "text/markdown",
        "body": doc.body,
    }))
}

fn resolve_review_name(arguments: &Value) -> Result<String> {
    let name = required_string(arguments, "review_name")?.trim();
    validate_review_name(name).map_err(|error| anyhow!(error))?;
    Ok(name.to_string())
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
        "open" => Ok(ReviewState::Open),
        "under_review" => Ok(ReviewState::UnderReview),
        _ => Err(anyhow!("invalid state value: {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::store::Store;
    use anyhow::{Result, anyhow};
    use tempfile::tempdir;
    use tokio::io::BufReader;

    #[test]
    fn initialize_uses_requested_supported_protocol_version() {
        let params = json!({"protocolVersion": "2025-03-26"});
        let response = initialize_result(Some(&params));

        assert_eq!(response["protocolVersion"], json!("2025-03-26"));
        assert_eq!(
            response["capabilities"]["tools"]["listChanged"],
            json!(false)
        );
        assert_eq!(
            response["capabilities"]["resources"]["listChanged"],
            json!(false)
        );
    }

    #[test]
    fn initialize_falls_back_to_default_protocol_version() {
        let unsupported = json!({"protocolVersion": "1.0.0"});
        let response = initialize_result(Some(&unsupported));
        assert_eq!(response["protocolVersion"], json!(DEFAULT_PROTOCOL_VERSION));

        let missing = initialize_result(None);
        assert_eq!(missing["protocolVersion"], json!(DEFAULT_PROTOCOL_VERSION));
    }

    #[tokio::test]
    async fn ping_request_returns_empty_result() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let request = RpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(1)),
            method: "ping".to_string(),
            params: None,
        };

        let response = handle_request(&service, request)
            .await
            .ok_or_else(|| anyhow!("ping request should return a response"))?;

        assert_eq!(response["result"], json!({}));
        Ok(())
    }

    #[tokio::test]
    async fn resources_list_returns_embedded_documentation() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let request = RpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(1)),
            method: "resources/list".to_string(),
            params: None,
        };

        let response = handle_request(&service, request)
            .await
            .ok_or_else(|| anyhow!("resources/list request should return a response"))?;
        let resources = response["result"]["resources"]
            .as_array()
            .ok_or_else(|| anyhow!("resources should be an array"))?;

        assert!(
            resources
                .iter()
                .any(|resource| resource["uri"] == json!("parley://docs/overview"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn resources_read_returns_embedded_markdown() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let request = RpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(1)),
            method: "resources/read".to_string(),
            params: Some(json!({"uri": "parley://docs/overview"})),
        };

        let response = handle_request(&service, request)
            .await
            .ok_or_else(|| anyhow!("resources/read request should return a response"))?;
        let content = &response["result"]["contents"][0];

        assert_eq!(content["uri"], json!("parley://docs/overview"));
        assert_eq!(content["mimeType"], json!("text/markdown"));
        assert!(
            content["text"]
                .as_str()
                .ok_or_else(|| anyhow!("content text should be a string"))?
                .contains("# Parley")
        );
        Ok(())
    }

    #[tokio::test]
    async fn get_documentation_tool_returns_requested_doc() -> Result<()> {
        let tempdir = tempdir()?;
        let service = ReviewService::new(Store::from_project_root(tempdir.path()));
        let response = handle_tools_call(
            &service,
            json!({
                "name": "get_documentation",
                "arguments": {"doc": "mcp"}
            }),
        )
        .await?;

        assert_eq!(response["structuredContent"]["slug"], json!("mcp"));
        assert!(
            response["structuredContent"]["body"]
                .as_str()
                .ok_or_else(|| anyhow!("body should be a string"))?
                .contains("# MCP Integration")
        );
        Ok(())
    }

    #[tokio::test]
    async fn read_message_body_supports_newline_delimited_json() -> Result<()> {
        let raw = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
        let mut reader = BufReader::new(&raw[..]);
        let mut framing_mode = None;

        let body = read_message_body(&mut reader, &mut framing_mode)
            .await
            .and_then(|body| body.ok_or_else(|| anyhow!("newline message should exist")))?;

        assert_eq!(framing_mode, Some(FramingMode::NewlineDelimited));
        let parsed: Value = serde_json::from_str(&body)?;
        assert_eq!(parsed["method"], json!("ping"));
        Ok(())
    }

    #[tokio::test]
    async fn read_message_body_supports_content_length_framing() -> Result<()> {
        let payload = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}";
        let raw = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);
        let mut reader = BufReader::new(raw.as_bytes());
        let mut framing_mode = None;

        let body = read_message_body(&mut reader, &mut framing_mode)
            .await
            .and_then(|body| body.ok_or_else(|| anyhow!("content-length message should exist")))?;

        assert_eq!(framing_mode, Some(FramingMode::ContentLength));
        let parsed: Value = serde_json::from_str(&body)?;
        assert_eq!(parsed["method"], json!("ping"));
        Ok(())
    }
}
