use std::{
    collections::HashMap,
    fmt, fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    time::Duration,
};

use edgehome_core::{Action, DeviceId, DryRunPlan, ExecutionPlan, ExecutionResult, PolicyDecision};
use edgehome_registry::{BackendKind, DeviceRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Executor, ExecutorError, ExecutorResult};

const DEFAULT_HA_TOKEN_ENV: &str = "EDGEHOME_HA_TOKEN";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeAssistantConfig {
    pub base_url: String,
    pub token_env: String,
    pub token_file: Option<PathBuf>,
    pub request_timeout_ms: u64,
    pub execute_enabled: bool,
}

impl Default for HomeAssistantConfig {
    fn default() -> Self {
        Self {
            base_url: "http://homeassistant.local:8123".to_owned(),
            token_env: DEFAULT_HA_TOKEN_ENV.to_owned(),
            token_file: None,
            request_timeout_ms: 8_000,
            execute_enabled: false,
        }
    }
}

impl HomeAssistantConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> ExecutorResult<Self> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|source| ExecutorError::HomeAssistantConfigRead {
                path: path.to_path_buf(),
                source,
            })?;
        let config = serde_yaml::from_str(&content).map_err(|source| {
            ExecutorError::HomeAssistantConfigParse {
                path: path.to_path_buf(),
                source,
            }
        })?;
        Ok(config)
    }
}

#[derive(Clone)]
pub struct HomeAssistantSecrets {
    token: String,
}

impl HomeAssistantSecrets {
    pub fn new(token: impl Into<String>) -> ExecutorResult<Self> {
        let token = token.into().trim().to_owned();
        if token.is_empty() {
            return Err(ExecutorError::HomeAssistantSecretMissing {
                env_var: DEFAULT_HA_TOKEN_ENV.to_owned(),
            });
        }
        Ok(Self { token })
    }

    fn token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for HomeAssistantSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HomeAssistantSecrets")
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SecretsLoader;

impl SecretsLoader {
    pub fn load(config: &HomeAssistantConfig) -> ExecutorResult<Option<HomeAssistantSecrets>> {
        let env_var = config.token_env.trim();
        if !env_var.is_empty() {
            if let Ok(token) = std::env::var(env_var) {
                if !token.trim().is_empty() {
                    return HomeAssistantSecrets::new(token).map(Some);
                }
            }
        }

        if let Some(token_file) = config.token_file.as_ref() {
            let token = fs::read_to_string(token_file)?;
            if !token.trim().is_empty() {
                return HomeAssistantSecrets::new(token).map(Some);
            }
        }

        Ok(None)
    }

