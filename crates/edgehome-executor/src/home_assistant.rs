use std::{
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use edgehome_core::{Action, DeviceId, DryRunPlan, ExecutionPlan, ExecutionResult, PolicyDecision};
use edgehome_registry::{BackendKind, DeviceRecord};
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Executor, ExecutorError, ExecutorResult,
    evidence::{sanitize_backend_evidence, sanitize_backend_text},
};

const DEFAULT_HA_TOKEN_ENV: &str = "EDGEHOME_HA_TOKEN";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeAssistantConfig {
    pub base_url: String,
    pub token_env: String,
    pub token_file: Option<PathBuf>,
    pub request_timeout_ms: u64,
    pub execute_enabled: bool,
    pub verify_state_after_execute: bool,
}

impl Default for HomeAssistantConfig {
    fn default() -> Self {
        Self {
            base_url: "http://homeassistant.local:8123".to_owned(),
            token_env: DEFAULT_HA_TOKEN_ENV.to_owned(),
            token_file: None,
            request_timeout_ms: 8_000,
            execute_enabled: false,
            verify_state_after_execute: true,
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
        if !env_var.is_empty()
            && let Ok(token) = std::env::var(env_var)
            && !token.trim().is_empty()
        {
            return HomeAssistantSecrets::new(token).map(Some);
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
        let response = request_json(
            &self.base_url,
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
        let response = request_json(
            &self.base_url,
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
    verify_state_after_execute: bool,
}

impl HomeAssistantExecutor {
    pub fn new(client: HomeAssistantClient, execute_enabled: bool) -> Self {
        Self {
            client,
            routes: HashMap::new(),
            execute_enabled,
            verify_state_after_execute: false,
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
        .with_post_state_verification(config.verify_state_after_execute)
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
            verify_state_after_execute: false,
        }
    }

    pub fn validate_routes(
        devices: &[DeviceRecord],
    ) -> ExecutorResult<HomeAssistantGatewayReadiness> {
        let mut route_count = 0;
        for device in devices
            .iter()
            .filter(|device| device.backend == BackendKind::HomeAssistant)
        {
            validate_entity_id(&device.backend_entity_id)?;
            route_count += 1;
        }
        Ok(HomeAssistantGatewayReadiness {
            route_count,
            routes_valid: true,
        })
    }

    pub fn with_post_state_verification(mut self, enabled: bool) -> Self {
        self.verify_state_after_execute = enabled;
        self
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
        let post_state = if self.verify_state_after_execute {
            Some(self.client.fetch_state(&service_call.entity_id)?)
        } else {
            None
        };
        let raw_backend_response =
            home_assistant_execution_evidence(&service_call, response, post_state.as_ref());
        Ok(ExecutionResult {
            success: true,
            message: format!(
                "home assistant service {} accepted",
                service_call.service_name()
            ),
            raw_backend_response: Some(raw_backend_response),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeAssistantGatewayReadiness {
    pub route_count: usize,
    pub routes_valid: bool,
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
struct HttpResponse {
    status: u16,
    body: String,
}

fn request_json(
    base_url: &str,
    method: &str,
    suffix: &str,
    body: Option<&Value>,
    bearer_token: &str,
    timeout: Duration,
) -> ExecutorResult<HttpResponse> {
    let url = home_assistant_url(base_url, suffix)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| {
            ExecutorError::HomeAssistantInvalidHttpResponse(sanitize_backend_text(
                &error.to_string(),
            ))
        })?;
    let request = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        other => {
            return Err(ExecutorError::HomeAssistantInvalidHttpResponse(format!(
                "unsupported HTTP method `{other}`"
            )));
        }
    }
    .header(ACCEPT, "application/json")
    .bearer_auth(bearer_token);
    let request = if let Some(body) = body {
        request.json(body)
    } else {
        request
    };
    let response = request.send().map_err(|error| {
        ExecutorError::HomeAssistantInvalidHttpResponse(sanitize_backend_text(&error.to_string()))
    })?;
    let status = response.status().as_u16();
    let body = response.text().map_err(|error| {
        ExecutorError::HomeAssistantInvalidHttpResponse(sanitize_backend_text(&error.to_string()))
    })?;

    Ok(HttpResponse { status, body })
}

fn home_assistant_url(base_url: &str, suffix: &str) -> ExecutorResult<String> {
    let base_url = base_url.trim();
    if !(base_url.starts_with("http://") || base_url.starts_with("https://"))
        || base_url.contains('?')
        || base_url.contains('#')
    {
        return Err(ExecutorError::HomeAssistantUnsupportedBaseUrl(
            base_url.to_owned(),
        ));
    }
    let url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    );
    reqwest::Url::parse(&url)
        .map_err(|_| ExecutorError::HomeAssistantUnsupportedBaseUrl(base_url.to_owned()))?;
    Ok(url)
}

fn ensure_success(status: u16, body: String) -> ExecutorResult<String> {
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        Err(ExecutorError::HomeAssistantHttpStatus {
            status,
            body: sanitize_backend_text(&body),
        })
    }
}

fn home_assistant_execution_evidence(
    service_call: &HomeAssistantServiceCall,
    response: Value,
    post_state: Option<&HomeAssistantState>,
) -> Value {
    json!({
        "backend": "home_assistant",
        "service": service_call.service_name(),
        "entity_id": service_call.entity_id,
        "response": sanitize_backend_evidence(response),
        "response_redacted": true,
        "post_state": post_state.map(home_assistant_state_summary),
    })
}

fn home_assistant_state_summary(state: &HomeAssistantState) -> Value {
    json!({
        "entity_id": state.entity_id,
        "state": state.state,
        "last_changed": state.last_changed,
        "last_updated": state.last_updated,
        "attributes_redacted": !state.attributes.is_null(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{CommandParams, DeviceId, DeviceType, RiskLevel, Room};

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
        assert!(config.verify_state_after_execute);
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
        assert_eq!(service_call.service_path(), "/api/services/light/turn_on");
        assert_eq!(
            service_call,
            HomeAssistantServiceCall {
                domain: "light".to_owned(),
                service: "turn_on".to_owned(),
                entity_id: "light.hallway".to_owned(),
                payload: json!({
                    "entity_id": "light.hallway",
                    "brightness_pct": 30
                }),
            }
        );
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
        assert_eq!(
            service_call,
            HomeAssistantServiceCall {
                domain: "climate".to_owned(),
                service: "set_temperature".to_owned(),
                entity_id: "climate.bedroom".to_owned(),
                payload: json!({
                    "entity_id": "climate.bedroom",
                    "temperature": 26
                }),
            }
        );
    }

    #[test]
    fn translates_climate_mode_to_service_call() {
        let mut execution_plan = plan(
            Action::SetMode,
            CommandParams {
                mode: Some("cool".to_owned()),
                ..CommandParams::default()
            },
        );
        execution_plan.target = DeviceId::new("bedroom_air_conditioner").expect("device id");

        let service_call =
            home_assistant_service_call(&execution_plan, "climate.bedroom").expect("service call");

        assert_eq!(
            service_call,
            HomeAssistantServiceCall {
                domain: "climate".to_owned(),
                service: "set_hvac_mode".to_owned(),
                entity_id: "climate.bedroom".to_owned(),
                payload: json!({
                    "entity_id": "climate.bedroom",
                    "hvac_mode": "cool"
                }),
            }
        );
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

    #[test]
    fn executor_missing_route_fails_closed() {
        let config = HomeAssistantConfig::default();
        let client = HomeAssistantClient::new(&config, None);
        let executor = HomeAssistantExecutor::new(client, false);

        let error = executor
            .translate_plan(&plan(Action::TurnOff, CommandParams::default()))
            .expect_err("missing route");

        assert!(matches!(
            error,
            ExecutorError::MissingHomeAssistantRoute(device_id) if device_id == "hallway_light"
        ));
    }

    #[test]
    fn gateway_route_validation_rejects_invalid_entity_id() {
        let devices = vec![DeviceRecord {
            device_id: DeviceId::new("hallway_light").expect("device id"),
            aliases: vec!["hallway light".to_owned()],
            room: Room::Hallway,
            device_type: DeviceType::Light,
            backend: BackendKind::HomeAssistant,
            backend_entity_id: "light.hallway/../../token".to_owned(),
            risk_level: RiskLevel::Low,
        }];

        let error = HomeAssistantExecutor::validate_routes(&devices)
            .expect_err("invalid entity id rejected");

        assert!(matches!(
            error,
            ExecutorError::InvalidHomeAssistantEntityId(_)
        ));
    }

    #[test]
    fn executor_rejects_non_home_assistant_dry_run_plan() {
        let config = HomeAssistantConfig::default();
        let client = HomeAssistantClient::new(&config, None);
        let executor = HomeAssistantExecutor::new(client, false);
        let dry_run = DryRunPlan {
            backend: "mock".to_owned(),
            payload: json!({ "backend": "mock" }),
            plan: plan(Action::TurnOff, CommandParams::default()),
        };

        let error = executor.dry_run(&dry_run).expect_err("backend mismatch");

        assert!(matches!(
            error,
            ExecutorError::ExecutorBackendMismatch { expected, actual }
                if expected == "home_assistant" && actual == "mock"
        ));
    }

    #[test]
    fn executor_dry_run_does_not_require_token_or_call_real_device() {
        let config = HomeAssistantConfig {
            token_env: "EDGEHOME_HA_TOKEN_SHOULD_NOT_EXIST_FOR_DRY_RUN_TEST".to_owned(),
            ..HomeAssistantConfig::default()
        };
        let client = HomeAssistantClient::new(&config, None);
        let executor = HomeAssistantExecutor::new(client, false);
        let dry_run = DryRunPlan {
            backend: "home_assistant".to_owned(),
            payload: json!({
                "backend": "home_assistant",
                "service": "light.turn_off",
                "payload": {
                    "entity_id": "light.hallway"
                }
            }),
            plan: plan(Action::TurnOff, CommandParams::default()),
        };

        let returned = executor.dry_run(&dry_run).expect("dry-run accepted");
        let serialized = serde_json::to_string(&returned).expect("serialize dry-run");

        assert_eq!(returned, dry_run);
        assert!(!serialized.contains("EDGEHOME_HA_TOKEN_SHOULD_NOT_EXIST_FOR_DRY_RUN_TEST"));
    }

    #[test]
    fn execution_evidence_redacts_backend_response_and_post_state_attributes() {
        let service_call = HomeAssistantServiceCall {
            domain: "light".to_owned(),
            service: "turn_on".to_owned(),
            entity_id: "light.hallway".to_owned(),
            payload: json!({ "entity_id": "light.hallway" }),
        };
        let post_state = HomeAssistantState {
            entity_id: "light.hallway".to_owned(),
            state: "on".to_owned(),
            attributes: json!({
                "access_token": "super-secret-token",
                "friendly_name": "Hallway"
            }),
            last_changed: Some("2026-07-04T12:00:00Z".to_owned()),
            last_updated: Some("2026-07-04T12:00:01Z".to_owned()),
        };

        let evidence = home_assistant_execution_evidence(
            &service_call,
            json!({
                "ok": true,
                "authorization": "Bearer super-secret-token",
                "context": {
                    "id": "safe-context-id"
                }
            }),
            Some(&post_state),
        );
        let serialized = serde_json::to_string(&evidence).expect("serialize evidence");

        assert!(!serialized.contains("super-secret-token"));
        assert!(!serialized.contains("friendly_name"));
        assert_eq!(
            evidence
                .pointer("/response/authorization")
                .and_then(Value::as_str),
            Some("<redacted>")
        );
        assert_eq!(
            evidence
                .pointer("/post_state/attributes_redacted")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            evidence
                .pointer("/post_state/state")
                .and_then(Value::as_str),
            Some("on")
        );
    }

    #[test]
    fn request_url_supports_https_gateway_base() {
        let url = home_assistant_url("https://ha.example.test:8123/api", "/states/light.hallway")
            .expect("url");

        assert_eq!(url, "https://ha.example.test:8123/api/states/light.hallway");
    }

    #[test]
    fn request_url_rejects_query_in_base_url() {
        let error = home_assistant_url("https://ha.example.test:8123?token=leak", "/api")
            .expect_err("query rejected");

        assert!(matches!(
            error,
            ExecutorError::HomeAssistantUnsupportedBaseUrl(_)
        ));
    }
}
