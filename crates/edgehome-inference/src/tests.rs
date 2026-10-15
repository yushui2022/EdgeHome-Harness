use super::*;
use edgehome_runtime::Message;
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
};

fn request() -> ModelRequest {
    ModelRequest {
        messages: vec![Message::new("user", "hello")],
        response_schema: json!({"type":"object"}),
    }
}
fn limits() -> ModelLimits {
    ModelLimits {
        timeout: Duration::from_secs(2),
        max_output_bytes: 4096,
        max_request_bytes: 16384,
    }
}
fn envelope(engine: Engine, text: &str) -> String {
    if engine == Engine::Ollama {
        json!({"message":{"role":"assistant","content":text},"done":true,"done_reason":"stop"})
            .to_string()
    } else {
        json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}]})
            .to_string()
    }
}
fn capture(stream: &mut TcpStream) -> (String, Value) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
    }
    let headers = String::from_utf8(bytes).unwrap();
    let len = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    let mut body = vec![0; len];
    stream.read_exact(&mut body).unwrap();
    (headers, serde_json::from_slice(&body).unwrap())
}
fn server(
    response: Vec<u8>,
    pause: Duration,
) -> (
    String,
    mpsc::Receiver<(String, Value)>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (send, receive) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let received = capture(&mut stream);
        send.send(received).unwrap();
        thread::sleep(pause);
        let _ = stream.write_all(&response);
    });
    (address, receive, handle)
}
fn response(body: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
}
fn config(engine: Engine, address: String) -> InferenceConfig {
    InferenceConfig {
        engine,
        base_url: if engine == Engine::Ollama {
            address
        } else {
            format!("{address}/v1")
        },
        model: "fixture-model".into(),
        ..InferenceConfig::default()
    }
}

#[test]
fn all_engine_presets_send_the_expected_wire_contract() {
    for engine in [
        Engine::Ollama,
        Engine::LlamaCpp,
        Engine::LmStudio,
        Engine::Vllm,
        Engine::Sglang,
        Engine::OpenAiCompatible,
    ] {
        let raw = r#"{"kind":"final","message":"hello"}"#;
        let (address, receive, handle) = server(response(&envelope(engine, raw)), Duration::ZERO);
        let mut provider = HttpProvider::new(config(engine, address)).unwrap();
        assert_eq!(provider.complete(&request(), limits()).unwrap(), raw);
        let (head, body) = receive.recv().unwrap();
        assert!(head.starts_with(if engine == Engine::Ollama {
            "POST /api/chat "
        } else {
            "POST /v1/chat/completions "
        }));
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "fixture-model");
        if engine == Engine::Ollama {
            assert_eq!(body["format"], request().response_schema);
            assert_eq!(body["options"]["num_ctx"], 2048);
        } else {
            assert_eq!(
                body["response_format"]["json_schema"]["schema"],
                request().response_schema
            );
            assert_eq!(body["max_tokens"], 256);
        }
        handle.join().unwrap();
    }
}

#[test]
fn response_caps_apply_to_announced_unannounced_and_chunked_bodies() {
    let body = "X".repeat(2048);
    for raw in [
        response(&body),
        format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}").into_bytes(),
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
            body.len()
        )
        .into_bytes(),
    ] {
        let (address, receive, handle) = server(raw, Duration::ZERO);
        let mut cfg = config(Engine::Ollama, address);
        cfg.max_response_bytes = 512;
        let mut provider = HttpProvider::new(cfg).unwrap();
        assert_eq!(
            provider.complete(&request(), limits()),
            Err(ModelError::ResponseTooLarge)
        );
        receive.recv().unwrap();
        handle.join().unwrap();
    }
}

#[test]
fn candidate_limit_is_separate_from_http_envelope_limit() {
    let (address, receive, handle) = server(
        response(&envelope(Engine::Ollama, &"X".repeat(100))),
        Duration::ZERO,
    );
    let mut provider = HttpProvider::new(config(Engine::Ollama, address)).unwrap();
    assert_eq!(
        provider.complete(
            &request(),
            ModelLimits {
                max_output_bytes: 30,
                ..limits()
            }
        ),
        Err(ModelError::ResponseTooLarge)
    );
    receive.recv().unwrap();
    handle.join().unwrap();
}

