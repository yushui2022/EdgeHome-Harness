use serde_json::{Map, Value, json};

const MAX_DEPTH: usize = 6;
const MAX_OBJECT_FIELDS: usize = 32;
const MAX_ARRAY_ITEMS: usize = 16;
const MAX_STRING_CHARS: usize = 256;

pub(crate) fn sanitize_backend_evidence(value: Value) -> Value {
    sanitize_value(value, 0)
}

pub(crate) fn sanitize_backend_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return serde_json::to_string(&sanitize_backend_evidence(value))
            .unwrap_or_else(|_| "<redacted-unserializable-json>".to_owned());
    }

    if looks_sensitive_text(trimmed) {
        return "<redacted-sensitive-text>".to_owned();
    }

    let char_count = trimmed.chars().count();
    if char_count > MAX_STRING_CHARS {
        format!("<redacted-long-text chars={char_count}>")
    } else {
        trimmed.to_owned()
    }
}

fn sanitize_value(value: Value, depth: usize) -> Value {
    if depth >= MAX_DEPTH {
        return json!({
            "redacted": true,
            "reason": "max_depth"
        });
    }

    match value {
        Value::Object(object) => Value::Object(sanitize_object(object, depth)),
        Value::Array(values) => sanitize_array(values, depth),
        Value::String(value) => sanitize_string(value),
        other => other,
    }
}

fn sanitize_object(object: Map<String, Value>, depth: usize) -> Map<String, Value> {
    let mut output = Map::new();
    let mut entries: Vec<_> = object.into_iter().collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let omitted = entries.len().saturating_sub(MAX_OBJECT_FIELDS);

    for (key, value) in entries.into_iter().take(MAX_OBJECT_FIELDS) {
        let value = if is_sensitive_key(&key) {
            json!("<redacted>")
        } else {
            sanitize_value(value, depth + 1)
        };
        output.insert(key, value);
    }

    if omitted > 0 {
        output.insert("_redacted_omitted_fields".to_owned(), json!(omitted));
    }

    output
}

fn sanitize_array(values: Vec<Value>, depth: usize) -> Value {
    let omitted = values.len().saturating_sub(MAX_ARRAY_ITEMS);
    let mut sanitized = values
        .into_iter()
        .take(MAX_ARRAY_ITEMS)
        .map(|value| sanitize_value(value, depth + 1))
        .collect::<Vec<_>>();

    if omitted > 0 {
        sanitized.push(json!({
            "redacted": true,
            "reason": "array_truncated",
            "omitted_items": omitted
        }));
    }

    Value::Array(sanitized)
}

fn sanitize_string(value: String) -> Value {
    if looks_sensitive_text(&value) {
        return json!("<redacted-sensitive-text>");
    }

    let char_count = value.chars().count();
    if char_count > MAX_STRING_CHARS {
        json!({
            "redacted": true,
            "reason": "string_too_long",
            "chars": char_count
        })
    } else {
        Value::String(value)
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "access_key",
        "access_token",
        "aiid",
        "api_key",
        "apikey",
        "authorization",
        "base_url",
        "broker_url",
        "cluster_id",
        "cookie",
        "credential",
        "did",
        "endpoint",
        "endpoint_id",
        "fabric_id",
        "host",
        "ip",
        "mac",
        "node_id",
        "password",
        "passwd",
        "piid",
        "private_key",
        "refresh_token",
        "secret",
        "siid",
        "token",
        "url",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn looks_sensitive_text(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    [
        "authorization:",
        "bearer ",
        "access_token",
        "refresh_token",
        "password=",
        "passwd=",
        "secret=",
        "token=",
        "api_key",
        "-----begin private key-----",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_keys_recursively() {
        let sanitized = sanitize_backend_evidence(json!({
            "ok": true,
            "access_token": "super-secret-token",
            "nested": {
                "password": "super-secret-password",
                "state": "on"
            },
            "devices": [
                {
                    "did": "xiaomi-private-did",
                    "result": "accepted"
                }
            ]
        }));
        let serialized = serde_json::to_string(&sanitized).expect("serialize");

        assert!(!serialized.contains("super-secret-token"));
        assert!(!serialized.contains("super-secret-password"));
        assert!(!serialized.contains("xiaomi-private-did"));
        assert_eq!(
            sanitized.pointer("/nested/state").and_then(Value::as_str),
            Some("on")
        );
        assert_eq!(
            sanitized.pointer("/access_token").and_then(Value::as_str),
            Some("<redacted>")
        );
    }

    #[test]
    fn bounds_large_backend_responses() {
        let sanitized = sanitize_backend_evidence(json!({
            "message": "x".repeat(MAX_STRING_CHARS + 1),
            "items": (0..(MAX_ARRAY_ITEMS + 2)).collect::<Vec<_>>()
        }));

        assert_eq!(
            sanitized.pointer("/message/reason").and_then(Value::as_str),
            Some("string_too_long")
        );
        assert_eq!(
            sanitized
                .pointer("/items/16/reason")
                .and_then(Value::as_str),
            Some("array_truncated")
        );
    }

    #[test]
    fn sanitizes_sensitive_error_text() {
        assert_eq!(
            sanitize_backend_text(r#"{"error":"nope","token":"super-secret"}"#),
            r#"{"error":"nope","token":"<redacted>"}"#
        );
        assert_eq!(
            sanitize_backend_text("Authorization: Bearer super-secret"),
            "<redacted-sensitive-text>"
        );
    }
}
