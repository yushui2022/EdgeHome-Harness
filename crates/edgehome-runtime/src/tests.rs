use super::*;
use schemars::JsonSchema;
use serde::Deserialize;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    thread,
};

#[test]
fn scoped_tools_are_enforced_by_the_host_and_hidden_from_the_model() {
    let mut f = fixture(vec![
        json!({"kind":"call","tool":"other.read","arguments":{"amount":1}}).to_string(),
    ]);
    f.tools
        .register(TypedTool::<Args, _, _>::new(
            "other.read",
            "Private tool",
            |_| Ok(()),
            |_, _| Ok(json!(null)),
        ))
        .unwrap();
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    assert!(matches!(
        agent.prompt_with_tools("add", &["counter.add"]),
        Err(AgentError::Decision)
    ));
    let requests = f.requests.borrow();
    let request = serde_json::to_string(&requests[0]).unwrap();
    assert!(!request.contains("other.read"));
    assert!(!request.contains("Private tool"));
    assert_eq!(f.count.get(), 0);
    assert!(matches!(
        agent.prompt_with_tools("add", &["absent"]),
        Err(AgentError::Configuration)
    ));
}

#[test]
fn cancellation_before_dispatch_revokes_plan_and_never_resets_budgets() {
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let p = plan(agent.prompt("add").unwrap());
    agent.cancellation_token().cancel();
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::Cancelled)
    ));
    assert!(agent.usage().pending_plan.is_none());
    assert_eq!(agent.usage().model_calls, 1);
    assert_eq!(agent.usage().tool_calls, 0);
    assert!(matches!(agent.prompt("again"), Err(AgentError::Cancelled)));
    assert_eq!(f.count.get(), 0);
}

#[test]
fn boxed_provider_can_cooperatively_stop_and_never_publish_a_late_plan() {
    struct StopProvider;
    impl ModelProvider for StopProvider {
        fn complete(&mut self, _: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
            panic!("cancellable override must be used")
        }
        fn complete_cancellable(
            &mut self,
            _: &ModelRequest,
            _: ModelLimits,
            cancel: &CancellationToken,
        ) -> Result<String, ModelError> {
            cancel.cancel();
            Ok(call(1))
        }
    }
    let f = fixture(vec![]);
    let provider: Box<dyn ModelProvider> = Box::new(StopProvider);
    let mut agent = Agent::new(provider, f.tools, enabled()).unwrap();
    assert!(matches!(agent.prompt("add"), Err(AgentError::Cancelled)));
    assert!(agent.usage().pending_plan.is_none());
    assert_eq!(f.count.get(), 0);
}

#[test]
fn lifecycle_listener_sees_ordered_events_without_sensitive_payloads() {
    let f = fixture(vec![call(1)]);
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let journal = ObservedJournal::new(MemoryJournal::default(), move |event: &Event| {
        sink.borrow_mut().push(event.clone())
    });
    let mut agent = Agent::with_journal(f.provider, f.tools, enabled(), journal).unwrap();
    let p = plan(agent.prompt("private input").unwrap());
    agent.execute(&p.plan_id, true).unwrap();
    let events = events.borrow();
    assert_eq!(
        events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![
            EventKind::TaskStarted,
            EventKind::ModelStarted,
            EventKind::ModelCompleted,
            EventKind::Proposed,
            EventKind::DispatchStarted,
            EventKind::Completed
        ]
    );
    assert!(
        !serde_json::to_string(&*events)
            .unwrap()
            .contains("private input")
    );
}

#[test]
fn cancellation_during_a_tool_returns_unknown_and_consumes_the_plan() {
    let mut f = fixture(vec![call(1)]);
    let count = f.count.clone();
    f.tools = ToolRegistry::new();
    f.tools
        .register(TypedTool::<Args, _, _>::new(
            "counter.add",
            "Cancelling fixture",
            |_| Ok(()),
            move |_, context| {
                count.set(count.get() + 1);
                context.cancellation.cancel();
                Ok(json!({"count":count.get()}))
            },
        ))
        .unwrap();
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let p = plan(agent.prompt("add").unwrap());
    let receipt = agent.execute(&p.plan_id, true).unwrap();
    assert_eq!(receipt.status, ExecutionStatus::OutcomeUnknown);
    assert!(!receipt.continuation_available);
    assert_eq!(f.count.get(), 1);
    assert!(agent.usage().pending_plan.is_none());
}

