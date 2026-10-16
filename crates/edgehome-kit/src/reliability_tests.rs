use super::*;
use crate::tests::Temp;
use edgehome_runtime::{Message, ModelError, ModelRequest};
use std::{cell::Cell, fs, rc::Rc, time::Duration};

struct Fixed(&'static str);
impl ModelProvider for Fixed {
    fn complete(&mut self, _: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
        Ok(self.0.into())
    }
}
fn wrong() -> Box<dyn ModelProvider> {
    Box::new(Fixed(
        r#"{"kind":"call","tool":"document.stats","arguments":{"document":"sample"}}"#,
    ))
}
fn suite(path: &std::path::Path, question: bool) {
    let expect = if question {
        json!({"kind":"clarify","message_contains_any":["document"]})
    } else {
        json!({"kind":"tool","tool":"document.stats","arguments":{"document":"empty"},"output":{"characters":0,"lines":0}})
    };
    fs::write(
        path,
        json!({"schema_version":2,"cases":[{
            "id":"test","input":"Count the document.","tools":["document.stats"],"expect":expect
        }]})
        .to_string(),
    )
    .unwrap();
}
#[test]
fn evaluation_does_not_use_the_answer_key_to_veto_wrong_allowed_execution() {
    let temp = Temp::new();
    let path = temp.path().join("cases.json");
    for clarification in [false, true] {
        suite(&path, clarification);
        let report = eval::evaluate(
            &path,
            &AppConfig::default(),
            true,
            &BTreeSet::from(["document.stats".into()]),
            &starter::registry,
            &wrong,
            eval::EvalOptions {
                repeat: 3,
                include_responses: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (report.total, report.passed, report.unexpected_proposals),
            (3, 0, 3)
        );
        assert!(report.cases.iter().all(|c| c.tool_calls == 1
            && c.observed.as_ref().unwrap()["execution"]["status"] == "succeeded"));
        assert_eq!(report.cases_passing_all_repeats, 0);
    }
}
#[test]
fn generic_clarification_does_not_pass_question_quality_check() {
    let temp = Temp::new();
    let path = temp.path().join("cases.json");
    suite(&path, true);
    let report = eval::evaluate(
        &path,
        &AppConfig::default(),
        true,
        &BTreeSet::new(),
        &starter::registry,
        &|| {
            Box::new(Fixed(
                r#"{"kind":"clarify","message":"a specific question"}"#,
            ))
        },
        eval::EvalOptions::default(),
    )
    .unwrap();
    assert_eq!(report.cases[0].outcome, "irrelevant_clarification");
    assert!(report.cases[0].responses.is_none());
    assert!(report.cases[0].observed.is_none());
}
#[test]
fn repeated_evaluation_retains_each_trial_and_limits_repetitions() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cases/sdk-starter.json");
    let allowed = BTreeSet::from(["document.stats".into(), "sensor.read".into()]);
    let report = eval::evaluate(
        &path,
        &AppConfig::default(),
        true,
        &allowed,
        &starter::registry,
        &starter::mock,
        eval::EvalOptions {
            repeat: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        (
            report.total,
            report.passed,
            report.unique_cases,
            report.cases_passing_all_repeats
        ),
        (24, 24, 8, 8)
    );
    assert_eq!(report.first_attempt_passed, 24);
    for repeat in [0, 11] {
        assert!(
            eval::evaluate(
                &path,
                &AppConfig::default(),
                true,
                &allowed,
                &starter::registry,
                &starter::mock,
                eval::EvalOptions {
                    repeat,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
#[test]
fn invalid_suite_is_rejected_before_model_factory_or_tool_execution() {
    let temp = Temp::new();
    let path = temp.path().join("cases.json");
    fs::write(&path,json!({"schema_version":2,"cases":[
        {"id":"valid","input":"sample document","tools":["document.stats"],"expect":{"kind":"clarify","message_contains_any":["document"]}},
        {"id":"invalid","input":"hello","tools":["unknown"],"expect":{"kind":"clarify","message_contains_any":["which"]}}
    ]}).to_string()).unwrap();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let factory = || {
        observed.set(observed.get() + 1);
        starter::mock()
    };
    assert!(
        eval::evaluate(
            &path,
            &AppConfig::default(),
            true,
            &BTreeSet::new(),
            &starter::registry,
            &factory,
            Default::default()
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
}
#[test]
fn role_probe_rejects_constant_answer_and_keeps_answer_out_of_user_and_schema() {
    let limits = ModelLimits {
        timeout: Duration::from_secs(1),
        max_request_bytes: 16384,
        max_output_bytes: 4096,
    };
    assert!(probe::check(&mut Fixed(r#"{"token":"cobalt"}"#), limits).is_err());
    struct Echo;
    impl ModelProvider for Echo {
        fn complete(
            &mut self,
            request: &ModelRequest,
            _: ModelLimits,
        ) -> Result<String, ModelError> {
            let Message { role, content } = &request.messages[0];
            assert_eq!(role, "system");
            let token = if content.contains("cobalt") {
                "cobalt"
            } else {
                "saffron"
            };
            assert!(!request.messages[1].content.contains(token));
            assert!(!request.response_schema.to_string().contains(token));
            Ok(json!({"token":token}).to_string())
        }
    }
    probe::check(&mut Echo, limits).unwrap();
}
