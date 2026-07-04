use schemars::schema_for;
use serde_json::{Value, json};

use crate::{MODEL_OUTPUT_SCHEMA_VERSION, ModelCandidate, NormalizedCommand};

pub fn model_candidate_schema_json() -> Value {
    let mut schema = serde_json::to_value(schema_for!(ModelCandidate))
        .expect("ModelCandidate schema serializes");
    tighten_model_candidate_schema(&mut schema);
    schema
}

pub fn normalized_command_schema_json() -> Value {
    serde_json::to_value(schema_for!(NormalizedCommand))
        .expect("NormalizedCommand schema serializes")
}

pub fn schema_as_pretty_json(schema: &Value) -> String {
    serde_json::to_string_pretty(schema).expect("schema JSON formats")
}

fn tighten_model_candidate_schema(schema: &mut Value) {
    if let Some(object) = schema.as_object_mut() {
        object.insert("additionalProperties".to_owned(), json!(false));
        object.insert(
            "required".to_owned(),
            json!([
                "schema_version",
                "intent",
                "room",
                "device_alias",
                "device_type",
                "action",
                "params"
            ]),
        );
    }

    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        properties.insert(
            "schema_version".to_owned(),
            json!({
                "type": "string",
                "enum": [MODEL_OUTPUT_SCHEMA_VERSION],
            }),
        );
    }

    tighten_command_params_schema(schema);
}

fn tighten_command_params_schema(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let is_params = object
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(is_command_params_properties);
            if is_params {
                object.insert("additionalProperties".to_owned(), json!(false));
                object.insert(
                    "required".to_owned(),
                    json!([
                        "brightness",
                        "temperature",
                        "mode",
                        "time_after",
                        "raw_value"
                    ]),
                );
            }
            for child in object.values_mut() {
                tighten_command_params_schema(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                tighten_command_params_schema(child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
fn is_command_params_schema(value: &Value) -> bool {
    let Some(properties) = value.get("properties").and_then(Value::as_object) else {
        return false;
    };
    is_command_params_properties(properties)
}

fn is_command_params_properties(properties: &serde_json::Map<String, Value>) -> bool {
    [
        "brightness",
        "temperature",
        "mode",
        "time_after",
        "raw_value",
    ]
    .iter()
    .all(|field| properties.contains_key(*field))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_candidate_schema_contains_properties() {
        let schema = model_candidate_schema_json();
        assert!(schema.get("properties").is_some());
    }

    #[test]
    fn model_candidate_schema_requires_canonical_fields_for_ollama() {
        let schema = model_candidate_schema_json();
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("required array");
        for field in [
            "schema_version",
            "intent",
            "room",
            "device_alias",
            "device_type",
            "action",
            "params",
        ] {
            assert!(required.iter().any(|value| value.as_str() == Some(field)));
        }

        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
        assert_eq!(
            schema
                .pointer("/properties/schema_version/enum/0")
                .and_then(Value::as_str),
            Some(MODEL_OUTPUT_SCHEMA_VERSION)
        );
        assert_eq!(
            find_command_params_schema(&schema)
                .and_then(|value| value.get("additionalProperties"))
                .and_then(Value::as_bool),
            Some(false),
        );
    }

    #[test]
    fn normalized_command_schema_contains_properties() {
        let schema = normalized_command_schema_json();
        assert!(schema.get("properties").is_some());
    }

    fn find_command_params_schema(value: &Value) -> Option<&Value> {
        if is_command_params_schema(value) {
            return Some(value);
        }
        match value {
            Value::Object(object) => object.values().find_map(find_command_params_schema),
            Value::Array(values) => values.iter().find_map(find_command_params_schema),
            _ => None,
        }
    }
}