#[test]
fn cancellation_from_a_dispatch_event_prevents_handler_entry() {
    let f = fixture(vec![call(1)]);
    let slot = Rc::new(RefCell::new(None::<CancellationToken>));
    let signal = slot.clone();
    let journal = ObservedJournal::new(MemoryJournal::default(), move |event: &Event| {
        if event.kind == EventKind::DispatchStarted {
            signal.borrow().as_ref().unwrap().cancel();
        }
    });
    let mut agent = Agent::with_journal(f.provider, f.tools, enabled(), journal).unwrap();
    *slot.borrow_mut() = Some(agent.cancellation_token());
    let p = plan(agent.prompt("add").unwrap());
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::Cancelled)
    ));
    assert_eq!(f.count.get(), 0);
    assert_eq!(agent.usage().tool_calls, 0);
}

pub(super) struct FixtureProvider {
    outputs: VecDeque<Result<String, ModelError>>,
    requests: Rc<RefCell<Vec<ModelRequest>>>,
}
impl ModelProvider for FixtureProvider {
    fn complete(&mut self, request: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
        self.requests.borrow_mut().push(request.clone());
        self.outputs
            .pop_front()
            .unwrap_or(Err(ModelError::Unavailable))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    amount: u32,
}

fn call(amount: u32) -> String {
    json!({"kind":"call","tool":"counter.add","arguments":{"amount":amount}}).to_string()
}

pub(super) struct Fixture {
    pub(super) provider: FixtureProvider,
    pub(super) tools: ToolRegistry,
    pub(super) count: Rc<Cell<u32>>,
    pub(super) allowed: Rc<Cell<bool>>,
    pub(super) requests: Rc<RefCell<Vec<ModelRequest>>>,
}

pub(super) fn fixture(outputs: Vec<String>) -> Fixture {
    let requests = Rc::new(RefCell::new(Vec::new()));
    let count = Rc::new(Cell::new(0));
    let allowed = Rc::new(Cell::new(true));
    let observed = count.clone();
    let policy = allowed.clone();
    let mut tools = ToolRegistry::new();
    tools
        .register(TypedTool::<Args, _, _>::new(
            "counter.add",
            "Add a bounded amount to a local counter",
            move |args| {
                if !policy.get() {
                    return Err(ToolError::Denied);
                }
                if args.amount > 10 {
                    return Err(ToolError::Arguments);
                }
                Ok(())
            },
            move |args, _| {
                observed.set(observed.get() + args.amount);
                Ok(json!({"count":observed.get()}))
            },
        ))
        .unwrap();
    Fixture {
        provider: FixtureProvider {
            outputs: outputs.into_iter().map(Ok).collect(),
            requests: requests.clone(),
        },
        tools,
        count,
        allowed,
        requests,
    }
}

pub(super) fn enabled() -> AgentConfig {
    AgentConfig {
        execute_enabled: true,
        ..AgentConfig::default()
    }
}
pub(super) fn plan(step: Step) -> PlanPreview {
    match step {
        Step::Proposal { plan } => plan,
        _ => panic!("expected proposal"),
    }
}

#[test]
fn full_loop_passes_tool_observation_to_model_and_stops_on_final() {
    let f = fixture(vec![
        call(2),
        json!({"kind":"final","message":"Counter is 2"}).to_string(),
    ]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let p = plan(agent.prompt("add two").unwrap());
    assert_eq!(f.count.get(), 0);
    let receipt = agent.execute(&p.plan_id, true).unwrap();
    assert_eq!(receipt.status, ExecutionStatus::Succeeded);
    assert!(receipt.continuation_available);
    assert_eq!(f.count.get(), 2);
    assert!(matches!(agent.continue_run().unwrap(), Step::Final { .. }));
    let requests = f.requests.borrow();
    let observed = requests[1].messages.last().unwrap();
    assert!(observed.content.contains("tool_result"));
    assert!(observed.content.contains("\"count\":2"));
    assert!(matches!(
        agent.continue_run(),
        Err(AgentError::NoContinuation)
    ));
}

#[test]
fn default_disabled_and_explicit_confirmation_prevent_side_effects() {
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(f.provider, f.tools, AgentConfig::default()).unwrap();
    let p = plan(agent.prompt("add").unwrap());
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::ExecuteDisabled)
    ));
    assert_eq!(f.count.get(), 0);
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let p = plan(agent.prompt("add").unwrap());
    assert!(matches!(
        agent.execute(&p.plan_id, false),
        Err(AgentError::ConfirmationRequired)
    ));
    assert_eq!(f.count.get(), 0);
    assert!(matches!(
        agent.continue_run(),
        Err(AgentError::NoContinuation)
    ));
}

