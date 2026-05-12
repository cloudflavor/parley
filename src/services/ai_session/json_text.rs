use serde_json::Value;

#[must_use]
pub(crate) fn first_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => {
            for key in ["text", "content"] {
                if let Some(Value::String(text)) = map.get(key) {
                    return Some(text.clone());
                }
            }
            for nested in map.values() {
                if let Some(text) = first_text(nested) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(text) = first_text(item) {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

#[must_use]
pub(crate) fn text_delta(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("text_delta")
                && let Some(delta) = map.get("delta").and_then(Value::as_str)
            {
                return Some(delta.to_string());
            }
            for nested in map.values() {
                if let Some(delta) = text_delta(nested) {
                    return Some(delta);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(delta) = text_delta(item) {
                    return Some(delta);
                }
            }
            None
        }
        _ => None,
    }
}

#[must_use]
pub(crate) fn assistant_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(role) = map.get("role").and_then(Value::as_str)
                && role != "assistant"
            {
                return None;
            }
            for key in [
                "delta", "text", "content", "message", "body", "reply", "output",
            ] {
                if let Some(text) = map.get(key).and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    return Some(text.to_string());
                }
            }
            for nested in map.values() {
                if let Some(text) = assistant_text(nested) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(items) => {
            for item in items {
                if let Some(text) = assistant_text(item) {
                    return Some(text);
                }
            }
            None
        }
        _ => None,
    }
}

#[must_use]
pub(crate) fn collect_named_text(value: &Value, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    collect_named_text_into(value, keys, None, &mut out);
    out
}

#[must_use]
pub(crate) fn first_named_text(value: &Value, keys: &[&str]) -> Option<String> {
    collect_named_text(value, keys).into_iter().next()
}

fn collect_named_text_into(
    value: &Value,
    keys: &[&str],
    current_key: Option<&str>,
    out: &mut Vec<String>,
) {
    match value {
        Value::String(text) => {
            let Some(current_key) = current_key else {
                return;
            };
            if keys.iter().any(|key| current_key.eq_ignore_ascii_case(key))
                && !text.trim().is_empty()
            {
                out.push(text.trim().to_string());
            }
        }
        Value::Object(map) => {
            for (key, nested) in map {
                collect_named_text_into(nested, keys, Some(key), out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_named_text_into(item, keys, current_key, out);
            }
        }
        _ => {}
    }
}

#[must_use]
pub(crate) fn compact_redacted_json_for_log(value: &Value) -> String {
    let mut redacted = value.clone();
    redact_prompt_text(&mut redacted);
    redact_file_content(&mut redacted);
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

fn redact_file_content(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(content)) = map.get_mut("content") {
                let chars = content.chars().count();
                *content = format!("<redacted file content: {chars} chars>");
            }
            for nested in map.values_mut() {
                redact_file_content(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_file_content(item);
            }
        }
        _ => {}
    }
}
