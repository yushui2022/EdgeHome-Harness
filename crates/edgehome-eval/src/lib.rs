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
    pub schema_valid: Option<bool>,
    pub fallback_used: bool,
    pub dead_loop_detected: bool,
    pub retry_count: u64,
    pub latency_ms: Option<i64>,
    pub failure_reason: Option<String>,
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
    pub schema_valid_rate: f32,
    pub memory_resolution_accuracy: f32,
    pub fallback_rate: f32,
    pub dead_loop_rate: f32,
    pub retry_rate: f32,
    pub latency_avg_ms: Option<f32>,
    pub latency_p95_ms: Option<i64>,
    pub low_memory_degrade_count: usize,
    pub results: Vec<EvalCaseResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalGateConfig {
    pub min_total_cases: usize,
    pub min_pass_rate: f32,
    pub min_schema_valid_rate: f32,
    pub max_dead_loop_rate: f32,
    pub min_trace_coverage: f32,
    pub min_intent_accuracy: f32,
    pub min_slot_accuracy: f32,
    pub max_retry_rate: f32,
}

impl Default for EvalGateConfig {
    fn default() -> Self {
        Self {
            min_total_cases: 1,
            min_pass_rate: 1.0,
            min_schema_valid_rate: 1.0,
            max_dead_loop_rate: 0.0,
            min_trace_coverage: 1.0,
            min_intent_accuracy: 0.95,
            min_slot_accuracy: 0.90,
            max_retry_rate: 0.30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalGateReport {
    pub passed: bool,
    pub checks: Vec<EvalGateCheck>,
    pub failing_cases: Vec<EvalGateCaseFailure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalGateCheck {
    pub name: String,
    pub actual: f32,
    pub expected: String,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalGateCaseFailure {
    pub id: String,
    pub input: String,
    pub trace_id: Option<String>,
    pub failures: Vec<String>,
    pub failure_reason: Option<String>,
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
    let trace_frame = output.get("trace_frame");
    let schema_valid = if output.get("mode").and_then(Value::as_str) == Some("memory_write") {
        None
    } else {
        Some(
            trace_frame
                .and_then(|frame| frame.get("schema_result"))
                .and_then(Value::as_str)
                .map(|value| value == "passed")
                .unwrap_or_else(|| normalized.is_some()),
        )
    };
    let output_governor = trace_frame.and_then(|frame| frame.get("output_governor"));
    let fallback_used = output_governor
        .and_then(|governor| governor.get("recommended_fallback"))
        .is_some_and(|fallback| !fallback.is_null());
    let dead_loop_detected = output_governor
        .and_then(|governor| governor.get("repeat_detected"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || output_governor
            .and_then(|governor| governor.get("failure_kind"))
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "dead_loop");
    let retry_count = trace_frame
        .and_then(|frame| frame.get("retry_count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let latency_ms = trace_frame
        .and_then(|frame| frame.get("latency_ms"))
        .and_then(Value::as_i64);
    let failure_reason = trace_frame
        .and_then(|frame| frame.get("failure_reason"))
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
        schema_valid,
        fallback_used,
        dead_loop_detected,
        retry_count,
        latency_ms,
        failure_reason,
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
            schema_valid_rate: option_accuracy(results.iter().map(|result| result.schema_valid)),
            memory_resolution_accuracy: memory_resolution_accuracy(&results),
            fallback_rate: ratio(
                results.iter().filter(|result| result.fallback_used).count(),
                total,
            ),
            dead_loop_rate: ratio(
                results
                    .iter()
                    .filter(|result| result.dead_loop_detected)
                    .count(),
                total,
            ),
            retry_rate: ratio(
                results
                    .iter()
                    .filter(|result| result.retry_count > 0)
                    .count(),
                total,
            ),
            latency_avg_ms: latency_avg_ms(&results),
            latency_p95_ms: latency_p95_ms(&results),
            low_memory_degrade_count: results
                .iter()
                .filter(|result| {
                    result
                        .failure_reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("memory pressure"))
                })
                .count(),
            results,
        }
    }
}

pub fn evaluate_release_gate(report: &EvalReport, config: &EvalGateConfig) -> EvalGateReport {
    let checks = vec![
        min_check(
            "total_cases",
            report.total as f32,
            config.min_total_cases as f32,
        ),
        min_check("pass_rate", report.pass_rate, config.min_pass_rate),
        min_check(
            "schema_valid_rate",
            report.schema_valid_rate,
            config.min_schema_valid_rate,
        ),
        max_check(
            "dead_loop_rate",
            report.dead_loop_rate,
            config.max_dead_loop_rate,
        ),
        min_check(
            "trace_coverage",
            report.trace_coverage,
            config.min_trace_coverage,
        ),
        min_check(
            "intent_accuracy",
            report.intent_accuracy,
            config.min_intent_accuracy,
        ),
        min_check(
            "slot_accuracy",
            report.slot_accuracy,
            config.min_slot_accuracy,
        ),
        max_check("retry_rate", report.retry_rate, config.max_retry_rate),
    ];
    let failing_cases = report
        .results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| EvalGateCaseFailure {
            id: result.id.clone(),
            input: result.input.clone(),
            trace_id: result.trace_id.clone(),
            failures: result.failures.clone(),
            failure_reason: result.failure_reason.clone(),
        })
        .collect::<Vec<_>>();
    let passed = checks.iter().all(|check| check.passed) && failing_cases.is_empty();

    EvalGateReport {
        passed,
        checks,
        failing_cases,
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

fn memory_resolution_accuracy(results: &[EvalCaseResult]) -> f32 {
    let values = results
        .iter()
        .filter(|result| result.id.contains("relative") || result.id.contains("alias_memory"))
        .map(|result| result.slots_correct);
    option_accuracy(values)
}

fn latency_avg_ms(results: &[EvalCaseResult]) -> Option<f32> {
    let values = results
        .iter()
        .filter_map(|result| result.latency_ms)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }

    Some(values.iter().sum::<i64>() as f32 / values.len() as f32)
}

fn latency_p95_ms(results: &[EvalCaseResult]) -> Option<i64> {
    let mut values = results
        .iter()
        .filter_map(|result| result.latency_ms)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }

    values.sort_unstable();
    let index = ((values.len() as f32 * 0.95).ceil() as usize).saturating_sub(1);
    values.get(index).copied()
}

fn min_check(name: &str, actual: f32, expected: f32) -> EvalGateCheck {
    EvalGateCheck {
        name: name.to_owned(),
        actual,
        expected: format!(">= {expected}"),
        passed: actual + f32::EPSILON >= expected,
    }
}

fn max_check(name: &str, actual: f32, expected: f32) -> EvalGateCheck {
    EvalGateCheck {
        name: name.to_owned(),
        actual,
        expected: format!("<= {expected}"),
        passed: actual <= expected + f32::EPSILON,
    }
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
            "trace_frame": {
                "schema_result": "passed",
                "output_governor": {
                    "accepted": true,
                    "repeat_detected": false,
                    "recommended_fallback": null
                },
                "retry_count": 0,
                "latency_ms": 12
            },
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
        assert_eq!(result.schema_valid, Some(true));
        assert!(!result.fallback_used);
        assert_eq!(result.latency_ms, Some(12));
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
                schema_valid: Some(true),
                fallback_used: false,
                dead_loop_detected: false,
                retry_count: 0,
                latency_ms: Some(10),
                failure_reason: None,
            },
            EvalCaseResult {
                id: "relative_bad".to_owned(),
                input: "bad".to_owned(),
                trace_id: None,
                passed: false,
                failures: vec!["trace_id missing".to_owned()],
                intent_correct: Some(false),
                slots_correct: Some(false),
                policy_correct: Some(true),
                dry_run_correct: Some(false),
                schema_valid: Some(false),
                fallback_used: true,
                dead_loop_detected: true,
                retry_count: 1,
                latency_ms: Some(30),
                failure_reason: Some("fallback".to_owned()),
            },
        ]);

        assert_eq!(report.total, 2);
        assert_eq!(report.passed, 1);
        assert_eq!(report.pass_rate, 0.5);
        assert_eq!(report.policy_accuracy, 1.0);
        assert_eq!(report.trace_coverage, 0.5);
        assert_eq!(report.schema_valid_rate, 0.5);
        assert_eq!(report.memory_resolution_accuracy, 0.0);
        assert_eq!(report.fallback_rate, 0.5);
        assert_eq!(report.dead_loop_rate, 0.5);
        assert_eq!(report.retry_rate, 0.5);
        assert_eq!(report.latency_avg_ms, Some(20.0));
        assert_eq!(report.latency_p95_ms, Some(30));
    }