#[test]
fn total_deadline_covers_response_headers() {
    let (address, receive, handle) = server(
        response(&envelope(Engine::Ollama, "{}")),
        Duration::from_millis(150),
    );
    let mut provider = HttpProvider::new(config(Engine::Ollama, address)).unwrap();
    assert_eq!(
        provider.complete(
            &request(),
            ModelLimits {
                timeout: Duration::from_millis(30),
                ..limits()
            }
        ),
        Err(ModelError::Timeout)
    );
    receive.recv().unwrap();
    handle.join().unwrap();
}

#[test]
fn total_deadline_covers_slow_response_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        capture(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n{")
            .unwrap();
        thread::sleep(Duration::from_millis(150));
        let _ = stream.write_all(b"}");
    });
    let mut provider = HttpProvider::new(config(Engine::Ollama, address)).unwrap();
    assert_eq!(
        provider.complete(
            &request(),
            ModelLimits {
                timeout: Duration::from_millis(40),
                ..limits()
            }
        ),
        Err(ModelError::Timeout)
    );
    handle.join().unwrap();
}

#[test]
fn incoming_chunks_do_not_renew_the_total_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        capture(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{")
            .unwrap();
        for _ in 0..8 {
            thread::sleep(Duration::from_millis(25));
            if stream.write_all(b" ").is_err() {
                break;
            }
        }
    });
    let mut cfg = config(Engine::Ollama, address);
    cfg.request_timeout_ms = 80;
    let mut provider = HttpProvider::new(cfg).unwrap();
    assert_eq!(
        provider.complete(&request(), limits()),
        Err(ModelError::Timeout)
    );
    handle.join().unwrap();
}

#[test]
fn truncated_refused_missing_and_native_tool_call_responses_are_rejected() {
    for (body, expected) in [
        (
            json!({"choices":[{"message":{"content":"{}"},"finish_reason":"length"}]}),
            ModelError::TruncatedResponse,
        ),
        (
            json!({"choices":[{"message":{"content":"{}","refusal":"no"},"finish_reason":"stop"}]}),
            ModelError::RefusedResponse,
        ),
        (
            json!({"choices":[{"message":{"content":"{}"},"finish_reason":"content_filter"}]}),
            ModelError::RefusedResponse,
        ),
        (
            json!({"choices":[{"message":{"content":"{}","tool_calls":[{}]},"finish_reason":"stop"}]}),
            ModelError::InvalidResponse,
        ),
        (json!({"choices":[]}), ModelError::InvalidResponse),
    ] {
        let (address, receive, handle) = server(response(&body.to_string()), Duration::ZERO);
        let mut provider = HttpProvider::new(config(Engine::LlamaCpp, address)).unwrap();
        assert_eq!(provider.complete(&request(), limits()), Err(expected));
        receive.recv().unwrap();
        handle.join().unwrap();
    }
}

#[test]
fn upstream_errors_are_sanitized_and_redirects_are_not_followed() {
    for status in [500, 302] {
        let raw=format!("HTTP/1.1 {status} Error\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 12\r\n\r\nsecret-token").into_bytes();
        let (address, receive, handle) = server(raw, Duration::ZERO);
        let mut provider = HttpProvider::new(config(Engine::Ollama, address)).unwrap();
        let error = provider.complete(&request(), limits()).unwrap_err();
        assert_eq!(error, ModelError::HttpStatus(status));
        assert!(!error.to_string().contains("secret-token"));
        receive.recv().unwrap();
        handle.join().unwrap();
    }
}

#[test]
fn invalid_configuration_and_oversized_request_fail_before_network_io() {
    for url in [
        "file:///tmp/model",
        "http://user:secret@localhost",
        "http://localhost?token=secret",
        "http://localhost/#private",
    ] {
        assert!(matches!(
            HttpProvider::new(config(Engine::Ollama, url.into())),
            Err(ModelError::Configuration)
        ));
    }
    let mut provider =
        HttpProvider::new(config(Engine::Ollama, "http://127.0.0.1:1".into())).unwrap();
    assert_eq!(
        provider.complete(
            &request(),
            ModelLimits {
                max_request_bytes: 5,
                ..limits()
            }
        ),
        Err(ModelError::RequestTooLarge)
    );
    assert!(!format!("{provider:?}").contains("127.0.0.1"));
}