    pub fn load_required(config: &HomeAssistantConfig) -> ExecutorResult<HomeAssistantSecrets> {
        Self::load(config)?.ok_or_else(|| ExecutorError::HomeAssistantSecretMissing {
            env_var: config.token_env.clone(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct HomeAssistantClient {
    base_url: String,
    timeout: Duration,
    secrets: Option<HomeAssistantSecrets>,
}

impl HomeAssistantClient {
    pub fn new(config: &HomeAssistantConfig, secrets: Option<HomeAssistantSecrets>) -> Self {
        Self {
            base_url: config.base_url.trim_end_matches('/').to_owned(),
            timeout: Duration::from_millis(config.request_timeout_ms),
            secrets,
        }
    }

    pub fn fetch_state(&self, entity_id: &str) -> ExecutorResult<HomeAssistantState> {
        validate_entity_id(entity_id)?;
        let token = self.required_token()?;
        let endpoint = HttpEndpoint::parse(&self.base_url)?;
        let response = request_json(
            &endpoint,
            "GET",
            &format!("/api/states/{entity_id}"),
            None,
            token,
            self.timeout,
        )?;
        ensure_success(response.status, response.body)
            .and_then(|body| serde_json::from_str(&body).map_err(ExecutorError::Json))
    }

    pub fn call_service(&self, service_call: &HomeAssistantServiceCall) -> ExecutorResult<Value> {
        let token = self.required_token()?;
        let endpoint = HttpEndpoint::parse(&self.base_url)?;
        let response = request_json(
            &endpoint,
            "POST",
            &service_call.service_path(),
            Some(&service_call.payload),
            token,
            self.timeout,
        )?;
        ensure_success(response.status, response.body)
            .and_then(|body| serde_json::from_str(&body).map_err(ExecutorError::Json))
    }

    fn required_token(&self) -> ExecutorResult<&str> {
        self.secrets
            .as_ref()
            .map(HomeAssistantSecrets::token)
            .ok_or_else(|| ExecutorError::HomeAssistantSecretMissing {
                env_var: DEFAULT_HA_TOKEN_ENV.to_owned(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeAssistantState {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: Value,
    pub last_changed: Option<String>,
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeAssistantServiceCall {
    pub domain: String,
    pub service: String,
    pub entity_id: String,
    pub payload: Value,
}

impl HomeAssistantServiceCall {
    pub fn service_name(&self) -> String {
        format!("{}.{}", self.domain, self.service)
    }

    pub fn service_path(&self) -> String {
        format!("/api/services/{}/{}", self.domain, self.service)
    }
}

#[derive(Debug, Clone)]
pub struct HomeAssistantExecutor {
    client: HomeAssistantClient,
    routes: HashMap<DeviceId, String>,
    execute_enabled: bool,
}

impl HomeAssistantExecutor {
    pub fn new(client: HomeAssistantClient, execute_enabled: bool) -> Self {
        Self {
            client,
            routes: HashMap::new(),
            execute_enabled,
        }
    }

    pub fn from_config(
        config: &HomeAssistantConfig,
        secrets: Option<HomeAssistantSecrets>,
    ) -> Self {
        Self::new(
            HomeAssistantClient::new(config, secrets),
            config.execute_enabled,
        )
    }

    pub fn from_device_records(
        client: HomeAssistantClient,
        execute_enabled: bool,
        devices: &[DeviceRecord],
    ) -> Self {
        let routes = devices
            .iter()
            .filter(|device| device.backend == BackendKind::HomeAssistant)
            .map(|device| (device.device_id.clone(), device.backend_entity_id.clone()))
            .collect();
        Self {
            client,
            routes,
            execute_enabled,
        }
    }

    pub fn with_route(
        mut self,
        device_id: DeviceId,
        entity_id: impl Into<String>,
    ) -> ExecutorResult<Self> {
        let entity_id = entity_id.into();
        validate_entity_id(&entity_id)?;
        self.routes.insert(device_id, entity_id);
        Ok(self)
    }

    pub fn translate_plan(&self, plan: &ExecutionPlan) -> ExecutorResult<HomeAssistantServiceCall> {
        let entity_id = self
            .routes
            .get(&plan.target)
            .ok_or_else(|| ExecutorError::MissingHomeAssistantRoute(plan.target.0.clone()))?;
        home_assistant_service_call(plan, entity_id)
    }

    pub fn fetch_state_for_device(
        &self,
        device_id: &DeviceId,
    ) -> ExecutorResult<HomeAssistantState> {
        let entity_id = self
            .routes
            .get(device_id)
            .ok_or_else(|| ExecutorError::MissingHomeAssistantRoute(device_id.0.clone()))?;
        self.client.fetch_state(entity_id)
    }
}

impl Executor for HomeAssistantExecutor {
    fn dry_run(&self, plan: &DryRunPlan) -> ExecutorResult<DryRunPlan> {
        if plan.backend != "home_assistant" {
            return Err(ExecutorError::ExecutorBackendMismatch {
                expected: "home_assistant".to_owned(),
                actual: plan.backend.clone(),
            });
        }
        Ok(plan.clone())
    }

    fn execute(&self, plan: &ExecutionPlan) -> ExecutorResult<ExecutionResult> {
        if !self.execute_enabled {
            return Err(ExecutorError::ExecuteDisabled);
        }
        if plan.policy == PolicyDecision::Deny {
            return Err(ExecutorError::PolicyDenied);
        }

        let service_call = self.translate_plan(plan)?;
        let response = self.client.call_service(&service_call)?;
        Ok(ExecutionResult {
            success: true,
            message: format!(
                "home assistant service {} accepted",
                service_call.service_name()
            ),
            raw_backend_response: Some(json!({
                "backend": "home_assistant",
                "service": service_call.service_name(),
                "entity_id": service_call.entity_id,
                "response": response,
            })),
        })
    }
}

pub fn home_assistant_service_call(
    plan: &ExecutionPlan,
    entity_id: &str,
) -> ExecutorResult<HomeAssistantServiceCall> {
    validate_entity_id(entity_id)?;
    let domain = entity_domain(entity_id)?;
    let mut payload = json!({ "entity_id": entity_id });

    let (service_domain, service) = match (domain, &plan.action) {
        ("light", Action::TurnOn) => {
            if let Some(brightness) = plan.params.brightness {
                payload["brightness_pct"] = json!(brightness);
            }
            ("light", "turn_on")
        }
        ("light", Action::TurnOff) => ("light", "turn_off"),
        ("light", Action::SetBrightness) => {
            if let Some(brightness) = plan.params.brightness {
                payload["brightness_pct"] = json!(brightness);
            }
            ("light", "turn_on")
        }
        ("light", Action::IncreaseBrightness) => {
            payload["brightness_step_pct"] = json!(10);
            ("light", "turn_on")
        }
        ("light", Action::DecreaseBrightness) => {
            payload["brightness_step_pct"] = json!(-10);
            ("light", "turn_on")
        }
        ("climate", Action::TurnOn) => ("climate", "turn_on"),
        ("climate", Action::TurnOff) => ("climate", "turn_off"),
        ("climate", Action::SetTemperature) => {
            if let Some(temperature) = plan.params.temperature {
                payload["temperature"] = json!(temperature);
            }
            ("climate", "set_temperature")
        }
        ("climate", Action::SetMode) => {
            if let Some(mode) = plan.params.mode.as_ref() {
                payload["hvac_mode"] = json!(mode);
            }
            ("climate", "set_hvac_mode")
        }
        ("lock", Action::Lock | Action::Close) => ("lock", "lock"),
        ("lock", Action::Unlock | Action::Open) => ("lock", "unlock"),
        ("cover", Action::Open) => ("cover", "open_cover"),
        ("cover", Action::Close) => ("cover", "close_cover"),
        (domain, Action::TurnOn) => (domain, "turn_on"),
        (domain, Action::TurnOff) => (domain, "turn_off"),
        _ => {
            return Err(ExecutorError::UnsupportedHomeAssistantAction {
                action: format!("{:?}", plan.action),
                entity_id: entity_id.to_owned(),
            });
        }
    };

    Ok(HomeAssistantServiceCall {
        domain: service_domain.to_owned(),
        service: service.to_owned(),
        entity_id: entity_id.to_owned(),
        payload,
    })
}

fn validate_entity_id(entity_id: &str) -> ExecutorResult<()> {
    let Some((domain, object_id)) = entity_id.split_once('.') else {
        return Err(ExecutorError::InvalidHomeAssistantEntityId(
            entity_id.to_owned(),
        ));
    };
    if domain.is_empty()
        || object_id.is_empty()
        || entity_id.contains('/')
        || entity_id.contains('\\')
        || entity_id
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(ExecutorError::InvalidHomeAssistantEntityId(
            entity_id.to_owned(),
        ));
    }
    Ok(())
}

fn entity_domain(entity_id: &str) -> ExecutorResult<&str> {
    validate_entity_id(entity_id)?;
    entity_id
        .split_once('.')
        .map(|(domain, _)| domain)
        .ok_or_else(|| ExecutorError::InvalidHomeAssistantEntityId(entity_id.to_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
    path_prefix: String,
}

impl HttpEndpoint {
    fn parse(base_url: &str) -> ExecutorResult<Self> {
        let Some(rest) = base_url.strip_prefix("http://") else {
            return Err(ExecutorError::HomeAssistantUnsupportedBaseUrl(
                base_url.to_owned(),
            ));
        };
        let (authority, path_prefix) = rest.split_once('/').unwrap_or((rest, ""));
        let (host, port) = if let Some((host, port)) = authority.split_once(':') {
            let parsed_port = port
                .parse::<u16>()
                .map_err(|_| ExecutorError::HomeAssistantUnsupportedBaseUrl(base_url.to_owned()))?;
            (host.to_owned(), parsed_port)
        } else {
            (authority.to_owned(), 80)
        };

        if host.trim().is_empty() {
            return Err(ExecutorError::HomeAssistantUnsupportedBaseUrl(
                base_url.to_owned(),
            ));
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

fn request_json(
    endpoint: &HttpEndpoint,
    method: &str,
    suffix: &str,
    body: Option<&Value>,
    bearer_token: &str,
    timeout: Duration,
) -> ExecutorResult<HttpResponse> {
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let body = body.map(serde_json::to_string).transpose()?;
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nAuthorization: Bearer {}\r\nConnection: close\r\n",
        method,
        endpoint.path(suffix),
        endpoint.host,
        bearer_token,
    );
    if let Some(body) = body.as_ref() {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    if let Some(body) = body.as_ref() {
        request.push_str(body);
    }

    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    parse_http_response(&response)
}

fn parse_http_response(raw: &[u8]) -> ExecutorResult<HttpResponse> {
    let response = String::from_utf8_lossy(raw);
    let Some((head, body)) = response.split_once("\r\n\r\n") else {
        return Err(ExecutorError::HomeAssistantInvalidHttpResponse(
            "missing header/body separator".to_owned(),
        ));
    };
    let mut lines = head.lines();
    let status_line = lines.next().ok_or_else(|| {
        ExecutorError::HomeAssistantInvalidHttpResponse("missing status line".to_owned())
    })?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| {
            ExecutorError::HomeAssistantInvalidHttpResponse("missing status code".to_owned())
        })?
        .parse::<u16>()
        .map_err(|_| {
            ExecutorError::HomeAssistantInvalidHttpResponse("invalid status code".to_owned())
        })?;

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

fn decode_chunked_body(body: &str) -> ExecutorResult<String> {
    let mut remaining = body;
    let mut output = String::new();
    loop {
        let Some((size_hex, rest)) = remaining.split_once("\r\n") else {
            return Err(ExecutorError::HomeAssistantInvalidHttpResponse(
                "invalid chunk header".to_owned(),
            ));
        };
        let size = usize::from_str_radix(size_hex.trim(), 16).map_err(|_| {
            ExecutorError::HomeAssistantInvalidHttpResponse("invalid chunk size".to_owned())
        })?;
        if size == 0 {
            return Ok(output);
        }
        if rest.len() < size + 2 {
            return Err(ExecutorError::HomeAssistantInvalidHttpResponse(
                "chunk shorter than declared size".to_owned(),
            ));
        }
        output.push_str(&rest[..size]);
        remaining = &rest[size + 2..];
    }
}

fn ensure_success(status: u16, body: String) -> ExecutorResult<String> {
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        Err(ExecutorError::HomeAssistantHttpStatus { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{CommandParams, DeviceId};

    fn workspace_config(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join(path)
    }

    fn plan(entity_action: Action, params: CommandParams) -> ExecutionPlan {
        ExecutionPlan {
            trace_id: Some("tr_test".to_owned()),
            dry_run: true,
            target: DeviceId::new("hallway_light").expect("device id"),
            action: entity_action,
            params,
            policy: PolicyDecision::Allow,
        }
    }

    #[test]
    fn example_config_loads_with_execution_disabled() {
        let config =
            HomeAssistantConfig::load_from_path(workspace_config("home_assistant.yaml.example"))
                .expect("load example config");

        assert_eq!(config.token_env, "EDGEHOME_HA_TOKEN");
        assert!(!config.execute_enabled);
        assert!(config.base_url.starts_with("http://"));
    }

    #[test]
    fn missing_token_does_not_panic() {
        let config = HomeAssistantConfig {
            token_env: "EDGEHOME_HA_TOKEN_SHOULD_NOT_EXIST_FOR_TEST".to_owned(),
            token_file: None,
            ..HomeAssistantConfig::default()
        };

        let secrets = SecretsLoader::load(&config).expect("load optional secrets");

        assert!(secrets.is_none());
    }

    #[test]
    fn secret_debug_is_redacted() {
        let secrets = HomeAssistantSecrets::new("super-secret-token").expect("secret");
        let debug = format!("{secrets:?}");

        assert!(debug.contains("redacted"));
        assert!(!debug.contains("super-secret-token"));
    }

    #[test]
    fn translates_light_brightness_to_service_call() {
        let service_call = home_assistant_service_call(
            &plan(
                Action::SetBrightness,
                CommandParams {
                    brightness: Some(30),
                    ..CommandParams::default()
                },
            ),
            "light.hallway",
        )
        .expect("service call");

        assert_eq!(service_call.service_name(), "light.turn_on");
        assert_eq!(service_call.payload["entity_id"], "light.hallway");
        assert_eq!(service_call.payload["brightness_pct"], 30);
    }

    #[test]
    fn translates_climate_temperature_to_service_call() {
        let mut execution_plan = plan(
            Action::SetTemperature,
            CommandParams {
                temperature: Some(26),
                ..CommandParams::default()
            },
        );
        execution_plan.target = DeviceId::new("bedroom_air_conditioner").expect("device id");

        let service_call =
            home_assistant_service_call(&execution_plan, "climate.bedroom").expect("service call");

        assert_eq!(service_call.service_name(), "climate.set_temperature");
        assert_eq!(service_call.payload["temperature"], 26);
    }

    #[test]
    fn rejects_entity_id_with_path_injection() {
        let error = home_assistant_service_call(
            &plan(Action::TurnOff, CommandParams::default()),
            "light.hallway/../../secrets",
        )
        .expect_err("invalid entity id");

        assert!(matches!(
            error,
            ExecutorError::InvalidHomeAssistantEntityId(_)
        ));
    }

    #[test]
    fn executor_execute_is_disabled_by_default() {
        let config = HomeAssistantConfig::default();
        let client = HomeAssistantClient::new(&config, None);
        let executor = HomeAssistantExecutor::new(client, false)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "light.hallway",
            )
            .expect("route");

        let error = executor
            .execute(&plan(Action::TurnOff, CommandParams::default()))
            .expect_err("disabled");

        assert!(matches!(error, ExecutorError::ExecuteDisabled));
    }

    #[test]
    fn executor_uses_route_to_translate_plan() {
        let config = HomeAssistantConfig::default();
        let client = HomeAssistantClient::new(&config, None);
        let executor = HomeAssistantExecutor::new(client, false)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "light.hallway",
            )
            .expect("route");

        let service_call = executor
            .translate_plan(&plan(Action::TurnOff, CommandParams::default()))
            .expect("service call");

        assert_eq!(service_call.service_name(), "light.turn_off");
    }
}
