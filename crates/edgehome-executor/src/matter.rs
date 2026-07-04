use std::{collections::HashMap, fmt, fs, path::Path};

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

pub const MATTER_BACKEND_NAME: &str = "matter_bridge";
const DEFAULT_MATTER_BRIDGE_TOKEN_ENV: &str = "EDGEHOME_MATTER_BRIDGE_TOKEN";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatterBridgeConfig {
    pub base_url: String,
    pub token_env: String,
    pub request_timeout_ms: u64,
    pub execute_enabled: bool,
}

impl Default for MatterBridgeConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:9797".to_owned(),
            token_env: DEFAULT_MATTER_BRIDGE_TOKEN_ENV.to_owned(),
            request_timeout_ms: 5_000,
            execute_enabled: false,
        }
    }
}

impl MatterBridgeConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> ExecutorResult<Self> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|source| ExecutorError::MatterBridgeConfigRead {
                path: path.to_path_buf(),
                source,
            })?;
        let config = serde_yaml::from_str(&content).map_err(|source| {
            ExecutorError::MatterBridgeConfigParse {
                path: path.to_path_buf(),
                source,
            }
        })?;
        Ok(config)
    }

    fn bridge_post_config(&self) -> BridgePostConfig {
        BridgePostConfig {
            backend: MATTER_BACKEND_NAME,
            base_url: self.base_url.clone(),
            request_timeout_ms: self.request_timeout_ms,
        }
    }
}

