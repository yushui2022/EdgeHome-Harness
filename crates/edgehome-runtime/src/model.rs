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
    #[error("model operation was cancelled")]
    Cancelled,
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

    /// Override to observe cancellation during I/O. The default checks only at
    /// callback boundaries; the supplied transport deadline still applies.
    fn complete_cancellable(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
        cancellation: &crate::CancellationToken,
    ) -> Result<String, ModelError> {
        if cancellation.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let result = self.complete(request, limits);
        if cancellation.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        result
    }
}

impl<T: ModelProvider + ?Sized> ModelProvider for Box<T> {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError> {
        (**self).complete(request, limits)
    }

    fn complete_cancellable(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
        cancellation: &crate::CancellationToken,
    ) -> Result<String, ModelError> {
        (**self).complete_cancellable(request, limits, cancellation)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Call { tool: String, arguments: Value },
    Final { message: String },
    Clarify { message: String },
}

/// `Action` puts the routing choice before arguments in the generated schema.
/// This avoids forcing a small model to fill arguments before it can clarify.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionProtocol {
    #[default]
    Tagged,
    Action,
}

pub(crate) fn parse_decision(text: &str, protocol: DecisionProtocol) -> Result<Decision, ()> {
    if protocol == DecisionProtocol::Tagged {
        return serde_json::from_str(text).map_err(|_| ());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Action {
        action: String,
        arguments: Option<Value>,
        message: Option<String>,
    }
    let value: Action = serde_json::from_str(text).map_err(|_| ())?;
    let fields: Value = serde_json::from_str(text).map_err(|_| ())?;
    match value.action.as_str() {
        "clarify" | "final" if fields.get("arguments").is_none() => {
            let message = value.message.ok_or(())?;
            Ok(if value.action == "clarify" {
                Decision::Clarify { message }
            } else {
                Decision::Final { message }
            })
        }
        "clarify" | "final" => Err(()),
        _ if fields.get("message").is_none() => Ok(Decision::Call {
            tool: value.action,
            arguments: value.arguments.ok_or(())?,
        }),
        _ => Err(()),
    }
}

pub(crate) fn action_schema(tools: &[&crate::ToolDefinition]) -> Value {
    let mut schema = decision_schema(tools);
    for variant in schema["anyOf"].as_array_mut().expect("variants") {
        let properties = variant["properties"].as_object_mut().expect("properties");
        let kind = properties.remove("kind").expect("kind");
        let is_call = kind["const"] == "call";
        if is_call {
            let parameters = properties.get_mut("arguments").expect("arguments");
            let fields = required_non_nullable(parameters);
            for field in fields {
                let original = parameters["properties"][&field].clone();
                parameters["properties"][&field] = json!({"anyOf":[original,{"type":"null"}]});
            }
        }
        let action = if is_call {
            properties.remove("tool").expect("tool")
        } else {
            kind
        };
        properties.insert("action".into(), action);
        variant["required"] = if is_call {
            json!(["action", "arguments"])
        } else {
            json!(["action", "message"])
        };
    }
    schema
}

// Only relax explicitly non-nullable top-level fields. Complex/ref schemas keep
// their original contract; the host's typed validator remains authoritative.
fn required_non_nullable(parameters: &Value) -> Vec<String> {
    parameters["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|field| {
            let name = field.as_str()?;
            let property = &parameters["properties"][name];
            let non_nullable = property["type"].as_str().is_some_and(|t| t != "null")
                || property["enum"]
                    .as_array()
                    .is_some_and(|values| !values.contains(&Value::Null));
            non_nullable.then(|| name.to_owned())
        })
        .collect()
}

pub(crate) fn missing_arguments(
    definition: &crate::ToolDefinition,
    arguments: &Value,
) -> Vec<String> {
    required_non_nullable(&definition.parameters)
        .into_iter()
        .filter(|field| arguments.get(field).is_some_and(Value::is_null))
        .collect()
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
        // Inline schemas avoid a redundant copy for common small-model tools.
        // Recursive schemas retain a namespaced target for their local refs.
        if contains_local_ref(&arguments, &reference) {
            definitions.insert(name, arguments.clone());
        }
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

fn contains_local_ref(value: &Value, prefix: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get("$ref")
                .and_then(Value::as_str)
                .is_some_and(|r| r.starts_with(prefix))
                || map.values().any(|v| contains_local_ref(v, prefix))
        }
        Value::Array(values) => values.iter().any(|v| contains_local_ref(v, prefix)),
        _ => false,
    }
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
