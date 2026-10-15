use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct ModelLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub max_request_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    pub response_schema: Value,
}

/// Errors intentionally carry no transport URLs, credentials, or response bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelError {
    #[error("invalid model configuration")]
    Configuration,
    #[error("model transport unavailable")]
    Unavailable,
    #[error("model request deadline exceeded")]
    Timeout,
    #[error("model request exceeds its byte budget")]
    RequestTooLarge,
    #[error("model response exceeds its byte budget")]
    ResponseTooLarge,
    #[error("model server returned HTTP {0}")]
    HttpStatus(u16),
    #[error("model generation reached its token limit before completion")]
    TruncatedResponse,
    #[error("model server refused the request")]
    RefusedResponse,
    #[error("model response is missing, invalid, or uses an unsupported response format")]
    InvalidResponse,
}

/// Trusted providers must bound bytes and time during transport. The runtime
/// checks again after return; it cannot preempt a synchronous custom provider.
pub trait ModelProvider {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError>;
}

impl<T: ModelProvider + ?Sized> ModelProvider for Box<T> {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError> {
        (**self).complete(request, limits)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Call { tool: String, arguments: Value },
    Final { message: String },
    Clarify { message: String },
}

pub(crate) fn decision_schema(tools: &[&crate::ToolDefinition]) -> Value {
    let mut definitions = serde_json::Map::new();
    let mut variants = Vec::new();
    for (index, tool) in tools.iter().enumerate() {
        let name = format!("tool_{index}");
        let reference = format!("#/definitions/{name}");
        let mut arguments = tool.parameters.clone();
        if let Some(object) = arguments.as_object_mut() {
            object.remove("$schema");
            object.remove("title");
        }
        relocate_refs(&mut arguments, &reference);
        definitions.insert(name, arguments.clone());
        variants.push(json!({
            "type":"object",
            "properties": {
                "kind":{"const":"call"},
                "tool":{"const":tool.name},
                "arguments":arguments
            },
            "required":["kind","tool","arguments"],
            "additionalProperties":false
        }));
    }
    variants.extend([
        json!({
            "type": "object",
            "properties": {
                "kind": {"const": "final"},
                "message": {"type": "string"}
            },
            "required": ["kind", "message"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "properties": {
                "kind": {"const": "clarify"},
                "message": {"type": "string"}
            },
            "required": ["kind", "message"],
            "additionalProperties": false
        }),
    ]);
    json!({"anyOf":variants,"definitions":definitions})
}

fn relocate_refs(value: &mut Value, root: &str) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref")
                && let Some(path) = reference.strip_prefix('#')
            {
                *reference = format!("{root}{path}");
            }
            for child in object.values_mut() {
                relocate_refs(child, root);
            }
        }
        Value::Array(array) => {
            for child in array {
                relocate_refs(child, root);
            }
        }
        _ => {}
    }
}
