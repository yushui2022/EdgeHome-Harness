//! Built-in, bounded model transports. Presets describe wire protocols, not a
//! claim that every model/version or physical device has been qualified.

use edgehome_runtime::{ModelError, ModelLimits, ModelProvider, ModelRequest};
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    #[default]
    Ollama,
    LlamaCpp,
    LmStudio,
    Vllm,
    Sglang,
    OpenAiCompatible,
}

impl Engine {
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::LlamaCpp => "http://127.0.0.1:8080/v1",
            Self::LmStudio => "http://127.0.0.1:1234/v1",
            Self::Vllm | Self::OpenAiCompatible => "http://127.0.0.1:8000/v1",
            Self::Sglang => "http://127.0.0.1:30000/v1",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InferenceConfig {
    pub engine: Engine,
    /// Empty selects the engine's localhost default. OpenAI-style URLs include /v1.
    pub base_url: String,
    pub model: String,
    /// Only the environment variable NAME belongs in checked-in configuration.
    pub api_key_env: Option<String>,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub max_response_bytes: usize,
    pub max_output_tokens: u32,
    /// Ollama request setting; other engines configure context at server startup.
    pub context_tokens: u32,
    /// Optional Ollama thinking control. Set false for supported thinking models
    /// when a short action response is required; omitted by default.
    pub think: Option<bool>,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            engine: Engine::Ollama,
            base_url: String::new(),
            model: String::new(),
            api_key_env: None,
            connect_timeout_ms: 2000,
            request_timeout_ms: 15000,
            max_response_bytes: 65536,
            max_output_tokens: 256,
            context_tokens: 2048,
            think: None,
        }
    }
}

pub struct HttpProvider {
    config: InferenceConfig,
    client: Client,
    endpoint: Url,
    api_key: Option<String>,
}

impl std::fmt::Debug for HttpProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpProvider")
            .field("engine", &self.config.engine)
            .finish_non_exhaustive()
    }
}

impl HttpProvider {
    pub fn new(config: InferenceConfig) -> Result<Self, ModelError> {
        if config.model.trim().is_empty()
            || config.model.len() > 256
            || config.connect_timeout_ms == 0
            || config.request_timeout_ms == 0
            || config.max_response_bytes == 0
            || config.max_response_bytes > 16 * 1024 * 1024
            || config.max_output_tokens == 0
            || config.context_tokens == 0
            || (config.think.is_some() && config.engine != Engine::Ollama)
        {
            return Err(ModelError::Configuration);
        }
        let base = if config.base_url.is_empty() {
            config.engine.default_base_url()
        } else {
            &config.base_url
        };
        let mut endpoint = Url::parse(base).map_err(|_| ModelError::Configuration)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(ModelError::Configuration);
        }
        let suffix = if config.engine == Engine::Ollama {
            "/api/chat"
        } else {
            "/chat/completions"
        };
        endpoint.set_path(&format!(
            "{}{suffix}",
            endpoint.path().trim_end_matches('/')
        ));
        let api_key = match &config.api_key_env {
            Some(name) => Some(
                std::env::var(name)
                    .ok()
                    .filter(|v| !v.trim().is_empty())
                    .ok_or(ModelError::Configuration)?,
            ),
            None => None,
        };
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(config.connect_timeout_ms))
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .redirect(Policy::none())
            // Local inference should not be routed through inherited web proxies.
            .no_proxy()
            .build().map_err(|_| ModelError::Configuration)?;
        Ok(Self {
            config,
            client,
            endpoint,
            api_key,
        })
    }

    pub fn engine(&self) -> Engine {
        self.config.engine
    }

    fn payload(&self, request: &ModelRequest) -> Value {
        if self.config.engine == Engine::Ollama {
            let mut payload = json!({
                "model":self.config.model, "messages":request.messages,
                "format":request.response_schema, "stream":false,
                "options":{"temperature":0,"num_predict":self.config.max_output_tokens,"num_ctx":self.config.context_tokens}
            });
            if let Some(think) = self.config.think {
                payload["think"] = json!(think);
            }
            payload
        } else {
            json!({
                "model":self.config.model, "messages":request.messages, "stream":false,
                "temperature":0, "max_tokens":self.config.max_output_tokens,
                "response_format":{"type":"json_schema","json_schema":{"name":"edgehome_decision","schema":request.response_schema}}
            })
        }
    }
}

