//! Eval case loading, output comparison, and metrics for EdgeHome Harness.
//!
//! Eval proves the harness behavior rather than relying on ad-hoc demos. The
//! runner compares normalized command fields, policy decisions, dry-run status,
//! and trace availability from a pipeline JSON output.

use std::path::{Path, PathBuf};

use edgehome_core::{
    Action, DeviceId, DeviceType, Intent, NormalizedCommand, PolicyDecision, Room,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub type EvalResult<T> = Result<T, EvalError>;

#[derive(Debug, Error)]
pub enum EvalError {
    #[error("failed to read eval cases `{path}`: {source}")]
    ReadCases {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse eval cases `{path}`: {source}")]
    ParseCases {
        path: PathBuf,
        source: serde_yaml::Error,
    },

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub input: String,
    #[serde(default)]
    pub expected: ExpectedOutput,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpectedOutput {
    pub intent: Option<Intent>,
    pub room: Option<Room>,
    pub device_id: Option<DeviceId>,
    pub device_type: Option<DeviceType>,
    pub action: Option<Action>,
    pub brightness: Option<u8>,
    pub temperature: Option<i16>,
    pub time_after: Option<String>,
    pub policy_decision: Option<PolicyDecision>,
    pub dry_run_ready: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalCaseResult {
    pub id: String,
    pub input: String,
    pub trace_id: Option<String>,
    pub passed: bool,
    pub failures: Vec<String>,
    pub intent_correct: Option<bool>,
    pub slots_correct: Option<bool>,
    pub policy_correct: Option<bool>,
    pub dry_run_correct: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f32,
    pub intent_accuracy: f32,
    pub slot_accuracy: f32,
    pub policy_accuracy: f32,
    pub dry_run_accuracy: f32,
    pub trace_coverage: f32,
    pub results: Vec<EvalCaseResult>,
}

pub fn load_cases(path: impl AsRef<Path>) -> EvalResult<Vec<EvalCase>> {
    let path = path.as_ref();
    let content = std::fs::read_to_string(path).map_err(|source| EvalError::ReadCases {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&content).map_err(|source| EvalError::ParseCases {
        path: path.to_path_buf(),
        source,
    })
}

pub fn evaluate_case_output(case: &EvalCase, output: &Value) -> EvalResult<EvalCaseResult> {
    let normalized = output
        .get("normalized_command")
        .cloned()
        .map(serde_json::from_value::<NormalizedCommand>)
        .transpose()?;
    let policy_decision = output
        .get("policy_decision")
        .cloned()
        .map(serde_json::from_value::<PolicyDecision>)
        .transpose()?;
    let dry_run_ready = output
        .get("dry_run_plan")
        .map(|value| !value.is_null())
        .unwrap_or(false);
    let trace_id = output
        .get("trace_id")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let mut failures = Vec::new();
    let mut slot_checks = Vec::new();

    let intent_correct = case.expected.intent.as_ref().map(|expected| {
        let actual = normalized.as_ref().map(|command| &command.intent);
        let ok = actual == Some(expected);
        push_failure(&mut failures, ok, "intent", expected, actual);
        ok
    });

    if let Some(command) = normalized.as_ref() {
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "room",
            case.expected.room.as_ref(),
            Some(&command.room),
        );
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "device_id",
            case.expected.device_id.as_ref(),
            command.device_id.as_ref(),
        );
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "device_type",
            case.expected.device_type.as_ref(),
            Some(&command.device_type),
        );
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "action",
            case.expected.action.as_ref(),
            Some(&command.action),
        );
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "brightness",
            case.expected.brightness.as_ref(),
            command.params.brightness.as_ref(),
        );
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "temperature",
            case.expected.temperature.as_ref(),
            command.params.temperature.as_ref(),
        );
        compare_slot(
            &mut failures,
            &mut slot_checks,
            "time_after",
            case.expected.time_after.as_ref(),
            command.params.time_after.as_ref(),
        );
    } else if case_has_slot_expectations(&case.expected) {
        failures.push("normalized_command missing".to_owned());
        slot_checks.push(false);
    }

    let policy_correct = case.expected.policy_decision.as_ref().map(|expected| {
        let ok = policy_decision.as_ref() == Some(expected);
        push_failure(
            &mut failures,
            ok,
            "policy_decision",
            expected,
            policy_decision.as_ref(),
        );
        ok
    });

    let dry_run_correct = case.expected.dry_run_ready.map(|expected| {
        let ok = dry_run_ready == expected;
        if !ok {
            failures.push(format!(
                "dry_run_ready expected `{expected}` got `{dry_run_ready}`"
            ));
        }
        ok
    });

    let slots_correct = (!slot_checks.is_empty()).then(|| slot_checks.iter().all(|ok| *ok));
    let passed = failures.is_empty() && trace_id.is_some();
    let mut failures = failures;
    if trace_id.is_none() {
        failures.push("trace_id missing".to_owned());
    }

    Ok(EvalCaseResult {
        id: case.id.clone(),
        input: case.input.clone(),
        trace_id,
        passed,
        failures,
        intent_correct,
        slots_correct,
        policy_correct,
        dry_run_correct,
    })
}

