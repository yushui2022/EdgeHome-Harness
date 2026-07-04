use std::{fmt, fs, path::Path, time::Duration};

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

    pub fn load_with_file(
        backend: &'static str,
        token_env: impl AsRef<str>,
        token_file: Option<&Path>,
    ) -> ExecutorResult<Option<Self>> {
        let token_env = token_env.as_ref();
        if let Some(secrets) = Self::load(backend, token_env)? {
            return Ok(Some(secrets));
        }

        if let Some(token_file) = token_file {
            let token = fs::read_to_string(token_file)?;
            if !token.trim().is_empty() {
                return Self::new(backend, token_env, token).map(Some);
            }
        }

        Ok(None)
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
    use std::{
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        thread,
    };

    use serde_json::json;

    use super::*;

    const BACKEND: &str = "test_bridge";

    fn temp_file_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}-{}",
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ))
    }

    #[test]
    fn base_url_accepts_http_and_https_without_secret_carriers() {
        validate_base_url(BACKEND, "http://127.0.0.1:8787").expect("http base");
        validate_base_url(BACKEND, "https://bridge.example.test/api").expect("https base");
    }

    #[test]
    fn bridge_secrets_can_load_private_token_file() {
        let token_file = temp_file_path("edgehome-bridge-token");
        fs::write(&token_file, "bridge-token-from-file").expect("write token file");

        let secrets = BridgeSecrets::load_with_file(
            BACKEND,
            "EDGEHOME_UNSET_BRIDGE_TEST_TOKEN",
            Some(token_file.as_path()),
        )
        .expect("load token file")
        .expect("secrets");

        let debug = format!("{secrets:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("bridge-token-from-file"));

        let _ = fs::remove_file(token_file);
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

    #[test]
    fn reqwest_poster_posts_json_to_local_bridge_with_bearer_token() {
        let (base_url, handle) = spawn_bridge_response(
            200,
            json!({
                "ok": true,
                "route_id": "miot.bedroom_ac",
                "state": "accepted"
            })
            .to_string(),
        );
        let config = BridgePostConfig {
            backend: BACKEND,
            base_url,
            request_timeout_ms: 2_000,
        };
        let secrets = BridgeSecrets::new(BACKEND, "EDGEHOME_TEST_BRIDGE_TOKEN", "bridge-token")
            .expect("secret");

        let response = ReqwestBridgePoster
            .post_json(
                &config,
                &secrets,
                "/v1/miot/execute",
                &json!({
                    "protocol": "miot",
                    "route_id": "miot.bedroom_ac",
                    "arguments": {
                        "temperature": 24
                    }
                }),
            )
            .expect("bridge response");
        let request = handle.join().expect("server thread");

        assert_eq!(response["ok"], true);
        assert!(request.starts_with("POST /v1/miot/execute HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer bridge-token")
        );
        assert!(request.contains("\"protocol\":\"miot\""));
        assert!(request.contains("\"temperature\":24"));
    }

    #[test]
    fn reqwest_poster_redacts_non_success_response_body() {
        let (base_url, handle) = spawn_bridge_response(
            500,
            json!({
                "ok": false,
                "token": "bridge-response-secret",
                "did": "private-xiaomi-did",
                "message": "controller failed"
            })
            .to_string(),
        );
        let config = BridgePostConfig {
            backend: BACKEND,
            base_url,
            request_timeout_ms: 2_000,
        };
        let secrets = BridgeSecrets::new(BACKEND, "EDGEHOME_TEST_BRIDGE_TOKEN", "bridge-token")
            .expect("secret");

        let error = ReqwestBridgePoster
            .post_json(&config, &secrets, "/v1/matter/execute", &json!({}))
            .expect_err("bridge returned non-success");
        handle.join().expect("server thread");

        match error {
            ExecutorError::BridgeHttpStatus { status, body, .. } => {
                assert_eq!(status, 500);
                assert!(body.contains("controller failed"));
                assert!(!body.contains("bridge-response-secret"));
                assert!(!body.contains("private-xiaomi-did"));
                assert!(body.contains("<redacted>"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn spawn_bridge_response(status: u16, body: String) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test bridge");
        let address = listener.local_addr().expect("local address");
        let base_url = format!("http://{address}");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request_bytes = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("read request");
                if read == 0 {
                    break;
                }
                request_bytes.extend_from_slice(&buffer[..read]);
                if request_complete(&request_bytes) {
                    break;
                }
            }

            let reason = if status == 200 {
                "OK"
            } else {
                "Internal Server Error"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            String::from_utf8(request_bytes).expect("request is utf-8")
        });
        (base_url, handle)
    }

    fn request_complete(request: &[u8]) -> bool {
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        request.len() >= header_end + 4 + content_length
    }
}