pub type MatterBridgeSecrets = BridgeSecrets;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatterBridgeRequest {
    pub protocol: String,
    pub route_id: String,
    pub device_id: DeviceId,
    pub action: Action,
    pub command: String,
    pub arguments: Value,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MatterBridgeAdapter;

impl BackendAdapter for MatterBridgeAdapter {
    fn kind(&self) -> BackendKind {
        BackendKind::MatterBridge
    }

    fn dry_run_payload(
        &self,
        device: &DeviceRecord,
        command: &NormalizedCommand,
        _plan: &ExecutionPlan,
    ) -> ExecutorResult<Value> {
        matter_payload(device, command)
    }
}

#[derive(Debug, Clone)]
pub struct MatterBridgeExecutor<P = ReqwestBridgePoster> {
    config: MatterBridgeConfig,
    secrets: Option<BridgeSecrets>,
    routes: HashMap<DeviceId, String>,
    poster: P,
}

impl MatterBridgeExecutor<ReqwestBridgePoster> {
    pub fn new(config: MatterBridgeConfig, secrets: Option<BridgeSecrets>) -> Self {
        Self::with_poster(config, secrets, ReqwestBridgePoster)
    }

    pub fn from_config(config: &MatterBridgeConfig) -> ExecutorResult<Self> {
        let secrets = BridgeSecrets::load(MATTER_BACKEND_NAME, &config.token_env)?;
        Ok(Self::new(config.clone(), secrets))
    }
}

impl<P> MatterBridgeExecutor<P> {
    pub fn with_poster(
        config: MatterBridgeConfig,
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
        config: MatterBridgeConfig,
        secrets: Option<BridgeSecrets>,
        devices: &[DeviceRecord],
        poster: P,
    ) -> ExecutorResult<Self> {
        let mut executor = Self::with_poster(config, secrets, poster);
        for device in devices
            .iter()
            .filter(|device| device.backend == BackendKind::MatterBridge)
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
        validate_bridge_route(MATTER_BACKEND_NAME, &route_id)?;
        self.routes.insert(device_id, route_id);
        Ok(self)
    }

    pub fn translate_plan(&self, plan: &ExecutionPlan) -> ExecutorResult<MatterBridgeRequest> {
        let route_id = self
            .routes
            .get(&plan.target)
            .ok_or_else(|| ExecutorError::MissingBridgeRoute {
                backend: MATTER_BACKEND_NAME.to_owned(),
                device_id: plan.target.0.clone(),
            })?
            .clone();
        matter_bridge_request(route_id, plan)
    }
}

impl<P> Executor for MatterBridgeExecutor<P>
where
    P: BridgePoster + fmt::Debug,
{
    fn dry_run(&self, plan: &DryRunPlan) -> ExecutorResult<DryRunPlan> {
        if plan.backend != MATTER_BACKEND_NAME {
            return Err(ExecutorError::ExecutorBackendMismatch {
                expected: MATTER_BACKEND_NAME.to_owned(),
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
                backend: MATTER_BACKEND_NAME.to_owned(),
                env_var: self.config.token_env.clone(),
            })?;
        let payload = serde_json::to_value(&request)?;
        let response = self.poster.post_json(
            &self.config.bridge_post_config(),
            secrets,
            "/v1/matter/execute",
            &payload,
        )?;
        Ok(ExecutionResult {
            success: true,
            message: format!("Matter bridge accepted route {}", request.route_id),
            raw_backend_response: Some(json!({
                "backend": MATTER_BACKEND_NAME,
                "protocol": "matter",
                "route_id": request.route_id,
                "request": request,
                "bridge_response": sanitize_backend_evidence(response),
                "bridge_response_redacted": true,
            })),
        })
    }
}

pub fn matter_payload(device: &DeviceRecord, command: &NormalizedCommand) -> ExecutorResult<Value> {
    let route_id = device.backend_entity_id.trim();
    validate_bridge_route(MATTER_BACKEND_NAME, route_id)?;
    let plan = ExecutionPlan {
        trace_id: None,
        dry_run: true,
        target: device.device_id.clone(),
        action: command.action.clone(),
        params: command.params.clone(),
        policy: PolicyDecision::Allow,
    };
    let request = matter_bridge_request(route_id.to_owned(), &plan)?;

    Ok(json!({
        "backend": MATTER_BACKEND_NAME,
        "protocol": "matter",
        "device_id": device.device_id,
        "route_id": route_id,
        "room": device.room,
        "device_type": device.device_type,
        "action": command.action,
        "bridge_path": "/v1/matter/execute",
        "request": request,
        "condition": {
            "time_after": command.params.time_after,
        }
    }))
}

fn matter_bridge_request(
    route_id: String,
    plan: &ExecutionPlan,
) -> ExecutorResult<MatterBridgeRequest> {
    validate_bridge_route(MATTER_BACKEND_NAME, &route_id)?;
    Ok(MatterBridgeRequest {
        protocol: "matter".to_owned(),
        route_id,
        device_id: plan.target.clone(),
        action: plan.action.clone(),
        command: matter_command(&plan.action).to_owned(),
        arguments: matter_arguments(&plan.action, &plan.params),
    })
}

fn matter_command(action: &Action) -> &'static str {
    match action {
        Action::TurnOn => "on_off.on",
        Action::TurnOff => "on_off.off",
        Action::SetBrightness => "level_control.move_to_level",
        Action::IncreaseBrightness => "level_control.step_up",
        Action::DecreaseBrightness => "level_control.step_down",
        Action::SetTemperature => "thermostat.setpoint",
        Action::SetMode => "thermostat.system_mode",
        Action::Open => "window_covering.up_or_open",
        Action::Close => "window_covering.down_or_close",
        Action::Lock => "door_lock.lock",
        Action::Unlock => "door_lock.unlock",
        Action::Unknown => "unknown",
    }
}

fn matter_arguments(action: &Action, params: &CommandParams) -> Value {
    match action {
        Action::TurnOn | Action::TurnOff => json!({}),
        Action::SetBrightness => json!({ "level_pct": params.brightness }),
        Action::IncreaseBrightness => json!({ "step_pct": 10 }),
        Action::DecreaseBrightness => json!({ "step_pct": -10 }),
        Action::SetTemperature => json!({ "temperature": params.temperature }),
        Action::SetMode => json!({ "mode": params.mode }),
        Action::Open | Action::Close | Action::Lock | Action::Unlock => json!({}),
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
            target: DeviceId::new("hallway_light").expect("device id"),
            action: Action::SetBrightness,
            params: CommandParams {
                brightness: Some(30),
                ..CommandParams::default()
            },
            policy: PolicyDecision::Allow,
        }
    }

    #[test]
    fn config_defaults_to_execution_disabled() {
        let config = MatterBridgeConfig::default();

        assert!(!config.execute_enabled);
        assert_eq!(config.token_env, "EDGEHOME_MATTER_BRIDGE_TOKEN");
    }

    #[test]
    fn executor_execute_is_disabled_by_default() {
        let executor = MatterBridgeExecutor::new(MatterBridgeConfig::default(), None)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "matter.hallway_light",
            )
            .expect("route");

        let error = executor.execute(&plan()).expect_err("disabled");

        assert!(matches!(error, ExecutorError::ExecuteDisabled));
    }

    #[test]
    fn executor_translates_plan_to_bridge_request() {
        let executor = MatterBridgeExecutor::new(MatterBridgeConfig::default(), None)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "matter.hallway_light",
            )
            .expect("route");

        let request = executor.translate_plan(&plan()).expect("request");

        assert_eq!(
            request,
            MatterBridgeRequest {
                protocol: "matter".to_owned(),
                route_id: "matter.hallway_light".to_owned(),
                device_id: DeviceId::new("hallway_light").expect("device id"),
                action: Action::SetBrightness,
                command: "level_control.move_to_level".to_owned(),
                arguments: json!({ "level_pct": 30 }),
            }
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
                "fabric_id": "private-fabric",
                "controller": {
                    "node_id": "private-node"
                }
            }))
        }
    }

    #[test]
    fn executor_posts_to_bridge_when_enabled() {
        let poster = RecordingPoster::default();
        let payloads = poster.payloads.clone();
        let config = MatterBridgeConfig {
            execute_enabled: true,
            ..MatterBridgeConfig::default()
        };
        let secrets = BridgeSecrets::new(MATTER_BACKEND_NAME, &config.token_env, "bridge-token")
            .expect("secret");
        let executor = MatterBridgeExecutor::with_poster(config, Some(secrets), poster)
            .with_route(
                DeviceId::new("hallway_light").expect("device id"),
                "matter.hallway_light",
            )
            .expect("route");

        let result = executor.execute(&plan()).expect("executed");

        assert!(result.success);
        assert_eq!(payloads.lock().expect("lock").len(), 1);
        let serialized = serde_json::to_string(&result).expect("serialize");
        assert!(!serialized.contains("bridge-token"));
        assert!(!serialized.contains("private-fabric"));
        assert!(!serialized.contains("private-node"));
        assert!(serialized.contains("<redacted>"));
    }
}
