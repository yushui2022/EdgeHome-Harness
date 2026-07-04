use std::{
    collections::HashMap,
    fmt, fs,
    path::{Path, PathBuf},
};

use edgehome_core::{
    Action, CommandParams, DeviceId, DryRunPlan, ExecutionPlan, ExecutionResult, NormalizedCommand,
    PolicyDecision,
};
use edgehome_registry::{BackendKind, DeviceRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    BackendAdapter, Executor, ExecutorError, ExecutorResult,
    bridge::{
        BridgePostConfig, BridgePoster, BridgeSecrets, ReqwestBridgePoster, validate_bridge_route,
    },
    evidence::sanitize_backend_evidence,
};

pub const MIOT_BACKEND_NAME: &str = "miio_local";
const MIOT_BRIDGE_BACKEND: &str = "miot_bridge";
const DEFAULT_MIOT_BRIDGE_TOKEN_ENV: &str = "EDGEHOME_MIOT_BRIDGE_TOKEN";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MiotBridgeConfig {
    pub base_url: String,
    pub token_env: String,
    pub token_file: Option<PathBuf>,
    pub request_timeout_ms: u64,
    pub execute_enabled: bool,
}

impl Default for MiotBridgeConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8787".to_owned(),
            token_env: DEFAULT_MIOT_BRIDGE_TOKEN_ENV.to_owned(),
            token_file: None,
            request_timeout_ms: 5_000,
            execute_enabled: false,
        }
    }
}

impl MiotBridgeConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> ExecutorResult<Self> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|source| ExecutorError::MiotBridgeConfigRead {
                path: path.to_path_buf(),
                source,
            })?;
        let config = serde_yaml::from_str(&content).map_err(|source| {
            ExecutorError::MiotBridgeConfigParse {
                path: path.to_path_buf(),
                source,
            }
        })?;
        Ok(config)
    }

    fn bridge_post_config(&self) -> BridgePostConfig {
        BridgePostConfig {
            backend: MIOT_BRIDGE_BACKEND,
            base_url: self.base_url.clone(),
            request_timeout_ms: self.request_timeout_ms,
        }
    }
}

