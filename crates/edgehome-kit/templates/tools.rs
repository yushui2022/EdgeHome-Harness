//! Starter fixtures. Replace the handlers with your bounded application I/O.
use edgehome_runtime::{
    ModelError, ModelLimits, ModelProvider, ModelRequest, ToolRegistry, TypedTool,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Document {
    Sample,
    Empty,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DocumentArgs {
    document: Document,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Sensor {
    Room,
    Outside,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SensorArgs {
    sensor: Sensor,
}

pub fn registry() -> anyhow::Result<ToolRegistry> {
    let mut tools = ToolRegistry::new();
    tools.register(TypedTool::<DocumentArgs,_,_>::new(
        "document.stats", "Count characters and lines in a named fixture document. The user must specify which document; there is no default.",
        |_|Ok(()), |args,context| {
            if context.cancellation.is_cancelled() { return Err(edgehome_runtime::ToolError::Unavailable); }
            let document=match args.document {
                Document::Sample=>"EdgeHome is a local agent development toolkit.\nTools belong to the host application.",
                Document::Empty=>"",
            };
            Ok(json!({"characters":document.chars().count(),"lines":document.lines().count()}))
        },
    ))?;
    tools.register(TypedTool::<SensorArgs,_,_>::new(
        "sensor.read", "Read a named fixture temperature sensor in Celsius. The user must specify which sensor; there is no default.",
        |_|Ok(()), |args,context| {
            if context.cancellation.is_cancelled() { return Err(edgehome_runtime::ToolError::Unavailable); }
            let temperature=match args.sensor { Sensor::Room=>22.5, Sensor::Outside=>18.0 };
            Ok(json!({"temperature_c":temperature,"source":"fixture"}))
        },
    ))?;
    Ok(tools)
}

/// Deterministic demo routing only. It is never selected after a model failure.
pub fn mock() -> Box<dyn ModelProvider> {
    Box::new(FixtureModel)
}
struct FixtureModel;
impl ModelProvider for FixtureModel {
    fn complete(&mut self, request: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
        let input = &request
            .messages
            .last()
            .ok_or(ModelError::InvalidResponse)?
            .content;
        if let Ok(value) = serde_json::from_str::<Value>(input)
            && let Some(result) = value.get("tool_result")
        {
            return Ok(
                json!({"kind":"final","message":format!("Fixture result: {}",result["output"])})
                    .to_string(),
            );
        }
        let input = input.to_lowercase();
        let choice = if input.contains("document") {
            if input.contains("sample") {
                Some(("document.stats", json!({"document":"sample"})))
            } else if input.contains("empty") {
                Some(("document.stats", json!({"document":"empty"})))
            } else {
                None
            }
        } else if input.contains("sensor") {
            if input.contains("outside") {
                Some(("sensor.read", json!({"sensor":"outside"})))
            } else if input.contains("room") {
                Some(("sensor.read", json!({"sensor":"room"})))
            } else {
                None
            }
        } else {
            None
        };
        Ok(match choice {
            Some((tool, arguments)) => json!({"kind":"call","tool":tool,"arguments":arguments}),
            None => json!({"kind":"clarify","message":"Which document or sensor should I use?"}),
        }
        .to_string())
    }
}