    #[test]
    fn release_gate_passes_clean_report() {
        let report = EvalReport::from_results(vec![EvalCaseResult {
            id: "ok".to_owned(),
            input: "把客厅灯关掉".to_owned(),
            trace_id: Some("tr_ok".to_owned()),
            passed: true,
            failures: Vec::new(),
            intent_correct: Some(true),
            slots_correct: Some(true),
            policy_correct: Some(true),
            dry_run_correct: Some(true),
            schema_valid: Some(true),
            fallback_used: false,
            dead_loop_detected: false,
            retry_count: 0,
            latency_ms: Some(10),
            failure_reason: None,
        }]);

        let gate = evaluate_release_gate(&report, &EvalGateConfig::default());

        assert!(gate.passed);
        assert!(gate.failing_cases.is_empty());
        assert!(gate.checks.iter().all(|check| check.passed));
    }

    #[test]
    fn release_gate_reports_failed_cases_and_checks() {
        let report = EvalReport::from_results(vec![
            EvalCaseResult {
                id: "ok".to_owned(),
                input: "把客厅灯关掉".to_owned(),
                trace_id: Some("tr_ok".to_owned()),
                passed: true,
                failures: Vec::new(),
                intent_correct: Some(true),
                slots_correct: Some(true),
                policy_correct: Some(true),
                dry_run_correct: Some(true),
                schema_valid: Some(true),
                fallback_used: false,
                dead_loop_detected: false,
                retry_count: 0,
                latency_ms: Some(10),
                failure_reason: None,
            },
            EvalCaseResult {
                id: "bad_json".to_owned(),
                input: "坏 JSON".to_owned(),
                trace_id: None,
                passed: false,
                failures: vec!["trace_id missing".to_owned()],
                intent_correct: Some(false),
                slots_correct: Some(false),
                policy_correct: Some(true),
                dry_run_correct: Some(false),
                schema_valid: Some(false),
                fallback_used: true,
                dead_loop_detected: true,
                retry_count: 1,
                latency_ms: Some(20),
                failure_reason: Some("schema failed".to_owned()),
            },
        ]);

        let gate = evaluate_release_gate(&report, &EvalGateConfig::default());

        assert!(!gate.passed);
        assert_eq!(gate.failing_cases.len(), 1);
        assert_eq!(gate.failing_cases[0].id, "bad_json");
        assert!(
            gate.checks
                .iter()
                .any(|check| check.name == "schema_valid_rate" && !check.passed)
        );
        assert!(
            gate.checks
                .iter()
                .any(|check| check.name == "dead_loop_rate" && !check.passed)
        );
    }
}
