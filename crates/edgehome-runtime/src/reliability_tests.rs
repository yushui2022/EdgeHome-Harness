use super::*;
use crate::tests::{enabled, fixture, plan};
fn action(amount: Value) -> String {
    json!({"action":"counter.add","arguments":{"amount":amount}}).to_string()
}
fn config() -> AgentConfig {
    AgentConfig {
        decision_protocol: DecisionProtocol::Action,
        max_decision_repairs: 1,
        ..enabled()
    }
}

#[test]
fn missing_required_value_clarifies_without_a_plan_or_tool_call() {
    let f = fixture(vec![action(Value::Null)]);
    let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
    let Step::Clarification { message } = agent.prompt("add an unspecified amount").unwrap() else {
        panic!()
    };
    assert!(message.contains("amount"));
    assert!(agent.usage().pending_plan.is_none());
    assert_eq!(f.count.get(), 0);
    assert_eq!(agent.usage().model_calls, 1);
}

#[test]
fn invalid_arguments_repair_once_but_execution_still_requires_approval() {
    let f = fixture(vec![action(json!("two")), action(json!(2))]);
    let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
    let p = plan(agent.prompt("add two").unwrap());
    assert_eq!(agent.usage().model_calls, 2);
    assert_eq!(f.count.get(), 0);
    assert!(
        agent
            .journal()
            .events()
            .iter()
            .any(|e| e.kind == EventKind::ValidationRepair)
    );
    let requests = f.requests.borrow();
    assert!(
        requests[1]
            .messages
            .last()
            .unwrap()
            .content
            .contains("invalid arguments")
    );
    assert!(
        !requests[1]
            .messages
            .last()
            .unwrap()
            .content
            .contains("\"amount\":2")
    );
    assert!(matches!(
        agent.execute(&p.plan_id, false),
        Err(AgentError::ConfirmationRequired)
    ));
    agent.execute(&p.plan_id, true).unwrap();
    assert_eq!(f.count.get(), 2);
}

#[test]
fn valid_but_wrong_decisions_are_not_semantically_retried() {
    let f = fixture(vec![action(json!(9)), action(json!(2))]);
    let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
    let p = plan(agent.prompt("add two").unwrap());
    assert_eq!(p.arguments, json!({"amount":9}));
    assert_eq!(agent.usage().model_calls, 1);
    assert_eq!(f.count.get(), 0);
}

#[test]
fn repair_count_and_global_call_budget_both_stop_invalid_generations() {
    for (max_calls, expected) in [(1, 1), (8, 2)] {
        let f = fixture(vec![
            "not JSON".into(),
            "still invalid".into(),
            action(json!(1)),
        ]);
        let mut agent = Agent::new(
            f.provider,
            f.tools,
            AgentConfig {
                max_model_calls: max_calls,
                ..config()
            },
        )
        .unwrap();
        assert!(matches!(agent.prompt("add one"), Err(AgentError::Decision)));
        assert_eq!(agent.usage().model_calls, expected);
        assert!(agent.usage().pending_plan.is_none());
        assert_eq!(f.count.get(), 0);
    }
}

#[test]
fn denied_calls_and_transport_failures_never_trigger_repair() {
    let f = fixture(vec![action(json!(1)), action(json!(2))]);
    f.allowed.set(false);
    let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
    assert!(matches!(
        agent.prompt("add"),
        Err(AgentError::Tool(ToolError::Denied))
    ));
    assert_eq!(agent.usage().model_calls, 1);
    let f = fixture(vec![]);
    let mut agent = Agent::new(f.provider, f.tools, config()).unwrap();
    assert!(matches!(
        agent.prompt("add"),
        Err(AgentError::Model(ModelError::Unavailable))
    ));
    assert_eq!(agent.usage().model_calls, 1);
}

#[test]
fn action_parser_rejects_extra_null_fields_duplicate_keys_and_wrong_types() {
    for text in [
        r#"{"action":"clarify","message":"Which?","arguments":null}"#,
        r#"{"action":"counter.add","arguments":{"amount":1},"message":null}"#,
        r#"{"action":"counter.add","action":"clarify","arguments":{}}"#,
        r#"{"action":3,"arguments":{}}"#,
    ] {
        assert!(model::parse_decision(text, DecisionProtocol::Action).is_err());
    }
}

#[test]
fn action_full_loop_preserves_observations_and_exhausted_tool_budget() {
    let f = fixture(vec![
        action(json!(3)),
        json!({"action":"final","message":"Count is 3"}).to_string(),
    ]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_tool_calls: 1,
            require_tool_observation: true,
            ..config()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("add three").unwrap());
    agent.execute(&p.plan_id, true).unwrap();
    assert!(matches!(agent.continue_run(), Ok(Step::Final { .. })));
    let requests = f.requests.borrow();
    assert!(
        !requests[1].response_schema["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["properties"].get("arguments").is_some())
    );
    assert!(
        requests[1]
            .messages
            .last()
            .unwrap()
            .content
            .contains("\"count\":3")
    );
}

#[test]
fn nullable_schema_fields_keep_their_original_meaning() {
    let tool = ToolDefinition {
        name: "read".into(),
        description: "read".into(),
        parameters: json!({
            "type":"object","properties":{"optional":{"type":["string","null"]},"required":{"type":"string"}},"required":["optional","required"]
        }),
    };
    assert_eq!(
        model::missing_arguments(&tool, &json!({"optional":null,"required":null})),
        vec!["required"]
    );
    let schema = model::action_schema(&[&tool]);
    assert_eq!(
        schema["anyOf"][0]["properties"]["arguments"]["properties"]["optional"]["type"],
        json!(["string", "null"])
    );
}
