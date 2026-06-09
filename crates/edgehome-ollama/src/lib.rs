//! Ollama adapter, MiniCPM5 profile, and output governor.
//!
//! Ollama structured outputs help constrain JSON syntax. This crate still treats
//! the model response as an untrusted candidate and runs it through an output
//! governor plus the parser/schema validator before the rest of the harness sees
//! a `ModelCandidate`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use edgehome_config::RuntimeProfile;
use edgehome_core::{ModelCandidate, model_candidate_schema_json};
use edgehome_parser::{ParserError, parse_model_output};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

pub type OllamaResult<T> = Result<T, OllamaError>;

#[derive(Debug, Error)]
pub enum OllamaError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ollama returned status {status}: {body}")]
    HttpStatus { status: u16, body: String },

    #[error("unsupported ollama base URL: {0}")]
    UnsupportedBaseUrl(String),

    #[error("invalid http response: {0}")]
    InvalidHttpResponse(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("parser error: {0}")]
    Parser(#[from] ParserError),

    #[error("output rejected: {kind:?}: {message}")]
    OutputRejected {
        kind: OutputFailureKind,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MiniCpm5Profile {
    pub model_name: String,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub num_ctx: u32,
    pub num_predict: u32,
    pub timeout_ms: u64,
    pub retry_count: u8,
}

impl MiniCpm5Profile {
    pub fn from_runtime_profile(profile: &RuntimeProfile) -> Self {
        Self {
            model_name: profile.model_name.clone(),
            temperature: profile.temperature,
            top_p: profile.top_p,
            top_k: profile.top_k,
            repeat_penalty: profile.repeat_penalty,
            num_ctx: profile.num_ctx,
            num_predict: profile.num_predict,
            timeout_ms: profile.timeout_ms,
            retry_count: profile.retry_count,
        }
    }

    pub fn low_memory_default() -> Self {
        Self::from_runtime_profile(&RuntimeProfile::low_memory())
    }

    pub fn options(&self) -> ModelOptions {
        ModelOptions {
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            repeat_penalty: self.repeat_penalty,
            num_ctx: self.num_ctx,
            num_predict: self.num_predict,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelOptions {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub num_ctx: u32,
    pub num_predict: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_owned(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredOutputRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub format: Value,
    pub options: ModelOptions,
    pub stream: bool,
}

impl StructuredOutputRequest {
    pub fn new(profile: &MiniCpm5Profile, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: profile.model_name.clone(),
            messages,
            format: model_candidate_schema_json(),
            options: profile.options(),
            stream: false,
        }
    }

    pub fn for_prompt(
        profile: &MiniCpm5Profile,
        system_prompt: impl Into<String>,
        user_prompt: impl Into<String>,
    ) -> Self {
        Self::new(
            profile,
            vec![
                ChatMessage::system(system_prompt),
                ChatMessage::user(user_prompt),
            ],
        )
    }

    pub fn to_ollama_payload(&self) -> Value {
        json!({
            "model": self.model,
            "messages": self.messages,
            "format": self.format,
            "options": self.options,
            "stream": self.stream,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredOutputResponse {
    pub model: String,
    pub raw_content: String,
    pub done: bool,
    pub total_duration: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    timeout: Duration,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            timeout: Duration::from_secs(8),
        }
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout = Duration::from_millis(timeout_ms);
        self
    }

    pub fn chat_structured(
        &self,
        request: &StructuredOutputRequest,
    ) -> OllamaResult<StructuredOutputResponse> {
        let endpoint = HttpEndpoint::parse(&self.base_url)?;
        let body = serde_json::to_string(&request.to_ollama_payload())?;
        let response = post_json(&endpoint, "/api/chat", &body, self.timeout)?;
        if !(200..300).contains(&response.status) {
            return Err(OllamaError::HttpStatus {
                status: response.status,
                body: response.body,
            });
        }

        let decoded: OllamaChatResponse = serde_json::from_str(&response.body)?;
        Ok(StructuredOutputResponse {
            model: decoded.model,
            raw_content: decoded.message.content,
            done: decoded.done,
            total_duration: decoded.total_duration,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    model: String,
    message: ChatMessage,
    done: bool,
    total_duration: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
    path_prefix: String,
}

impl HttpEndpoint {
    fn parse(base_url: &str) -> OllamaResult<Self> {
        let Some(rest) = base_url.strip_prefix("http://") else {
            return Err(OllamaError::UnsupportedBaseUrl(base_url.to_owned()));
        };
        let (authority, path_prefix) = rest.split_once('/').unwrap_or((rest, ""));
        let (host, port) = if let Some((host, port)) = authority.split_once(':') {
            let parsed_port = port
                .parse::<u16>()
                .map_err(|_| OllamaError::UnsupportedBaseUrl(base_url.to_owned()))?;
            (host.to_owned(), parsed_port)
        } else {
            (authority.to_owned(), 80)
        };

        if host.trim().is_empty() {
            return Err(OllamaError::UnsupportedBaseUrl(base_url.to_owned()));
        }

        Ok(Self {
            host,
            port,
            path_prefix: if path_prefix.is_empty() {
                String::new()
            } else {
                format!("/{}", path_prefix.trim_end_matches('/'))
            },
        })
    }

    fn path(&self, suffix: &str) -> String {
        format!("{}{}", self.path_prefix, suffix)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpResponse {
    status: u16,
    body: String,
}

fn post_json(
    endpoint: &HttpEndpoint,
    suffix: &str,
    body: &str,
    timeout: Duration,
) -> OllamaResult<HttpResponse> {
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        endpoint.path(suffix),
        endpoint.host,
        body.len(),
        body
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    parse_http_response(&response)
}

fn parse_http_response(raw: &[u8]) -> OllamaResult<HttpResponse> {
    let response = String::from_utf8_lossy(raw);
    let Some((head, body)) = response.split_once("\r\n\r\n") else {
        return Err(OllamaError::InvalidHttpResponse(
            "missing header/body separator".to_owned(),
        ));
    };
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| OllamaError::InvalidHttpResponse("missing status line".to_owned()))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| OllamaError::InvalidHttpResponse("missing status code".to_owned()))?
        .parse::<u16>()
        .map_err(|_| OllamaError::InvalidHttpResponse("invalid status code".to_owned()))?;

    let chunked = lines.any(|line| {
        let lowered = line.to_ascii_lowercase();
        lowered.starts_with("transfer-encoding:") && lowered.contains("chunked")
    });
    let body = if chunked {
        decode_chunked_body(body)?
    } else {
        body.to_owned()
    };

    Ok(HttpResponse { status, body })
}

fn decode_chunked_body(body: &str) -> OllamaResult<String> {
    let mut remaining = body;
    let mut output = String::new();
    loop {
        let Some((size_hex, rest)) = remaining.split_once("\r\n") else {
            return Err(OllamaError::InvalidHttpResponse(
                "invalid chunk header".to_owned(),
            ));
        };
        let size = usize::from_str_radix(size_hex.trim(), 16)
            .map_err(|_| OllamaError::InvalidHttpResponse("invalid chunk size".to_owned()))?;
        if size == 0 {
            return Ok(output);
        }
        if rest.len() < size + 2 {
            return Err(OllamaError::InvalidHttpResponse(
                "chunk shorter than declared size".to_owned(),
            ));
        }
        output.push_str(&rest[..size]);
        remaining = &rest[size + 2..];
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputGovernorConfig {
    pub max_output_bytes: usize,
    pub max_output_chars: usize,
    pub retry_count: u8,
}

impl OutputGovernorConfig {
    pub fn from_profile(profile: &MiniCpm5Profile) -> Self {
        let max_output_chars = (profile.num_predict as usize).saturating_mul(8).max(256);
        Self {
            max_output_bytes: max_output_chars.saturating_mul(4),
            max_output_chars,
            retry_count: profile.retry_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFailureKind {
    Timeout,
    Empty,
    TooManyBytes,
    TooManyChars,
    DeadLoop,
    InvalidJson,
    SchemaFailed,
}

#[derive(Debug, Clone)]
pub struct OutputGovernor {
    config: OutputGovernorConfig,
}

impl OutputGovernor {
    pub fn new(config: OutputGovernorConfig) -> Self {
        Self { config }
    }

    pub fn from_profile(profile: &MiniCpm5Profile) -> Self {
        Self::new(OutputGovernorConfig::from_profile(profile))
    }

    pub fn govern(&self, raw: &str) -> OllamaResult<ModelCandidate> {
        self.inspect(raw)?;
        parse_model_output(raw).map_err(|error| match error {
            ParserError::JsonNotFound
            | ParserError::InvalidJson(_)
            | ParserError::DuplicateKey(_) => OllamaError::OutputRejected {
                kind: OutputFailureKind::InvalidJson,
                message: error.to_string(),
            },
            ParserError::InvalidSchemaVersion(_)
            | ParserError::SchemaValidation(_)
            | ParserError::BrightnessOutOfRange(_) => OllamaError::OutputRejected {
                kind: OutputFailureKind::SchemaFailed,
                message: error.to_string(),
            },
            other => OllamaError::Parser(other),
        })
    }

    pub fn inspect(&self, raw: &str) -> OllamaResult<()> {
        if raw.trim().is_empty() {
            return Err(rejected(OutputFailureKind::Empty, "model output is empty"));
        }
        if raw.len() > self.config.max_output_bytes {
            return Err(rejected(
                OutputFailureKind::TooManyBytes,
                format!(
                    "model output bytes {} exceed {}",
                    raw.len(),
                    self.config.max_output_bytes
                ),
            ));
        }
        let char_count = raw.chars().count();
        if char_count > self.config.max_output_chars {
            return Err(rejected(
                OutputFailureKind::TooManyChars,
                format!(
                    "model output chars {} exceed {}",
                    char_count, self.config.max_output_chars
                ),
            ));
        }
        if detect_dead_loop(raw) {
            return Err(rejected(
                OutputFailureKind::DeadLoop,
                "model output appears to repeat the same fragment",
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackMode {
    FullJson,
    CompactJson,
    EnumOnly,
    RuleOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u8,
}

impl RetryPolicy {
    pub fn from_profile(profile: &MiniCpm5Profile) -> Self {
        Self {
            max_retries: profile.retry_count,
        }
    }

    pub fn mode_for_attempt(&self, attempt: u8, failure: OutputFailureKind) -> FallbackMode {
        if attempt == 0 {
            return FallbackMode::FullJson;
        }
        if attempt <= self.max_retries {
            return match failure {
                OutputFailureKind::InvalidJson
                | OutputFailureKind::SchemaFailed
                | OutputFailureKind::DeadLoop
                | OutputFailureKind::TooManyChars
                | OutputFailureKind::TooManyBytes => FallbackMode::CompactJson,
                OutputFailureKind::Timeout | OutputFailureKind::Empty => FallbackMode::EnumOnly,
            };
        }
        FallbackMode::RuleOnly
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelHealth {
    pub consecutive_timeouts: u32,
    pub consecutive_invalid_outputs: u32,
    pub consecutive_dead_loops: u32,
    pub circuit_breaker_threshold: u32,
}

impl Default for ModelHealth {
    fn default() -> Self {
        Self {
            consecutive_timeouts: 0,
            consecutive_invalid_outputs: 0,
            consecutive_dead_loops: 0,
            circuit_breaker_threshold: 3,
        }
    }
}

impl ModelHealth {
    pub fn record_success(&mut self) {
        self.consecutive_timeouts = 0;
        self.consecutive_invalid_outputs = 0;
        self.consecutive_dead_loops = 0;
    }

    pub fn record_failure(&mut self, failure: OutputFailureKind) {
        match failure {
            OutputFailureKind::Timeout => self.consecutive_timeouts += 1,
            OutputFailureKind::DeadLoop => self.consecutive_dead_loops += 1,
            OutputFailureKind::InvalidJson
            | OutputFailureKind::SchemaFailed
            | OutputFailureKind::TooManyBytes
            | OutputFailureKind::TooManyChars
            | OutputFailureKind::Empty => self.consecutive_invalid_outputs += 1,
        }
    }

    pub fn circuit_open(&self) -> bool {
        self.consecutive_timeouts >= self.circuit_breaker_threshold
            || self.consecutive_invalid_outputs >= self.circuit_breaker_threshold
            || self.consecutive_dead_loops >= self.circuit_breaker_threshold
    }
}

fn rejected(kind: OutputFailureKind, message: impl Into<String>) -> OllamaError {
    OllamaError::OutputRejected {
        kind,
        message: message.into(),
    }
}

fn detect_dead_loop(raw: &str) -> bool {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() < 32 {
        return false;
    }

    for window in [4usize, 6, 8, 12, 16, 24] {
        if repeated_window(&compact, window, 4) {
            return true;
        }
    }

    let mut previous = "";
    let mut repeated_lines = 0usize;
    for line in raw.lines().map(str::trim).filter(|line| line.len() >= 8) {
        if line == previous {
            repeated_lines += 1;
            if repeated_lines >= 3 {
                return true;
            }
        } else {
            previous = line;
            repeated_lines = 1;
        }
    }

    false
}

fn repeated_window(value: &str, window: usize, threshold: usize) -> bool {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() < window * threshold {
        return false;
    }

    for start in 0..=chars.len() - (window * threshold) {
        let fragment = &chars[start..start + window];
        let mut count = 1usize;
        while count < threshold {
            let offset = start + window * count;
            if &chars[offset..offset + window] != fragment {
                break;
            }
            count += 1;
        }
        if count >= threshold {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_candidate_json() -> String {
        json!({
            "schema_version": "model_output.v1",
            "intent": "control_device",
            "room": "hallway",
            "device_alias": "走廊灯",
            "device_type": "light",
            "action": "set_brightness",
            "params": {
                "brightness": 30,
                "time_after": "22:00"
            }
        })
        .to_string()
    }

    #[test]
    fn minicpm5_profile_uses_low_memory_runtime_limits() {
        let profile = MiniCpm5Profile::low_memory_default();

        assert_eq!(profile.model_name, "openbmb/minicpm5:1b");
        assert!(profile.num_ctx <= 1024);
        assert!(profile.num_predict <= 128);
        assert!(profile.retry_count <= 1);
    }

    #[test]
    fn structured_output_request_contains_schema_and_options() {
        let profile = MiniCpm5Profile::low_memory_default();
        let request = StructuredOutputRequest::for_prompt(&profile, "system", "user");
        let payload = request.to_ollama_payload();

        assert_eq!(payload["model"], "openbmb/minicpm5:1b");
        assert_eq!(payload["stream"], false);
        assert!(payload["format"].get("properties").is_some());
        assert_eq!(payload["options"]["num_ctx"], 1024);
        assert_eq!(payload["messages"][0]["role"], "system");
    }

    #[test]
    fn output_governor_accepts_think_block_wrapped_json() {
        let profile = MiniCpm5Profile::low_memory_default();
        let governor = OutputGovernor::from_profile(&profile);
        let raw = format!("<think>hidden</think>\n{}", valid_candidate_json());

        let candidate = governor.govern(&raw).expect("candidate");

        assert_eq!(candidate.params.brightness, Some(30));
    }

    #[test]
    fn output_governor_rejects_overlong_output() {
        let governor = OutputGovernor::new(OutputGovernorConfig {
            max_output_bytes: 64,
            max_output_chars: 64,
            retry_count: 1,
        });

        let error = governor.inspect(&"x".repeat(128)).expect_err("too long");

        assert!(matches!(
            error,
            OllamaError::OutputRejected {
                kind: OutputFailureKind::TooManyBytes,
                ..
            }
        ));
    }

    #[test]
    fn output_governor_rejects_dead_loop() {
        let governor = OutputGovernor::new(OutputGovernorConfig {
            max_output_bytes: 2048,
            max_output_chars: 2048,
            retry_count: 1,
        });

        let error = governor
            .inspect("重复输出重复输出重复输出重复输出重复输出重复输出")
            .expect_err("dead loop");

        assert!(matches!(
            error,
            OllamaError::OutputRejected {
                kind: OutputFailureKind::DeadLoop,
                ..
            }
        ));
    }

    #[test]
    fn retry_policy_degrades_to_rule_only_after_budget() {
        let policy = RetryPolicy { max_retries: 1 };

        assert_eq!(
            policy.mode_for_attempt(0, OutputFailureKind::InvalidJson),
            FallbackMode::FullJson
        );
        assert_eq!(
            policy.mode_for_attempt(1, OutputFailureKind::InvalidJson),
            FallbackMode::CompactJson
        );
        assert_eq!(
            policy.mode_for_attempt(2, OutputFailureKind::InvalidJson),
            FallbackMode::RuleOnly
        );
    }

    #[test]
    fn model_health_opens_circuit_after_repeated_timeouts() {
        let mut health = ModelHealth::default();

        health.record_failure(OutputFailureKind::Timeout);
        health.record_failure(OutputFailureKind::Timeout);
        assert!(!health.circuit_open());
        health.record_failure(OutputFailureKind::Timeout);
        assert!(health.circuit_open());
        health.record_success();
        assert!(!health.circuit_open());
    }
}