#[test]
fn previews_are_copies_and_consumed_plans_cannot_repeat_or_cross_instances() {
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let mut p = plan(agent.prompt("add").unwrap());
    p.arguments = json!({"amount":9});
    p.tool = "unregistered".into();
    let other = fixture(vec![call(1)]);
    let mut other = Agent::new(other.provider, other.tools, enabled()).unwrap();
    assert!(matches!(
        other.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
    agent.execute(&p.plan_id, true).unwrap();
    assert_eq!(f.count.get(), 1);
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
}

#[test]
fn new_invalid_prompt_revokes_the_pending_plan() {
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let p = plan(agent.prompt("add").unwrap());
    assert!(matches!(agent.prompt(""), Err(AgentError::Input)));
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
    assert_eq!(f.count.get(), 0);
    assert_eq!(f.requests.borrow().len(), 1);
}

#[test]
fn policy_is_rechecked_at_dispatch() {
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    let p = plan(agent.prompt("add").unwrap());
    f.allowed.set(false);
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::Tool(ToolError::Denied))
    ));
    assert_eq!(f.count.get(), 0);
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
}

#[test]
fn unknown_tools_extra_fields_bad_arguments_and_malformed_json_never_dispatch() {
    for output in [
        "not json".to_owned(),
        call(11),
        json!({"kind":"call","tool":"shell.run","arguments":{}}).to_string(),
        json!({"kind":"call","tool":"counter.add","arguments":{"amount":1,"admin":true}})
            .to_string(),
        json!({"kind":"call","tool":"counter.add","arguments":[],"approved":true}).to_string(),
    ] {
        let f = fixture(vec![output]);
        let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
        assert!(agent.prompt("add").is_err());
        assert_eq!(f.count.get(), 0);
        assert!(matches!(
            agent.continue_run(),
            Err(AgentError::NoContinuation)
        ));
    }
}

#[test]
fn expired_plan_and_explicit_cancellation_prevent_dispatch() {
    let f = fixture(vec![call(1), call(1)]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            plan_ttl: Duration::from_millis(1),
            ..enabled()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("add").unwrap());
    thread::sleep(Duration::from_millis(5));
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::Expired)
    ));
    let p = plan(agent.prompt("add").unwrap());
    agent.cancel_pending().unwrap();
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
    assert_eq!(f.count.get(), 0);
}

#[test]
fn session_budgets_survive_new_tasks() {
    let f = fixture(vec![call(1), call(1)]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_model_calls: 1,
            ..enabled()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("add").unwrap());
    agent.execute(&p.plan_id, true).unwrap();
    assert!(matches!(agent.prompt("again"), Err(AgentError::Budget)));
    assert_eq!(f.requests.borrow().len(), 1);
    let f = fixture(vec![call(1), call(1)]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_tool_calls: 1,
            ..enabled()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("add").unwrap());
    agent.execute(&p.plan_id, true).unwrap();
    assert!(matches!(agent.continue_run(), Err(AgentError::Budget)));
    assert_eq!(f.count.get(), 1);
}

struct FailingJournal(EventKind);
impl Journal for FailingJournal {
    fn record(&mut self, event: Event) -> Result<(), JournalError> {
        if event.kind == self.0 {
            Err(JournalError)
        } else {
            Ok(())
        }
    }
}

