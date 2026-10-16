use anyhow::{Result, bail};
use edgehome_runtime::{Message, ModelLimits, ModelProvider, ModelRequest};
use serde_json::{Value, json};

/// Verify two distinct system-only values. Neither the user message nor schema
/// contains the answer: a const-only JSON test cannot detect dropped system roles.
pub fn check(provider: &mut dyn ModelProvider, limits: ModelLimits) -> Result<()> {
    for token in ["cobalt", "saffron"] {
        let request = ModelRequest {
            messages: vec![
                Message::new(
                    "system",
                    format!(
                        "Return one JSON object with the key token and the exact lowercase string value {token}."
                    ),
                ),
                Message::new("user", "Return the token given in the system instruction."),
            ],
            response_schema: json!({"type":"object","properties":{"token":{"type":"string"}},"required":["token"],"additionalProperties":false}),
        };
        let response = provider.complete(&request, limits)?;
        let value: Option<Value> = serde_json::from_str(&response).ok();
        if value != Some(json!({"token":token})) {
            bail!(
                "model role probe failed; check the chat template and model instruction following"
            );
        }
    }
    Ok(())
}