impl EvalReport {
    pub fn from_results(results: Vec<EvalCaseResult>) -> Self {
        let total = results.len();
        let passed = results.iter().filter(|result| result.passed).count();
        let failed = total.saturating_sub(passed);
        Self {
            total,
            passed,
            failed,
            pass_rate: ratio(passed, total),
            intent_accuracy: option_accuracy(results.iter().map(|result| result.intent_correct)),
            slot_accuracy: option_accuracy(results.iter().map(|result| result.slots_correct)),
            policy_accuracy: option_accuracy(results.iter().map(|result| result.policy_correct)),
            dry_run_accuracy: option_accuracy(results.iter().map(|result| result.dry_run_correct)),
            trace_coverage: ratio(
                results
                    .iter()
                    .filter(|result| result.trace_id.is_some())
                    .count(),
                total,
            ),
            results,
        }
    }
}

fn compare_slot<T>(
    failures: &mut Vec<String>,
    slot_checks: &mut Vec<bool>,
    label: &str,
    expected: Option<&T>,
    actual: Option<&T>,
) where
    T: std::fmt::Debug + PartialEq,
{
    if let Some(expected) = expected {
        let ok = actual == Some(expected);
        push_failure(failures, ok, label, expected, actual);
        slot_checks.push(ok);
    }
}

fn push_failure<T>(
    failures: &mut Vec<String>,
    ok: bool,
    label: &str,
    expected: &T,
    actual: Option<&T>,
) where
    T: std::fmt::Debug,
{
    if !ok {
        failures.push(format!(
            "{label} expected `{:?}` got `{:?}`",
            expected, actual
        ));
    }
}

fn case_has_slot_expectations(expected: &ExpectedOutput) -> bool {
    expected.room.is_some()
        || expected.device_id.is_some()
        || expected.device_type.is_some()
        || expected.action.is_some()
        || expected.brightness.is_some()
        || expected.temperature.is_some()
        || expected.time_after.is_some()
}

fn option_accuracy(values: impl Iterator<Item = Option<bool>>) -> f32 {
    let values = values.flatten().collect::<Vec<_>>();
    ratio(values.iter().filter(|value| **value).count(), values.len())
}

fn ratio(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f32 / denominator as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn evaluates_successful_case_output() {
        let case = EvalCase {
            id: "hallway_brightness".to_owned(),
            input: "晚上十点后把走廊灯调到30%".to_owned(),
            expected: ExpectedOutput {
                intent: Some(Intent::ControlDevice),
                room: Some(Room::Hallway),
                device_id: Some(DeviceId::new("hallway_light").expect("device id")),
                device_type: Some(DeviceType::Light),
                action: Some(Action::SetBrightness),
                brightness: Some(30),
                time_after: Some("22:00".to_owned()),
                policy_decision: Some(PolicyDecision::Allow),
                dry_run_ready: Some(true),
                ..ExpectedOutput::default()
            },
        };
        let output = json!({
            "trace_id": "tr_test",
            "policy_decision": "allow",
            "dry_run_plan": { "plan": {} },
            "normalized_command": {
                "schema_version": "command.v1",
                "intent": "control_device",
                "room": "hallway",
                "device_id": "hallway_light",
                "device_type": "light",
                "action": "set_brightness",
                "params": {
                    "brightness": 30,
                    "time_after": "22:00"
                },
                "risk": "low"
            }
        });

        let result = evaluate_case_output(&case, &output).expect("result");

        assert!(result.passed);
        assert_eq!(result.failures, Vec::<String>::new());
    }

    #[test]
    fn report_calculates_metrics() {
        let report = EvalReport::from_results(vec![
            EvalCaseResult {
                id: "ok".to_owned(),
                input: "ok".to_owned(),
                trace_id: Some("tr_ok".to_owned()),
                passed: true,
                failures: Vec::new(),
                intent_correct: Some(true),
                slots_correct: Some(true),
                policy_correct: Some(true),
                dry_run_correct: Some(true),
            },
            EvalCaseResult {
                id: "bad".to_owned(),
                input: "bad".to_owned(),
                trace_id: None,
                passed: false,
                failures: vec!["trace_id missing".to_owned()],
                intent_correct: Some(false),
                slots_correct: Some(false),
                policy_correct: Some(true),
                dry_run_correct: Some(false),
            },
        ]);

        assert_eq!(report.total, 2);
        assert_eq!(report.passed, 1);
        assert_eq!(report.pass_rate, 0.5);
        assert_eq!(report.policy_accuracy, 1.0);
        assert_eq!(report.trace_coverage, 0.5);
    }
}