#[test]
fn journal_failure_before_dispatch_is_closed_and_completion_failure_cannot_retry() {
    for fail in [EventKind::DispatchStarted, EventKind::Completed] {
        let f = fixture(vec![call(1)]);
        let mut agent =
            Agent::with_journal(f.provider, f.tools, enabled(), FailingJournal(fail)).unwrap();
        let p = plan(agent.prompt("add").unwrap());
        let result = agent.execute(&p.plan_id, true);
        if fail == EventKind::DispatchStarted {
            assert!(matches!(result, Err(AgentError::Journal(_))));
            assert_eq!(f.count.get(), 0);
        } else {
            let receipt = result.unwrap();
            assert!(!receipt.completion_recorded);
            assert!(!receipt.continuation_available);
            assert_eq!(f.count.get(), 1);
        }
        assert!(matches!(
            agent.execute(&p.plan_id, true),
            Err(AgentError::UnknownPlan)
        ));
    }
}

#[test]
fn failed_or_oversized_tool_result_is_not_retried_or_fed_to_model() {
    for fail in [true, false] {
        let mut f = fixture(vec![call(1)]);
        let count = f.count.clone();
        f.tools = ToolRegistry::new();
        f.tools
            .register(TypedTool::<Args, _, _>::new(
                "counter.add",
                "test",
                |_| Ok(()),
                move |_, _| {
                    count.set(count.get() + 1);
                    if fail {
                        Err(ToolError::Unavailable)
                    } else {
                        Ok(json!("X".repeat(5000)))
                    }
                },
            ))
            .unwrap();
        let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
        let p = plan(agent.prompt("add").unwrap());
        let receipt = agent.execute(&p.plan_id, true).unwrap();
        assert_eq!(receipt.status, ExecutionStatus::OutcomeUnknown);
        assert!(receipt.output.is_none());
        assert!(matches!(
            agent.continue_run(),
            Err(AgentError::NoContinuation)
        ));
        assert!(matches!(
            agent.execute(&p.plan_id, true),
            Err(AgentError::UnknownPlan)
        ));
        assert_eq!(f.count.get(), 1);
    }
}

#[test]
fn input_output_and_context_limits_stop_work() {
    let f = fixture(vec![call(1)]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_input_bytes: 2,
            ..enabled()
        },
    )
    .unwrap();
    assert!(matches!(agent.prompt("long"), Err(AgentError::Input)));
    assert!(f.requests.borrow().is_empty());
    let f = fixture(vec!["X".repeat(5000)]);
    let mut agent = Agent::new(f.provider, f.tools, enabled()).unwrap();
    assert!(matches!(
        agent.prompt("add"),
        Err(AgentError::Model(ModelError::ResponseTooLarge))
    ));
    let f = fixture(vec![call(1)]);
    assert!(matches!(
        Agent::new(
            f.provider,
            f.tools,
            AgentConfig {
                max_context_bytes: 10,
                ..enabled()
            }
        ),
        Err(AgentError::Context)
    ));
}

#[test]
fn tool_registry_rejects_duplicate_names_and_journal_retention_is_bounded() {
    let mut f = fixture(vec![]);
    assert!(
        f.tools
            .register(TypedTool::<Args, _, _>::new(
                "counter.add",
                "duplicate",
                |_| Ok(()),
                |_, _| Ok(json!(null))
            ))
            .is_err()
    );
    let mut journal = MemoryJournal::new(2);
    for _ in 0..4 {
        journal
            .record(Event {
                kind: EventKind::Final,
                plan_id: None,
                tool: None,
            })
            .unwrap();
    }
    assert_eq!(journal.events().len(), 2);
}

#[test]
fn required_observation_blocks_fabricated_final_but_allows_clarification() {
    let f = fixture(vec![
        json!({"kind":"final","message":"Count is 999"}).to_string(),
    ]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            require_tool_observation: true,
            ..enabled()
        },
    )
    .unwrap();
    assert!(matches!(
        agent.prompt("count"),
        Err(AgentError::ObservationRequired)
    ));
    assert_eq!(f.count.get(), 0);
    let schema = &f.requests.borrow()[0].response_schema;
    assert!(
        !schema["anyOf"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["properties"]["kind"]["const"] == "final")
    );
    let f = fixture(vec![
        json!({"kind":"clarify","message":"Which counter?"}).to_string(),
    ]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            require_tool_observation: true,
            ..enabled()
        },
    )
    .unwrap();
    assert!(matches!(
        agent.prompt("count"),
        Ok(Step::Clarification { .. })
    ));
}