impl ModelProvider for HttpProvider {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError> {
        if limits.timeout.is_zero() || limits.max_output_bytes == 0 || limits.max_request_bytes == 0
        {
            return Err(ModelError::Configuration);
        }
        let payload =
            serde_json::to_vec(&self.payload(request)).map_err(|_| ModelError::Configuration)?;
        if payload.len() > limits.max_request_bytes {
            return Err(ModelError::RequestTooLarge);
        }
        let deadline = limits
            .timeout
            .min(Duration::from_millis(self.config.request_timeout_ms));
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .timeout(deadline)
            .body(payload);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }
        let response = request.send().map_err(transport_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(ModelError::HttpStatus(status.as_u16()));
        }
        let cap = self.config.max_response_bytes;
        if response
            .content_length()
            .is_some_and(|len| len > cap as u64)
        {
            return Err(ModelError::ResponseTooLarge);
        }
        // Limit before allocation/JSON decoding, even with no Content-Length or
        // a chunked response. The request deadline remains active during reads.
        let mut bytes = Vec::with_capacity(cap.min(8192));
        response
            .take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        if bytes.len() > cap {
            return Err(ModelError::ResponseTooLarge);
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| ModelError::InvalidResponse)?;
        let content = if self.config.engine == Engine::Ollama {
            if body.get("done_reason").and_then(Value::as_str) == Some("length") {
                return Err(ModelError::TruncatedResponse);
            }
            if body.get("done").and_then(Value::as_bool) != Some(true)
                || body
                    .pointer("/message/tool_calls")
                    .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
                || body
                    .get("done_reason")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "stop")
            {
                return Err(ModelError::InvalidResponse);
            }
            body.pointer("/message/content").and_then(Value::as_str)
        } else {
            let reason = body
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str);
            if reason == Some("length") {
                return Err(ModelError::TruncatedResponse);
            }
            if reason == Some("content_filter")
                || body
                    .pointer("/choices/0/message/refusal")
                    .is_some_and(|v| !v.is_null())
            {
                return Err(ModelError::RefusedResponse);
            }
            if body
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
                != Some("stop")
                || body
                    .pointer("/choices/0/message/tool_calls")
                    .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
            {
                return Err(ModelError::InvalidResponse);
            }
            body.pointer("/choices/0/message/content")
                .and_then(Value::as_str)
        }
        .filter(|v| !v.trim().is_empty())
        .ok_or(ModelError::InvalidResponse)?;
        if content.len() > limits.max_output_bytes {
            return Err(ModelError::ResponseTooLarge);
        }
        Ok(content.to_owned())
    }
}

fn transport_error(error: reqwest::Error) -> ModelError {
    if error.is_timeout() {
        ModelError::Timeout
    } else {
        ModelError::Unavailable
    }
}

fn read_error(error: std::io::Error) -> ModelError {
    if error.kind() == std::io::ErrorKind::TimedOut {
        return ModelError::Timeout;
    }
    // io::Error::source() can skip the wrapped reqwest error and expose only
    // its private timeout cause. Inspect get_ref() before walking that chain.
    let mut source: Option<&(dyn std::error::Error + 'static)> = error.get_ref().map(|e| e as _);
    while let Some(item) = source {
        if item
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
        {
            return ModelError::Timeout;
        }
        source = item.source();
    }
    ModelError::Unavailable
}

#[cfg(test)]
mod tests;
