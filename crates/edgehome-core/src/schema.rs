use schemars::schema_for;

use crate::{ModelCandidate, NormalizedCommand};

pub fn model_candidate_schema_json() -> serde_json::Value {
    serde_json::to_value(schema_for!(ModelCandidate)).expect("ModelCandidate schema serializes")
}

pub fn normalized_command_schema_json() -> serde_json::Value {
    serde_json::to_value(schema_for!(NormalizedCommand))
        .expect("NormalizedCommand schema serializes")
}

pub fn schema_as_pretty_json(schema: &serde_json::Value) -> String {
    serde_json::to_string_pretty(schema).expect("schema JSON formats")
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
    fn normalized_command_schema_contains_properties() {
        let schema = normalized_command_schema_json();
        assert!(schema.get("properties").is_some());
    }
}
