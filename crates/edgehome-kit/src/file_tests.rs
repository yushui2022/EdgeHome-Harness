use super::*;
use crate::files::FileWorkspace;
use crate::tests::Temp;
use edgehome_runtime::*;
use std::{fs, io::Cursor};

struct One(String);
impl ModelProvider for One {
    fn complete(&mut self, _: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
        Ok(self.0.clone())
    }
}
fn agent(root: &Temp, tool: &str, path: Value) -> Agent<One> {
    let args = if tool == "file.list" {
        json!({})
    } else {
        json!({"path":path})
    };
    Agent::new(
        One(json!({"action":tool,"arguments":args}).to_string()),
        FileWorkspace::new(root.path())
            .unwrap()
            .registry(true)
            .unwrap(),
        AgentConfig {
            decision_protocol: DecisionProtocol::Action,
            execute_enabled: true,
            ..Default::default()
        },
    )
    .unwrap()
}
fn proposal(step: Step) -> PlanPreview {
    match step {
        Step::Proposal { plan } => plan,
        _ => panic!("expected plan"),
    }
}

#[test]
fn actual_files_clarify_ambiguity_then_read_exact_selected_path_over_rpc() {
    let root = Temp::new();
    for dir in ["one", "two"] {
        fs::create_dir(root.path().join(dir)).unwrap();
        fs::write(root.path().join(dir).join("r.txt"), dir).unwrap();
    }
    let mut a = agent(&root, "file.read", json!("r.txt"));
    assert!(matches!(
        a.prompt("read r.txt").unwrap(),
        Step::Clarification { .. }
    ));
    let q = a.clarification().unwrap().clone();
    assert_eq!(
        q.fields[0].choices,
        vec![json!("one/r.txt"), json!("two/r.txt")]
    );
    let command=json!({"id":1,"command":{"method":"resolve","clarification_id":q.clarification_id,"values":{"path":"two/r.txt"}}}).to_string()+"\n";
    let mut output = Vec::new();
    rpc::serve(&mut a, &[], &mut Cursor::new(command), &mut output).unwrap();
    let response: Value =
        serde_json::from_str(String::from_utf8(output).unwrap().lines().nth(1).unwrap()).unwrap();
    let receipt = a
        .execute(
            response["result"]["plan"]["plan_id"].as_str().unwrap(),
            true,
        )
        .unwrap();
    assert_eq!(
        receipt.output,
        Some(json!({"path":"two/r.txt","text":"two"}))
    );
    assert_eq!(a.usage().model_calls, 1);
}

#[test]
fn file_bounds_reject_escape_hidden_devices_binary_and_oversize_before_dispatch() {
    let root = Temp::new();
    fs::write(root.path().join("large.txt"), vec![b'x'; 16385]).unwrap();
    fs::write(root.path().join("binary.bin"), [255, 254, 0]).unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    fs::write(root.path().join(".secret"), "secret").unwrap();
    for path in [
        "../outside",
        "C:/Windows/win.ini",
        "/etc/passwd",
        "folder/../.secret",
        "file:stream",
        ".secret",
        "large.txt",
        "binary.bin",
        "folder",
        "foo.",
        "foo ",
        "a\\b",
    ] {
        let mut a = agent(&root, "file.read", json!(path));
        assert!(
            matches!(a.prompt("read"), Err(AgentError::Tool(ToolError::Denied))),
            "{path}"
        );
        assert_eq!(a.usage().tool_calls, 0);
    }
}

#[test]
fn clarification_choices_exclude_existing_but_unreadable_files() {
    let root = Temp::new();
    fs::write(root.path().join("large.txt"), vec![b'x'; 16385]).unwrap();
    fs::write(root.path().join("binary.bin"), [255, 254, 0]).unwrap();
    fs::write(root.path().join("ok.txt"), "readable").unwrap();
    let mut a = agent(&root, "file.read", json!("missing.txt"));
    a.prompt("read missing.txt").unwrap();
    assert_eq!(
        a.clarification().unwrap().fields[0].choices,
        vec![json!("ok.txt")]
    );
    for path in ["NUL", "CON.txt", "COM1", "LPT9.log"] {
        let mut a = agent(&root, "file.read", json!(path));
        assert!(matches!(
            a.prompt("read"),
            Err(AgentError::Tool(ToolError::Denied))
        ));
    }
}

#[test]
fn dispatch_rechecks_deleted_and_grown_files_and_read_statistics_are_real() {
    let root = Temp::new();
    let file = root.path().join("text.txt");
    fs::write(&file, "边缘\n").unwrap();
    let mut a = agent(&root, "file.stats", json!("text.txt"));
    let p = proposal(a.prompt("count").unwrap());
    assert_eq!(
        a.execute(&p.plan_id, true).unwrap().output,
        Some(json!({"path":"text.txt","bytes":7,"characters":3,"lines":1}))
    );
    let mut a = agent(&root, "file.read", json!("text.txt"));
    let p = proposal(a.prompt("read").unwrap());
    fs::write(&file, vec![b'x'; 16385]).unwrap();
    assert!(a.execute(&p.plan_id, true).is_err());
    assert_eq!(a.usage().tool_calls, 0);
    fs::write(&file, "ok").unwrap();
    let mut a = agent(&root, "file.read", json!("text.txt"));
    let p = proposal(a.prompt("read").unwrap());
    fs::remove_file(file).unwrap();
    fs::write(root.path().join("other.txt"), "other").unwrap();
    assert!(matches!(
        a.execute(&p.plan_id, true),
        Err(AgentError::ClarificationRequired)
    ));
    assert!(a.clarification().is_some());
    assert_eq!(a.usage().tool_calls, 0);
}

#[test]
fn directory_walk_and_output_limits_fail_closed() {
    let root = Temp::new();
    for i in 0..129 {
        fs::write(root.path().join(format!("{i}.txt")), "").unwrap();
    }
    let mut a = agent(&root, "file.list", Value::Null);
    let p = proposal(a.prompt("list").unwrap());
    assert_eq!(
        a.execute(&p.plan_id, true).unwrap().status,
        ExecutionStatus::OutcomeUnknown
    );
    let root = Temp::new();
    fs::write(root.path().join("text.txt"), "x".repeat(5000)).unwrap();
    let mut a = agent(&root, "file.read", json!("text.txt"));
    let p = proposal(a.prompt("read").unwrap());
    assert_eq!(
        a.execute(&p.plan_id, true).unwrap().status,
        ExecutionStatus::OutcomeUnknown
    );
}

#[test]
fn links_are_not_read_or_listed() {
    let root = Temp::new();
    let outside = Temp::new();
    fs::write(outside.path().join("private.txt"), "private").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    #[cfg(windows)]
    {
        // Directory junctions need no developer-mode symlink privilege.
        // Both paths are fresh test directories; cmd is used only for mklink.
        let result = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.path().join("linked"))
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(result.status.success(), "junction creation failed");
    }
    let mut a = agent(&root, "file.read", json!("linked/private.txt"));
    assert!(a.prompt("read").is_err());
    let mut a = agent(&root, "file.list", Value::Null);
    let p = proposal(a.prompt("list").unwrap());
    assert_eq!(
        a.execute(&p.plan_id, true).unwrap().output,
        Some(json!({"files":[]}))
    );
    // Remove only the known link, never recurse through its target.
    #[cfg(windows)]
    fs::remove_dir(root.path().join("linked")).unwrap();
    #[cfg(unix)]
    fs::remove_file(root.path().join("linked")).unwrap();
    assert!(outside.path().join("private.txt").exists());
}