pub type MiotBridgeSecrets = BridgeSecrets;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MiotBridgeRequest {
    pub protocol: String,
    pub route_id: String,
    pub device_id: DeviceId,
    pub action: Action,
    pub method: String,
    pub arguments: Value,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MiotAdapter;

impl BackendAdapter for MiotAdapter {
    fn kind(&self) -> BackendKind {
        BackendKind::MiioLocal
    }

    fn dry_run_payload(
        &self,
        device: &DeviceRecord,
        command: &NormalizedCommand,
        _plan: &ExecutionPlan,
    ) -> ExecutorResult<Value> {
        miot_payload(device, command)
    }
}

#[derive(Debug, Clone)]
pub struct MiotBridgeExecutor<P = ReqwestBridgePoster> {
    config: MiotBridgeConfig,
    secrets: Option<BridgeSecrets>,
    routes: HashMap<DeviceId, String>,
    poster: P,
}

impl MiotBridgeExecutor<ReqwestBridgePoster> {
    pub fn new(config: MiotBridgeConfig, secrets: Option<BridgeSecrets>) -> Self {
        Self::with_poster(config, secrets, ReqwestBridgePoster)
    }

    pub fn from_config(config: &MiotBridgeConfig) -> ExecutorResult<Self> {
        let secrets = BridgeSecrets::load_with_file(
            MIOT_BRIDGE_BACKEND,
            &config.token_env,
            config.token_file.as_deref(),
        )?;
        Ok(Self::new(config.clone(), secrets))
    }
}

impl<P> MiotBridgeExecutor<P> {
    pub fn with_poster(
        config: MiotBridgeConfig,
        secrets: Option<BridgeSecrets>,
        poster: P,
    ) -> Self {
        Self {
            config,
            secrets,
            routes: HashMap::new(),
            poster,
        }
    }

    pub fn from_device_records(
        config: MiotBridgeConfig,
        secrets: Option<BridgeSecrets>,
        devices: &[DeviceRecord],
        poster: P,
    ) -> ExecutorResult<Self> {
        let mut executor = Self::with_poster(config, secrets, poster);
        for device in devices
            .iter()
            .filter(|device| device.backend == BackendKind::MiioLocal)
        {
            executor = executor.with_route(device.device_id.clone(), &device.backend_entity_id)?;
        }
        Ok(executor)
    }

    pub fn with_route(
        mut self,
        device_id: DeviceId,
        route_id: impl Into<String>,
    ) -> ExecutorResult<Self> {
        let route_id = route_id.into();
        validate_bridge_route(MIOT_BRIDGE_BACKEND, &route_id)?;
        self.routes.insert(device_id, route_id);
        Ok(self)
    }

    pub fn translate_plan(&self, plan: &ExecutionPlan) -> ExecutorResult<MiotBridgeRequest> {
        let route_id = self
            .routes
            .get(&plan.target)
            .ok_or_else(|| ExecutorError::MissingBridgeRoute {
                backend: MIOT_BRIDGE_BACKEND.to_owned(),
                device_id: plan.target.0.clone(),
            })?
            .clone();
        miot_bridge_request(route_id, plan)
    }
}

impl<P> Executor for MiotBridgeExecutor<P>
where
    P: BridgePoster + fmt::Debug,
{
    fn dry_run(&self, plan: &DryRunPlan) -> ExecutorResult<DryRunPlan> {
        if plan.backend != MIOT_BACKEND_NAME {
            return Err(ExecutorError::ExecutorBackendMismatch {
                expected: MIOT_BACKEND_NAME.to_owned(),
                actual: plan.backend.clone(),
            });
        }
        Ok(plan.clone())
    }

    fn execute(&self, plan: &ExecutionPlan) -> ExecutorResult<ExecutionResult> {
        if !self.config.execute_enabled {
            return Err(ExecutorError::ExecuteDisabled);
        }
        if plan.policy == PolicyDecision::Deny {
            return Err(ExecutorError::PolicyDenied);
        }

        let request = self.translate_plan(plan)?;
        let secrets = self
            .secrets
            .as_ref()
            .ok_or_else(|| ExecutorError::BridgeSecretMissing {
                backend: MIOT_BRIDGE_BACKEND.to_owned(),
                env_var: self.config.token_env.clone(),
            })?;
        let payload = serde_json::to_value(&request)?;
        let response = self.poster.post_json(
            &self.config.bridge_post_config(),
            secrets,
            "/v1/miot/execute",
            &payload,
        )?;
        Ok(ExecutionResult {
            success: true,
            message: format!("MIoT bridge accepted route {}", request.route_id),
            raw_backend_response: Some(json!({
                "backend": MIOT_BACKEND_NAME,
                "protocol": "miot",
                "route_id": request.route_id,
                "request": request,
                "bridge_response": sanitize_backend_evidence(response),
                "bridge_response_redacted": true,
            })),
        })
    }
}

pub fn miot_payload(device: &DeviceRecord, command: &NormalizedCommand) -> ExecutorResult<Value> {
    let route_id = device.backend_entity_id.trim();
    validate_bridge_route(MIOT_BRIDGE_BACKEND, route_id)?;
    let plan = ExecutionPlan {
        trace_id: None,
        dry_run: true,
        target: device.device_id.clone(),
        action: command.action.clone(),
        params: command.params.clone(),
        policy: PolicyDecision::Allow,
    };
    let request = miot_bridge_request(route_id.to_owned(), &plan)?;

    Ok(json!({
        "backend": MIOT_BACKEND_NAME,
        "protocol": "miot",
        "device_id": device.device_id,
        "route_id": route_id,
        "room": device.room,
        "device_type": device.device_type,
        "action": command.action,
        "bridge_path": "/v1/miot/execute",
        "request": request,
        "condition": {
            "time_after": command.params.time_after,
        }
    }))
}

fn miot_bridge_request(
    route_id: String,
    plan: &ExecutionPlan,
) -> ExecutorResult<MiotBridgeRequest> {
    validate_bridge_route(MIOT_BRIDGE_BACKEND, &route_id)?;
    Ok(MiotBridgeRequest {
        protocol: "miot".to_owned(),
        route_id,
        device_id: plan.target.clone(),
        action: plan.action.clone(),
        method: miot_method(&plan.action).to_owned(),
        arguments: miot_arguments(&plan.action, &plan.params),
    })
}

fn miot_method(action: &Action) -> &'static str {
    match action {
        Action::SetBrightness | Action::SetTemperature | Action::SetMode => "set_properties",
        _ => "action",
    }
}