#[test]
fn observation_requirement_resets_between_tasks() {
    let f = fixture(vec![
        call(1),
        json!({"kind":"final","message":"One"}).to_string(),
        json!({"kind":"final","message":"Two"}).to_string(),
    ]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            require_tool_observation: true,
            ..enabled()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("add").unwrap());
    agent.execute(&p.plan_id, true).unwrap();
    assert!(matches!(agent.continue_run(), Ok(Step::Final { .. })));
    assert!(matches!(
        agent.prompt("another task"),
        Err(AgentError::ObservationRequired)
    ));
}

#[test]
fn exhausted_tool_budget_removes_calls_but_allows_a_grounded_final() {
    let f = fixture(vec![
        call(1),
        json!({"kind":"final","message":"Count is 1"}).to_string(),
    ]);
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            max_tool_calls: 1,
            require_tool_observation: true,
            ..enabled()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("count").unwrap());
    agent.execute(&p.plan_id, true).unwrap();
    assert!(matches!(agent.continue_run(), Ok(Step::Final { .. })));
    let requests = f.requests.borrow();
    let variants = requests[1].response_schema["anyOf"].as_array().unwrap();
    assert!(
        !variants
            .iter()
            .any(|v| v["properties"]["kind"]["const"] == "call")
    );
    assert!(
        variants
            .iter()
            .any(|v| v["properties"]["kind"]["const"] == "final")
    );
    assert_eq!(f.count.get(), 1);
}

#[test]
fn recursive_argument_references_resolve_inside_their_own_tool_namespace() {
    let schema = json!({"type":"object","properties":{"next":{"$ref":"#"},"child":{"$ref":"#/definitions/Child"}},"definitions":{"Child":{"type":"string"}}});
    let first = ToolDefinition {
        name: "first".into(),
        description: "first".into(),
        parameters: schema.clone(),
    };
    let second = ToolDefinition {
        name: "second".into(),
        description: "second".into(),
        parameters: schema,
    };
    let combined = model::decision_schema(&[&first, &second]);
    for index in 0..2 {
        let arguments = &combined["anyOf"][index]["properties"]["arguments"];
        let root = arguments["properties"]["next"]["$ref"].as_str().unwrap();
        let child = arguments["properties"]["child"]["$ref"].as_str().unwrap();
        assert_eq!(root, format!("#/definitions/tool_{index}"));
        assert!(combined.pointer(root.strip_prefix('#').unwrap()).is_some());
        assert_eq!(
            combined.pointer(child.strip_prefix('#').unwrap()).unwrap()["type"],
            "string"
        );
    }
}

#[test]
fn late_tool_completion_has_unknown_outcome_and_no_continuation() {
    let mut f = fixture(vec![call(1)]);
    f.tools = ToolRegistry::new();
    let count = f.count.clone();
    f.tools
        .register(TypedTool::<Args, _, _>::new(
            "counter.add",
            "Slow fixture",
            |_| Ok(()),
            move |args, context| {
                assert_eq!(context.timeout, Duration::from_millis(1));
                count.set(count.get() + args.amount);
                thread::sleep(Duration::from_millis(5));
                Ok(json!({"count":count.get()}))
            },
        ))
        .unwrap();
    let mut agent = Agent::new(
        f.provider,
        f.tools,
        AgentConfig {
            tool_timeout: Duration::from_millis(1),
            ..enabled()
        },
    )
    .unwrap();
    let p = plan(agent.prompt("add").unwrap());
    let receipt = agent.execute(&p.plan_id, true).unwrap();
    assert_eq!(receipt.status, ExecutionStatus::OutcomeUnknown);
    assert!(!receipt.continuation_available);
    assert_eq!(f.count.get(), 1);
    assert!(matches!(
        agent.execute(&p.plan_id, true),
        Err(AgentError::UnknownPlan)
    ));
}
