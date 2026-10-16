use super::*;
use edgehome_runtime::{Event, EventKind, Journal, MemoryJournal};
use std::{
    fs,
    io::Cursor,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
pub(crate) struct Temp(std::path::PathBuf);
impl Temp {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "edgehome-kit-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(self.0.parent(), Some(parent.as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("edgehome-kit-test-")
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn agent(enabled: bool) -> Agent<Box<dyn ModelProvider>, MemoryJournal> {
    Agent::new(
        starter::mock(),
        starter::registry().unwrap(),
        AppConfig::default().agent_config(enabled),
    )
    .unwrap()
}
fn exchange(agent: &mut Agent<Box<dyn ModelProvider>>, requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut output = Vec::new();
    rpc::serve(agent, &[], &mut Cursor::new(input), &mut output).unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn rpc_requires_matching_confirmation_and_rejects_replay() {
    let mut agent = agent(true);
    let Step::Proposal { plan } = agent.prompt("sample document").unwrap() else {
        panic!()
    };
    let replies = exchange(
        &mut agent,
        vec![
            json!({"id":1,"command":{"method":"execute","plan_id":plan.plan_id,"approved":false}}),
            json!({"id":2,"command":{"method":"execute","plan_id":plan.plan_id,"approved":true}}),
            json!({"id":3,"command":{"method":"execute","plan_id":plan.plan_id,"approved":true}}),
        ],
    );
    assert_eq!(replies[1]["code"], "confirmation_required");
    assert_eq!(
        replies[2]["result"]["output"],
        json!({"characters":84,"lines":2})
    );
    assert_eq!(replies[3]["code"], "unknown_plan");
    assert_eq!(agent.usage().tool_calls, 1);
}

#[test]
fn rpc_execution_is_disabled_without_process_opt_in() {
    let mut agent = agent(false);
    let Step::Proposal { plan } = agent.prompt("sample document").unwrap() else {
        panic!()
    };
    let replies = exchange(
        &mut agent,
        vec![json!({"id":1,"command":{"method":"execute","plan_id":plan.plan_id,"approved":true}})],
    );
    assert_eq!(replies[1]["code"], "execution_disabled");
    assert_eq!(agent.usage().tool_calls, 0);
}

#[test]
fn rpc_invalid_messages_never_run_tools_and_cancel_stops_future_prompts() {
    let mut agent = agent(true);
    let replies = exchange(
        &mut agent,
        vec![
            json!({"id":1,"command":{"method":"execute","plan_id":"x","approved":true,"arguments":{"admin":true}}}),
            json!({"id":2,"command":{"method":"cancel"}}),
            json!({"id":3,"command":{"method":"prompt","input":"sample document"}}),
        ],
    );
    assert_eq!(replies[1]["code"], "invalid_request");
    assert_eq!(replies[2]["result"]["cancelled"], true);
    assert_eq!(replies[3]["code"], "cancelled");
    assert_eq!(agent.usage().model_calls, 0);
    assert_eq!(agent.usage().tool_calls, 0);
}

#[test]
fn rpc_frame_limit_applies_before_json_parsing_or_dispatch() {
    let mut agent = agent(true);
    let mut output = Vec::new();
    assert!(
        rpc::serve(
            &mut agent,
            &[],
            &mut Cursor::new(vec![b'x'; rpc::MAX_FRAME_BYTES + 100]),
            &mut output
        )
        .is_err()
    );
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("frame_too_large")
    );
    assert_eq!(agent.usage().model_calls, 0);
}

#[test]
fn configuration_errors_do_not_echo_pasted_secrets() {
    let temp = Temp::new();
    let path = temp.path().join("agent.yaml");
    fs::write(&path, "inference:\n  secret_token: VERY-PRIVATE-VALUE\n").unwrap();
    let error = AppConfig::load(Some(&path)).unwrap_err().to_string();
    assert!(!error.contains("VERY-PRIVATE"));
    fs::write(&path, vec![b'x'; 65537]).unwrap();
    assert!(AppConfig::load(Some(&path)).is_err());
}

#[test]
fn scaffolding_creates_an_independent_app_and_preserves_existing_files() {
    let temp = Temp::new();
    let path = temp.path().join("my app");
    scaffold::create(&path, None, "my-agent").unwrap();
    assert!(
        fs::read_to_string(path.join("Cargo.toml"))
            .unwrap()
            .contains("[workspace]")
    );
    assert!(path.join("src/tools.rs").is_file());
    assert!(path.join("client.py").is_file());
    fs::write(path.join("src/main.rs"), "user content").unwrap();
    assert!(scaffold::create(&path, None, "my-agent").is_err());
    assert_eq!(
        fs::read_to_string(path.join("src/main.rs")).unwrap(),
        "user content"
    );
}

#[test]
fn starter_evaluation_checks_results_and_requires_explicit_tool_authority() {
    let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cases/sdk-starter.json");
    let allowed = BTreeSet::from(["document.stats".into(), "sensor.read".into()]);
    let report = eval::evaluate(
        &cases,
        &AppConfig::default(),
        true,
        &allowed,
        &starter::registry,
        &starter::mock,
        eval::EvalOptions::default(),
    )
    .unwrap();
    assert_eq!((report.passed, report.total), (8, 8));
    let denied = eval::evaluate(
        &cases,
        &AppConfig::default(),
        true,
        &BTreeSet::new(),
        &starter::registry,
        &starter::mock,
        eval::EvalOptions::default(),
    )
    .unwrap();
    assert_eq!(denied.passed, 2);
}

#[test]
fn journal_requires_a_new_file_and_keeps_raw_inputs_out() {
    let temp = Temp::new();
    let path = temp.path().join("session.jsonl");
    let mut journal = SessionJournal::new(Some(&path), false).unwrap();
    journal
        .record(Event {
            kind: EventKind::TaskStarted,
            plan_id: None,
            tool: None,
        })
        .unwrap();
    assert!(SessionJournal::new(Some(&path), false).is_err());
    drop(journal);
    let saved: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(saved["event"]["kind"], "task_started");
    assert!(saved["event"].get("input").is_none());
}

#[test]
fn cli_keeps_mock_execution_and_loop_opt_ins_explicit() {
    let args =
        Cli::try_parse_from(["agent", "run", "--mock", "--prompt", "sample document"]).unwrap();
    let Command::Run {
        allow,
        continue_loop,
        model,
        ..
    } = args.command
    else {
        panic!()
    };
    assert!(allow.is_empty());
    assert!(!continue_loop);
    assert!(model.mock);
    assert!(Cli::try_parse_from(["agent", "rpc", "--allow", "anything"]).is_err());
}