#[test]
fn checked_in_profiles_parse_and_thinking_is_an_explicit_ollama_setting() {
    for source in [
        include_str!("../../../configs/inference/ollama.yaml"),
        include_str!("../../../configs/inference/llama-cpp.yaml"),
        include_str!("../../../configs/inference/lm-studio.yaml"),
        include_str!("../../../configs/inference/vllm.yaml"),
        include_str!("../../../configs/inference/sglang.yaml"),
        include_str!("../../../configs/inference/openai-compatible.yaml"),
    ] {
        let cfg: InferenceConfig = serde_yaml::from_str(source).unwrap();
        assert!(HttpProvider::new(cfg).is_ok());
    }
    let mut cfg = config(Engine::Ollama, "http://127.0.0.1:1".into());
    assert!(
        HttpProvider::new(cfg.clone())
            .unwrap()
            .payload(&request())
            .get("think")
            .is_none()
    );
    cfg.think = Some(false);
    assert_eq!(
        HttpProvider::new(cfg.clone()).unwrap().payload(&request())["think"],
        false
    );
    cfg.engine = Engine::LlamaCpp;
    assert!(matches!(
        HttpProvider::new(cfg),
        Err(ModelError::Configuration)
    ));
}

#[test]
fn ollama_incomplete_generation_never_becomes_a_candidate() {
    for (body, expected) in [
        (
            json!({"done":true,"done_reason":"length","message":{"content":"{}"}}),
            ModelError::TruncatedResponse,
        ),
        (
            json!({"done":false,"message":{"content":"{}"}}),
            ModelError::InvalidResponse,
        ),
        (
            json!({"done":true,"message":{"content":""}}),
            ModelError::InvalidResponse,
        ),
        (
            json!({"done":true,"message":{"content":"{}","tool_calls":[{}]}}),
            ModelError::InvalidResponse,
        ),
    ] {
        let (address, receive, handle) = server(response(&body.to_string()), Duration::ZERO);
        let mut provider = HttpProvider::new(config(Engine::Ollama, address)).unwrap();
        assert_eq!(provider.complete(&request(), limits()), Err(expected));
        receive.recv().unwrap();
        handle.join().unwrap();
    }
}

#[test]
fn both_protocols_drive_the_runtime_through_a_real_http_tool_roundtrip() {
    use edgehome_runtime::{Agent, AgentConfig, Step, ToolRegistry, TypedTool};
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Args {
        key: String,
    }
    for engine in [Engine::Ollama, Engine::LlamaCpp] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let (_, body) = capture(&mut stream);
                let raw = if index == 0 {
                    json!({"kind":"call","tool":"record.read","arguments":{"key":"sample"}})
                } else {
                    let messages = body["messages"].as_array().unwrap();
                    let observation: Value =
                        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap())
                            .unwrap();
                    assert_eq!(observation["tool_result"]["output"]["count"], 84);
                    json!({"kind":"final","message":"84"})
                };
                stream
                    .write_all(&response(&envelope(engine, &raw.to_string())))
                    .unwrap();
            }
        });
        let provider = HttpProvider::new(config(engine, address)).unwrap();
        let mut tools = ToolRegistry::new();
        tools
            .register(TypedTool::<Args, _, _>::new(
                "record.read",
                "Read one fixture",
                |args| {
                    if args.key == "sample" {
                        Ok(())
                    } else {
                        Err(edgehome_runtime::ToolError::Arguments)
                    }
                },
                |_, _| Ok(json!({"count":84})),
            ))
            .unwrap();
        let mut agent = Agent::new(
            provider,
            tools,
            AgentConfig {
                execute_enabled: true,
                require_tool_observation: true,
                max_tool_calls: 1,
                ..AgentConfig::default()
            },
        )
        .unwrap();
        let Step::Proposal { plan } = agent.prompt("count sample").unwrap() else {
            panic!("expected plan")
        };
        assert!(
            agent
                .execute(&plan.plan_id, true)
                .unwrap()
                .continuation_available
        );
        assert!(
            matches!(agent.continue_run().unwrap(), Step::Final { message } if message == "84")
        );
        handle.join().unwrap();
    }
}
