use crate::{AppConfig, config::read_bounded, provider, rpc::error_code};
use anyhow::{Result, bail};
use edgehome_runtime::{
    Agent, CancellationToken, ExecutionStatus, ModelError, ModelLimits, ModelProvider,
    ModelRequest, Step, ToolRegistry,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    rc::Rc,
    time::Instant,
};

#[derive(Clone, Copy)]
pub struct EvalOptions {
    pub repeat: u32,
    pub order_seed: u64,
    pub include_responses: bool,
}
impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            repeat: 1,
            order_seed: 42,
            include_responses: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    schema_version: u32,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    input: String,
    tools: Vec<String>,
    #[serde(default = "general")]
    category: String,
    expect: Expectation,
}
fn general() -> String {
    "general".into()
}

// JSON numbers 2 and 2.0 express the same bounded tool value. Do not coerce
// strings, lose integer precision above 2^53, or apply a permissive tolerance.
fn equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if x == y {
                return true;
            }
            let safe = |n: &serde_json::Number| {
                n.as_f64()
                    .is_some_and(|v| v.abs() <= 9_007_199_254_740_992.0)
                    && n.as_u64().is_none_or(|v| v <= 9_007_199_254_740_992)
                    && n.as_i64()
                        .is_none_or(|v| v.unsigned_abs() <= 9_007_199_254_740_992)
            };
            safe(x) && safe(y) && x.as_f64() == y.as_f64()
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|other| equivalent(v, other)))
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equivalent(a, b))
        }
        _ => a == b,
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Expectation {
    Tool {
        tool: String,
        arguments: Value,
        output: Value,
    },
    Clarify {
        #[serde(default)]
        message_contains_any: Vec<String>,
    },
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub mode: &'static str,
    pub model: Option<String>,
    pub model_configuration: Option<Value>,
    pub runtime_configuration: Value,
    pub repeat: u32,
    pub order_seed: u64,
    pub unique_cases: usize,
    pub total: usize,
    pub passed: usize,
    pub task_success_rate: f64,
    pub first_attempt_passed: usize,
    pub recovered_after_validation_repair: usize,
    pub cases_passing_all_repeats: usize,
    pub per_repeat: Vec<Counts>,
    pub per_category: BTreeMap<String, Counts>,
    pub unexpected_proposals: usize,
    pub latency_ms: Value,
    pub cases: Vec<Outcome>,
}
#[derive(Default, Serialize)]
pub struct Counts {
    pub total: usize,
    pub passed: usize,
}
#[derive(Serialize)]
pub struct Outcome {
    pub id: String,
    pub repetition: u32,
    pub category: String,
    pub outcome: String,
    pub elapsed_ms: u128,
    pub model_calls: u32,
    pub tool_calls: u32,
    pub unexpected_proposal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<Vec<Value>>,
}

// Evaluation-only recorder. Production journals still exclude content.
struct RecordedProvider {
    inner: Box<dyn ModelProvider>,
    responses: Rc<RefCell<Vec<Value>>>,
    capture: bool,
}
impl ModelProvider for RecordedProvider {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError> {
        self.complete_cancellable(request, limits, &CancellationToken::default())
    }
    fn complete_cancellable(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
        cancel: &CancellationToken,
    ) -> Result<String, ModelError> {
        let started = Instant::now();
        let result = self.inner.complete_cancellable(request, limits, cancel);
        if self.capture {
            let response = result
                .as_ref()
                .ok()
                .map(|s| s.chars().take(limits.max_output_bytes).collect::<String>());
            self.responses.borrow_mut().push(json!({
                "response":response, "error":result.as_ref().err(),
                "elapsed_ms":started.elapsed().as_millis(),
                "response_truncated_for_trace":result.as_ref().is_ok_and(|s|s.len()>limits.max_output_bytes)
            }));
        }
        result
    }
}

