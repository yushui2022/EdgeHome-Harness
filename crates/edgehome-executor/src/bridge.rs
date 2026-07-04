use std::{fmt, time::Duration};

use serde_json::Value;

use crate::{ExecutorError, ExecutorResult, evidence::sanitize_backend_text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgePostConfig {
    pub backend: &'static str,
    pub base_url: String,
    pub request_timeout_ms: u64,
}

#[derive(Clone)]
pub struct BridgeSecrets {
    token: String,
}

impl BridgeSecrets {
    pub fn new(
        backend: &'static str,
        token_env: impl Into<String>,
        token: impl Into<String>,
    ) -> ExecutorResult<Self> {
        let token = token.into().trim().to_owned();
        if token.is_empty() {
            return Err(ExecutorError::BridgeSecretMissing {
                backend: backend.to_owned(),
                env_var: token_env.into(),
            });
        }
        Ok(Self { token })
    }

    pub fn load(backend: &'static str, token_env: impl AsRef<str>) -> ExecutorResult<Option<Self>> {
        let token_env = token_env.as_ref().trim();
        if token_env.is_empty() {
            return Ok(None);
        }
        let Some(token) = std::env::var(token_env).ok().and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }) else {
            return Ok(None);
        };
        Self::new(backend, token_env, token).map(Some)
    }

    pub fn load_required(
        backend: &'static str,
        token_env: impl AsRef<str>,
    ) -> ExecutorResult<Self> {
        let token_env = token_env.as_ref();
        Self::load(backend, token_env)?.ok_or_else(|| ExecutorError::BridgeSecretMissing {
            backend: backend.to_owned(),
            env_var: token_env.to_owned(),
        })
    }

    fn token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for BridgeSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BridgeSecrets")
            .field("token", &"<redacted>")
            .finish()
    }
}

pub trait BridgePoster {
    fn post_json(
        &self,
        config: &BridgePostConfig,
        secrets: &BridgeSecrets,
        path: &str,
        payload: &Value,
    ) -> ExecutorResult<Value>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ReqwestBridgePoster;

impl BridgePoster for ReqwestBridgePoster {
    fn post_json(
        &self,
        config: &BridgePostConfig,
        secrets: &BridgeSecrets,
        path: &str,
        payload: &Value,
    ) -> ExecutorResult<Value> {
        validate_base_url(config.backend, &config.base_url)?;
        let url = format!(
            "{}/{}",
            config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms.max(1)))
            .build()
            .map_err(|error| ExecutorError::BridgeHttp {
                backend: config.backend.to_owned(),
                message: sanitize_backend_text(&error.to_string()),
            })?;
        let response = client
            .post(url)
            .bearer_auth(secrets.token())
            .json(payload)
            .send()
            .map_err(|error| ExecutorError::BridgeHttp {
                backend: config.backend.to_owned(),
                message: sanitize_backend_text(&error.to_string()),
            })?;
        let status = response.status();
        let body = response.text().map_err(|error| ExecutorError::BridgeHttp {
            backend: config.backend.to_owned(),
            message: sanitize_backend_text(&error.to_string()),
        })?;
        if !status.is_success() {
            return Err(ExecutorError::BridgeHttpStatus {
                backend: config.backend.to_owned(),
                status: status.as_u16(),
                body: sanitize_backend_text(&body),
            });
        }
        if body.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }
        serde_json::from_str(&body).map_err(ExecutorError::Json)
    }
}

pub fn validate_bridge_route(backend: &'static str, route_id: &str) -> ExecutorResult<()> {
    if route_id.trim().is_empty()
        || route_id != route_id.trim()
        || route_id.contains('/')
        || route_id.contains('\\')
        || route_id.chars().any(|character| {
            character.is_control() || character.is_whitespace() || matches!(character, '?' | '#')
        })
    {
        return Err(ExecutorError::InvalidBridgeRoute {
            backend: backend.to_owned(),
            route: route_id.to_owned(),
        });
    }
    Ok(())
}

pub fn validate_base_url(backend: &'static str, base_url: &str) -> ExecutorResult<()> {
    let base_url = base_url.trim();
    let unsupported = || ExecutorError::BridgeUnsupportedBaseUrl {
        backend: backend.to_owned(),
        base_url: base_url.to_owned(),
    };

    let url = reqwest::Url::parse(base_url).map_err(|_| unsupported())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ExecutorError::BridgeUnsupportedBaseUrl {
            backend: backend.to_owned(),
            base_url: base_url.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKEND: &str = "test_bridge";

    #[test]
    fn base_url_accepts_http_and_https_without_secret_carriers() {
        validate_base_url(BACKEND, "http://127.0.0.1:8787").expect("http base");
        validate_base_url(BACKEND, "https://bridge.example.test/api").expect("https base");
    }

    #[test]
    fn base_url_rejects_query_fragment_and_userinfo() {
        for base_url in [
            "https://bridge.example.test?token=secret",
            "https://bridge.example.test/#secret",
            "https://user:pass@bridge.example.test",
        ] {
            let error = validate_base_url(BACKEND, base_url).expect_err("invalid base URL");
            assert!(matches!(
                error,
                ExecutorError::BridgeUnsupportedBaseUrl { .. }
            ));
        }
    }

    #[test]
    fn base_url_rejects_non_http_schemes_and_missing_host() {
        for base_url in ["mqtt://bridge.example.test", "http://", "https://"] {
            let error = validate_base_url(BACKEND, base_url).expect_err("invalid base URL");
            assert!(matches!(
                error,
                ExecutorError::BridgeUnsupportedBaseUrl { .. }
            ));
        }
    }
}