fn miot_arguments(action: &Action, params: &CommandParams) -> Value {
    match action {
        Action::TurnOn => json!({ "power": "on" }),
        Action::TurnOff => json!({ "power": "off" }),
        Action::SetBrightness => json!({ "brightness_pct": params.brightness }),
        Action::IncreaseBrightness => json!({ "brightness_delta": "increase" }),
        Action::DecreaseBrightness => json!({ "brightness_delta": "decrease" }),
        Action::SetTemperature => json!({ "temperature": params.temperature }),
        Action::SetMode => json!({ "mode": params.mode }),
        Action::Open => json!({ "state": "open" }),
        Action::Close => json!({ "state": "close" }),
        Action::Lock => json!({ "state": "locked" }),
        Action::Unlock => json!({ "state": "unlocked" }),
        Action::Unknown => json!({ "operation": "unknown" }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use edgehome_core::{CommandParams, DeviceId};

    fn plan() -> ExecutionPlan {
        ExecutionPlan {
            trace_id: Some("tr_test".to_owned()),
            dry_run: true,
            target: DeviceId::new("bedroom_air_conditioner").expect("device id"),
            action: Action::SetTemperature,
            params: CommandParams {
                temperature: Some(24),
                ..CommandParams::default()
            },
            policy: PolicyDecision::RequireConfirmation,
        }
    }

    #[test]
    fn config_defaults_to_execution_disabled() {
        let config = MiotBridgeConfig::default();

        assert!(!config.execute_enabled);
        assert_eq!(config.token_env, "EDGEHOME_MIOT_BRIDGE_TOKEN");
        assert!(config.token_file.is_none());
    }

    #[test]
    fn executor_execute_is_disabled_by_default() {
        let executor = MiotBridgeExecutor::new(MiotBridgeConfig::default(), None)
            .with_route(
                DeviceId::new("bedroom_air_conditioner").expect("device id"),
                "miot.bedroom_ac",
            )
            .expect("route");

        let error = executor.execute(&plan()).expect_err("disabled");

        assert!(matches!(error, ExecutorError::ExecuteDisabled));
    }

    #[test]
    fn executor_translates_plan_to_bridge_request() {
        let executor = MiotBridgeExecutor::new(MiotBridgeConfig::default(), None)
            .with_route(
                DeviceId::new("bedroom_air_conditioner").expect("device id"),
                "miot.bedroom_ac",
            )
            .expect("route");

        let request = executor.translate_plan(&plan()).expect("request");

        assert_eq!(
            request,
            MiotBridgeRequest {
                protocol: "miot".to_owned(),
                route_id: "miot.bedroom_ac".to_owned(),
                device_id: DeviceId::new("bedroom_air_conditioner").expect("device id"),
                action: Action::SetTemperature,
                method: "set_properties".to_owned(),
                arguments: json!({ "temperature": 24 }),
            }
        );
    }

    #[test]
    fn bridge_request_schema_matches_serialized_request_shape() {
        let request = MiotBridgeExecutor::new(MiotBridgeConfig::default(), None)
            .with_route(
                DeviceId::new("bedroom_air_conditioner").expect("device id"),
                "miot.bedroom_ac",
            )
            .expect("route")
            .translate_plan(&plan())
            .expect("request");
        let request_json = serde_json::to_value(&request).expect("request json");
        let schema = load_schema("miot-bridge-request.schema.json");
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema required");

        for key in request_json
            .as_object()
            .expect("request object")
            .keys()
            .map(String::as_str)
        {
            assert!(
                required
                    .iter()
                    .any(|required_key| required_key.as_str() == Some(key)),
                "schema required fields missing `{key}`"
            );
        }
        assert_eq!(
            schema.pointer("/properties/protocol/const"),
            Some(&json!("miot"))
        );
        assert!(
            schema
                .pointer("/properties/method/enum")
                .and_then(Value::as_array)
                .expect("method enum")
                .contains(&json!(request.method.clone()))
        );
        assert!(
            schema
                .pointer("/$defs/action/enum")
                .and_then(Value::as_array)
                .expect("action enum")
                .contains(&json!(request.action.clone()))
        );
    }

    #[derive(Debug, Default, Clone)]
    struct RecordingPoster {
        payloads: Arc<Mutex<Vec<Value>>>,
    }

    impl BridgePoster for RecordingPoster {
        fn post_json(
            &self,
            _config: &BridgePostConfig,
            _secrets: &BridgeSecrets,
            _path: &str,
            payload: &Value,
        ) -> ExecutorResult<Value> {
            self.payloads.lock().expect("lock").push(payload.clone());
            Ok(json!({
                "ok": true,
                "access_token": "bridge-response-secret",
                "node": {
                    "did": "private-xiaomi-did"
                }
            }))
        }
    }

    #[test]
    fn executor_posts_to_bridge_when_enabled() {
        let poster = RecordingPoster::default();
        let payloads = poster.payloads.clone();
        let config = MiotBridgeConfig {
            execute_enabled: true,
            ..MiotBridgeConfig::default()
        };
        let secrets = BridgeSecrets::new(MIOT_BRIDGE_BACKEND, &config.token_env, "bridge-token")
            .expect("secret");
        let executor = MiotBridgeExecutor::with_poster(config, Some(secrets), poster)
            .with_route(
                DeviceId::new("bedroom_air_conditioner").expect("device id"),
                "miot.bedroom_ac",
            )
            .expect("route");

        let result = executor.execute(&plan()).expect("executed");

        assert!(result.success);
        assert_eq!(payloads.lock().expect("lock").len(), 1);
        let serialized = serde_json::to_string(&result).expect("serialize");
        assert!(!serialized.contains("bridge-token"));
        assert!(!serialized.contains("bridge-response-secret"));
        assert!(!serialized.contains("private-xiaomi-did"));
        assert!(serialized.contains("<redacted>"));
    }

    fn load_schema(file_name: &str) -> Value {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
            .join("schemas")
            .join(file_name);
        let content = std::fs::read_to_string(&path).expect("read schema");
        serde_json::from_str(&content).expect("parse schema")
    }
}