pub fn evaluate(
    path: &Path,
    config: &AppConfig,
    is_mock: bool,
    allowed: &BTreeSet<String>,
    registry: &impl Fn() -> Result<ToolRegistry>,
    mock: &impl Fn() -> Box<dyn ModelProvider>,
    options: EvalOptions,
) -> Result<Report> {
    let suite: Suite = serde_json::from_slice(&read_bounded(path, 65536)?)
        .map_err(|_| anyhow::anyhow!("invalid task evaluation file"))?;
    let mut ids = BTreeSet::new();
    if !matches!(suite.schema_version, 1 | 2)
        || suite.cases.is_empty()
        || suite.cases.len() > 64
        || !(1..=10).contains(&options.repeat)
        || config.runtime.max_model_calls > 16
        || config.runtime.max_context_bytes > 65536
        || suite.cases.iter().any(|c| {
            c.id.is_empty()
                || c.id.len() > 128
                || c.category.is_empty()
                || c.category.len() > 64
                || !ids.insert(c.id.clone())
        })
    {
        bail!("invalid evaluation schema, IDs, repetition count, or runtime budget");
    }
    // Preflight the entire suite before any provider or tool is invoked.
    let definitions = registry()?;
    let names: BTreeSet<_> = definitions.definitions().map(|d| d.name.as_str()).collect();
    if allowed.iter().any(|name| !names.contains(name.as_str())) {
        bail!("unknown allowed tool");
    }
    for case in &suite.cases {
        let unique: BTreeSet<_> = case.tools.iter().collect();
        if case.input.trim().is_empty()
            || case.input.len() > 2048
            || case.tools.is_empty()
            || unique.len() != case.tools.len()
            || case.tools.iter().any(|t| !names.contains(t.as_str()))
        {
            bail!("invalid evaluation input or tool scope");
        }
        match &case.expect {
            Expectation::Tool {
                tool, arguments, ..
            } if !case.tools.contains(tool) || !arguments.is_object() => {
                bail!("expected tool must belong to the case scope and have object arguments")
            }
            Expectation::Clarify {
                message_contains_any,
            } if suite.schema_version == 2
                && (message_contains_any.is_empty()
                    || message_contains_any.iter().any(|s| s.trim().is_empty())) =>
            {
                bail!("schema v2 clarification cases require a nonempty question-content check")
            }
            _ => {}
        }
    }
    let mut outcomes = Vec::new();
    let mut state = options.order_seed;
    for repetition in 1..=options.repeat {
        let mut order: Vec<_> = (0..suite.cases.len()).collect();
        for i in (1..order.len()).rev() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            order.swap(i, (state % (i as u64 + 1)) as usize);
        }
        for index in order {
            let case = &suite.cases[index];
            let responses = Rc::new(RefCell::new(Vec::new()));
            let mut agent = Agent::new(
                RecordedProvider {
                    inner: provider(config, is_mock, mock)?,
                    responses: responses.clone(),
                    capture: options.include_responses,
                },
                registry()?,
                config.agent_config(!allowed.is_empty()),
            )?;
            let started = Instant::now();
            let step = agent.prompt_with_tools(
                &case.input,
                &case.tools.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            let mut observed = step
                .as_ref()
                .map(|s| json!({"step":s}))
                .unwrap_or_else(|e| json!({"error":error_code(e),"message":e.to_string()}));
            let mut unexpected_proposal = false;
            // Authority depends ONLY on --allow, never the answer key.
            // Wrong allowed calls execute in the test environment and count as failures.
            let outcome = match step {
                Ok(Step::Proposal { plan }) => {
                    let matches = match &case.expect {
                        Expectation::Tool {
                            tool, arguments, ..
                        } => plan.tool == *tool && equivalent(&plan.arguments, arguments),
                        _ => false,
                    };
                    unexpected_proposal = !matches;
                    let receipt = if allowed.contains(&plan.tool) {
                        Some(agent.execute(&plan.plan_id, true))
                    } else {
                        None
                    };
                    if let Some(result) = &receipt {
                        observed["execution"] = match result {
                            Ok(r) => json!(r),
                            Err(e) => json!({"error":error_code(e)}),
                        };
                    }
                    match &case.expect {
                        Expectation::Clarify { .. } => "unexpected_proposal".into(),
                        Expectation::Tool { tool, .. } if plan.tool != *tool => {
                            "incorrect_tool".into()
                        }
                        Expectation::Tool { arguments, .. }
                            if !equivalent(&plan.arguments, arguments) =>
                        {
                            "incorrect_arguments".into()
                        }
                        Expectation::Tool { output, .. } => match receipt {
                            None => "execution_not_authorized".into(),
                            Some(Ok(r))
                                if r.status == ExecutionStatus::Succeeded
                                    && r.completion_recorded =>
                            {
                                if r.output
                                    .as_ref()
                                    .is_some_and(|value| equivalent(value, output))
                                {
                                    "passed"
                                } else {
                                    "incorrect_output"
                                }
                                .into()
                            }
                            Some(Ok(_)) => "execution_uncertain".into(),
                            Some(Err(e)) => error_code(&e).into(),
                        },
                    }
                }
                Ok(Step::Clarification { message }) => match &case.expect {
                    Expectation::Clarify {
                        message_contains_any,
                    } => if message_contains_any.is_empty()
                        || message_contains_any
                            .iter()
                            .any(|term| message.to_lowercase().contains(&term.to_lowercase()))
                    {
                        "passed"
                    } else {
                        "irrelevant_clarification"
                    }
                    .into(),
                    _ => "unexpected_clarification".into(),
                },
                Ok(_) => "unexpected_decision".into(),
                Err(e) => format!("{}: {}", error_code(&e), e),
            };
            let usage = agent.usage();
            outcomes.push(Outcome {
                id: case.id.clone(),
                category: case.category.clone(),
                repetition,
                outcome,
                elapsed_ms: started.elapsed().as_millis(),
                model_calls: usage.model_calls,
                tool_calls: usage.tool_calls,
                unexpected_proposal,
                observed: options.include_responses.then_some(observed),
                responses: options
                    .include_responses
                    .then(|| responses.borrow().clone()),
            });
        }
    }
    let total = outcomes.len();
    let passed = outcomes.iter().filter(|c| c.outcome == "passed").count();
    let mut per_repeat: Vec<Counts> = (0..options.repeat).map(|_| Counts::default()).collect();
    let mut per_category: BTreeMap<String, Counts> = BTreeMap::new();
    let mut all_pass: BTreeMap<&str, bool> = BTreeMap::new();
    let mut latencies: Vec<_> = outcomes.iter().map(|c| c.elapsed_ms).collect();
    latencies.sort_unstable();
    for c in &outcomes {
        let pass = c.outcome == "passed";
        for count in [
            &mut per_repeat[c.repetition as usize - 1],
            per_category.entry(c.category.clone()).or_default(),
        ] {
            count.total += 1;
            count.passed += usize::from(pass);
        }
        all_pass
            .entry(&c.id)
            .and_modify(|v| *v &= pass)
            .or_insert(pass);
    }
    Ok(Report {
        schema_version: 2,
        mode: if is_mock { "mock" } else { "model" },
        model: if is_mock {
            None
        } else {
            config.inference.as_ref().map(|c| c.model.clone())
        },
        model_configuration: if is_mock {
            None
        } else {
            config.inference.as_ref().map(|c|json!({
            "engine":c.engine,"request_timeout_ms":c.request_timeout_ms,"max_response_bytes":c.max_response_bytes,
            "max_output_tokens":c.max_output_tokens,"context_tokens":c.context_tokens,"think":c.think,"temperature":0
        }))
        },
        runtime_configuration: json!(config.runtime),
        repeat: options.repeat,
        order_seed: options.order_seed,
        unique_cases: suite.cases.len(),
        total,
        passed,
        task_success_rate: passed as f64 / total as f64,
        first_attempt_passed: outcomes
            .iter()
            .filter(|c| c.outcome == "passed" && c.model_calls == 1)
            .count(),
        recovered_after_validation_repair: outcomes
            .iter()
            .filter(|c| c.outcome == "passed" && c.model_calls > 1)
            .count(),
        cases_passing_all_repeats: all_pass.values().filter(|v| **v).count(),
        per_repeat,
        per_category,
        unexpected_proposals: outcomes.iter().filter(|c| c.unexpected_proposal).count(),
        latency_ms: json!({"p50":latencies[(total-1)/2],"p95":latencies[(total*95).div_ceil(100)-1],"max":latencies[total-1]}),
        cases: outcomes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_comparison_preserves_types_precision_and_structure() {
        assert!(equivalent(&json!({"n":2}), &json!({"n":2.0})));
        assert!(!equivalent(&json!("2"), &json!(2)));
        assert!(!equivalent(
            &json!(9_007_199_254_740_993_u64),
            &json!(9_007_199_254_740_992.0)
        ));
        assert!(!equivalent(&json!({"a":1}), &json!({"a":1,"b":2})));
        assert!(!equivalent(&json!([1, 2]), &json!([2, 1])));
    }
}
