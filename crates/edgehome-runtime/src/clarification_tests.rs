use super::*;
use crate::tests::{enabled, fixture, plan};
use schemars::JsonSchema;
use serde::Deserialize;
use std::{cell::Cell, rc::Rc};

fn config() -> AgentConfig {
    AgentConfig {
        decision_protocol: DecisionProtocol::Action,
        ..enabled()
    }
}
fn call(value: Value) -> String {
    json!({"action":"counter.add","arguments":{"amount":value}}).to_string()
}

#[test]
fn structured_resolution_consumes_id_and_requires_fresh_plan_approval() {
    let f = fixture(vec![call(Value::Null)]);
    let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
    assert!(matches!(
        agent.prompt("add a number").unwrap(),
        Step::Clarification { .. }
    ));
    let id = agent.clarification().unwrap().clarification_id.clone();
    for values in [
        json!({}),
        json!({"amount":1,"extra":1}),
        json!({"amount":null}),
        json!({"amount":"one"}),
    ] {
        assert!(agent.resolve(&id, values).is_err());
        assert_eq!(agent.clarification().unwrap().clarification_id, id);
        assert_eq!(f.count.get(), 0);
    }
    assert!(matches!(
        agent.continue_run(),
        Err(AgentError::NoContinuation)
    ));
    let p = plan(agent.resolve(&id, json!({"amount":3})).unwrap());
    assert_eq!(agent.usage().model_calls, 1);
    assert!(matches!(
        agent.resolve(&id, json!({"amount":3})),
        Err(AgentError::NoClarification)
    ));
    assert!(matches!(
        agent.execute(&p.plan_id, false),
        Err(AgentError::ConfirmationRequired)
    ));
    agent.execute(&p.plan_id, true).unwrap();
    assert_eq!(f.count.get(), 3);
}

#[test]
fn free_text_reply_retains_task_scope_history_and_call_budget() {
    let f = fixture(vec![
        json!({"action":"clarify","message":"What amount?"}).to_string(),
        call(json!(4)),
    ]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_model_calls: 2,
            ..config()
        },
    )
    .unwrap();
    agent
        .prompt_with_tools("add a number", &["counter.add"])
        .unwrap();
    let id = agent.clarification().unwrap().clarification_id.clone();
    assert!(matches!(
        agent.reply("stale", "4"),
        Err(AgentError::NoClarification)
    ));
    let p = plan(agent.reply(&id, "4").unwrap());
    assert_eq!(p.arguments, json!({"amount":4}));
    let requests = f.requests.borrow();
    let context = serde_json::to_string(&requests[1]).unwrap();
    assert!(context.contains("add a number") && context.contains("What amount?"));
    assert_eq!(agent.usage().model_calls, 2);
    drop(requests);
    assert!(matches!(agent.prompt("new"), Err(AgentError::Budget)));
}

#[test]
fn new_invalid_prompt_and_cancellation_revoke_questions() {
    for cancel in [true, false] {
        let f = fixture(vec![call(Value::Null)]);
        let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
        agent.prompt("add").unwrap();
        let id = agent.clarification().unwrap().clarification_id.clone();
        if cancel {
            agent.cancellation_token().cancel();
        } else {
            assert!(agent.prompt("").is_err());
        }
        assert!(agent.resolve(&id, json!({"amount":1})).is_err());
        assert!(agent.clarification().is_none());
        assert_eq!(f.count.get(), 0);
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Pair {
    target: String,
    amount: i32,
}

#[test]
fn policy_resolution_preserves_other_arguments_and_checks_choices() {
    let f = fixture(vec![
        json!({"action":"pair.add","arguments":{"target":"unknown","amount":7}}).to_string(),
    ]);
    let mut registry = ToolRegistry::new();
    registry
        .register(GuardedTool::new(
            TypedTool::<Pair, _, _>::new(
                "pair.add",
                "Add to target",
                |_| Ok(()),
                |args, _| Ok(json!({"target":args.target,"amount":args.amount})),
            ),
            |v: &Value| {
                if v["target"] == "known" {
                    Ok(Preflight::Allow)
                } else {
                    Ok(Preflight::Clarify {
                        message: "Which target?".into(),
                        fields: vec![InputField {
                            name: "target".into(),
                            prompt: "Choose a target".into(),
                            choices: vec![json!("known")],
                        }],
                    })
                }
            },
        ))
        .unwrap();
    let mut agent = Agent::new(f.provider, registry, config()).unwrap();
    agent.prompt("add 7 to unknown").unwrap();
    let q = agent.clarification().unwrap().clone();
    assert_eq!(q.draft_arguments, json!({"amount":7}));
    assert!(matches!(
        agent.resolve(&q.clarification_id, json!({"target":"other"})),
        Err(AgentError::InvalidAnswer)
    ));
    let p = plan(
        agent
            .resolve(&q.clarification_id, json!({"target":"known"}))
            .unwrap(),
    );
    assert_eq!(p.arguments, json!({"target":"known","amount":7}));
    assert_eq!(agent.usage().model_calls, 1);
    assert_eq!(
        agent.execute(&p.plan_id, true).unwrap().output,
        Some(json!({"target":"known","amount":7}))
    );
}

#[test]
fn changed_preflight_at_dispatch_revokes_approved_plan_and_asks_again() {
    let mut f = fixture(vec![call(json!(2))]);
    let allow = Rc::new(Cell::new(true));
    let guard = allow.clone();
    let original = f.tools.tools.remove("counter.add").unwrap();
    // Custom tool wrapping an existing implementation, to exercise the public trait.
    struct Policy {
        inner: Box<dyn Tool>,
        allow: Rc<Cell<bool>>,
    }
    impl Tool for Policy {
        fn definition(&self) -> ToolDefinition {
            self.inner.definition()
        }
        fn validate(&self, v: &Value) -> Result<(), ToolError> {
            self.inner.validate(v)
        }
        fn preflight(&self, v: &Value) -> Result<Preflight, ToolError> {
            self.validate(v)?;
            if self.allow.get() {
                Ok(Preflight::Allow)
            } else {
                Ok(Preflight::Clarify {
                    message: "Choose another amount".into(),
                    fields: vec![InputField {
                        name: "amount".into(),
                        prompt: "Amount".into(),
                        choices: vec![],
                    }],
                })
            }
        }
        fn execute(&mut self, v: Value, c: ToolContext) -> Result<Value, ToolError> {
            self.inner.execute(v, c)
        }
    }
    let mut registry = ToolRegistry::new();
    registry
        .register(Policy {
            inner: original,
            allow: guard,
        })
        .unwrap();
    let mut agent = Agent::new(f.provider, registry, config()).unwrap();
    let p = plan(agent.prompt("add2").unwrap());
    allow.set(false);
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::ClarificationRequired)
    ));
    assert_eq!(f.count.get(), 0);
    assert!(agent.clarification().is_some());
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
}

#[test]
fn repeated_clarifications_and_oversized_replies_remain_bounded() {
    let f = fixture(vec![call(Value::Null), call(Value::Null)]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_clarification_turns: 1,
            ..config()
        },
    )
    .unwrap();
    agent.prompt("add").unwrap();
    let id = agent.clarification().unwrap().clarification_id.clone();
    assert!(matches!(
        agent.reply(&id, &"x".repeat(2049)),
        Err(AgentError::Input)
    ));
    assert!(agent.clarification().is_some());
    assert!(matches!(
        agent.reply(&id, "still unspecified"),
        Err(AgentError::Budget)
    ));
    assert!(agent.usage().pending_plan.is_none());
    assert_eq!(f.count.get(), 0);
}
