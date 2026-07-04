//! Dry-run planner, mock executor, and execution transaction for EdgeHome Harness.
//!
//! The executor layer never accepts raw user text or raw model JSON. It only
//! accepts `ExecutionPlan`/`DryRunPlan` created after parser, registry, gate, and
//! policy checks have already happened.

mod home_assistant;

use std::{collections::HashMap, path::PathBuf};

use edgehome_core::{
    Action, DeviceId, DryRunPlan, ExecutionPlan, ExecutionResult, NormalizedCommand,
    PolicyDecision, RiskLevel,
};
use edgehome_gate::{
    MemoryWriteSource, TransitionAction, TransitionContext, TransitionGate, TransitionViolation,
};
use edgehome_registry::{BackendKind, DeviceRecord};
use edgehome_trace::TraceId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use time::{Duration, OffsetDateTime};

pub use home_assistant::{
    HomeAssistantClient, HomeAssistantConfig, HomeAssistantExecutor, HomeAssistantSecrets,
    HomeAssistantServiceCall, HomeAssistantState, SecretsLoader, home_assistant_service_call,
};

pub type ExecutorResult<T> = Result<T, ExecutorError>;

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("command has no resolved device_id")]
    MissingDeviceId,

    #[error("policy denied command")]
    PolicyDenied,

    #[error("dry-run plan target `{plan_target}` does not match command target `{command_target}`")]
    TargetMismatch {
        plan_target: String,
        command_target: String,
    },

    #[error("transition violation: {0}")]
    Transition(#[from] TransitionViolation),

    #[error("real execution is disabled by default")]
    ExecuteDisabled,

    #[error("execution plan is a duplicate of a recent plan")]
    DuplicatePlan,

    #[error("rate limit rejected execution plan")]
    RateLimited,

    #[error("post-state verification failed")]
    PostStateVerificationFailed,

    #[error("executor backend mismatch: expected `{expected}`, got `{actual}`")]
    ExecutorBackendMismatch { expected: String, actual: String },

    #[error("backend adapter is not implemented: {backend}")]
    BackendAdapterNotImplemented { backend: String },

    #[error("missing Home Assistant route for device `{0}`")]
    MissingHomeAssistantRoute(String),

    #[error("failed to read Home Assistant config `{path}`: {source}")]
    HomeAssistantConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse Home Assistant config `{path}`: {source}")]
    HomeAssistantConfigParse {
        path: PathBuf,
        source: serde_yaml::Error,
    },

    #[error("missing Home Assistant token; set `{env_var}` or configure token_file")]
    HomeAssistantSecretMissing { env_var: String },

    #[error("invalid Home Assistant entity_id: {0}")]
    InvalidHomeAssistantEntityId(String),

    #[error("unsupported Home Assistant base URL: {0}")]
    HomeAssistantUnsupportedBaseUrl(String),

    #[error("invalid Home Assistant HTTP response: {0}")]
    HomeAssistantInvalidHttpResponse(String),

    #[error("Home Assistant returned status {status}: {body}")]
    HomeAssistantHttpStatus { status: u16, body: String },

    #[error("unsupported Home Assistant action `{action}` for entity `{entity_id}`")]
    UnsupportedHomeAssistantAction { action: String, entity_id: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub trait Executor {
    fn dry_run(&self, plan: &DryRunPlan) -> ExecutorResult<DryRunPlan>;
    fn execute(&self, plan: &ExecutionPlan) -> ExecutorResult<ExecutionResult>;
}

pub trait BackendAdapter {
    fn kind(&self) -> BackendKind;

    fn dry_run_payload(
        &self,
        device: &DeviceRecord,
        command: &NormalizedCommand,
        plan: &ExecutionPlan,
    ) -> ExecutorResult<Value>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MockAdapter;

impl BackendAdapter for MockAdapter {
    fn kind(&self) -> BackendKind {
        BackendKind::Mock
    }

    fn dry_run_payload(
        &self,
        device: &DeviceRecord,
        command: &NormalizedCommand,
        _plan: &ExecutionPlan,
    ) -> ExecutorResult<Value> {
        Ok(mock_payload(device, command))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HomeAssistantAdapter;

impl BackendAdapter for HomeAssistantAdapter {
    fn kind(&self) -> BackendKind {
        BackendKind::HomeAssistant
    }

    fn dry_run_payload(
        &self,
        device: &DeviceRecord,
        command: &NormalizedCommand,
        plan: &ExecutionPlan,
    ) -> ExecutorResult<Value> {
        home_assistant_payload(device, command, plan)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DryRunPlanner;

impl DryRunPlanner {
    pub fn plan(
        &self,
        trace_id: &TraceId,
        command: &NormalizedCommand,
        device: &DeviceRecord,
        policy: PolicyDecision,
    ) -> ExecutorResult<DryRunPlan> {
        if policy == PolicyDecision::Deny {
            return Err(ExecutorError::PolicyDenied);
        }

        let command_device_id = command
            .device_id
            .as_ref()
            .ok_or(ExecutorError::MissingDeviceId)?;
        if command_device_id != &device.device_id {
            return Err(ExecutorError::TargetMismatch {
                plan_target: device.device_id.0.clone(),
                command_target: command_device_id.0.clone(),
            });
        }

        let plan = ExecutionPlan {
            trace_id: Some(trace_id.0.clone()),
            dry_run: true,
            target: device.device_id.clone(),
            action: command.action.clone(),
            params: command.params.clone(),
            policy,
        };

        Ok(DryRunPlan {
            backend: backend_name(&device.backend).to_owned(),
            payload: backend_payload(device, command, &plan)?,
            plan,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct MockExecutor {
    execute_enabled: bool,
}

impl MockExecutor {
    pub fn new(execute_enabled: bool) -> Self {
        Self { execute_enabled }
    }
}

impl Executor for MockExecutor {
    fn dry_run(&self, plan: &DryRunPlan) -> ExecutorResult<DryRunPlan> {
        Ok(plan.clone())
    }

    fn execute(&self, plan: &ExecutionPlan) -> ExecutorResult<ExecutionResult> {
        if !self.execute_enabled {
            return Err(ExecutorError::ExecuteDisabled);
        }
        if plan.policy == PolicyDecision::Deny {
            return Err(ExecutorError::PolicyDenied);
        }

        Ok(ExecutionResult {
            success: true,
            message: "mock execution accepted".to_owned(),
            raw_backend_response: Some(json!({
                "backend": "mock",
                "target": plan.target,
                "action": plan.action,
                "dry_run": plan.dry_run,
            })),
        })
    }
}

#[derive(Debug)]
pub struct ExecutionTransaction {
    transition_gate: TransitionGate,
    idempotency: IdempotencyChecker,
    rate_limiter: RateLimiter,
    post_state_verifier: MockPostStateVerifier,
}

impl Default for ExecutionTransaction {
    fn default() -> Self {
        Self {
            transition_gate: TransitionGate,
            idempotency: IdempotencyChecker::default(),
            rate_limiter: RateLimiter::default(),
            post_state_verifier: MockPostStateVerifier,
        }
    }
}

impl ExecutionTransaction {
    pub fn new(
        idempotency: IdempotencyChecker,
        rate_limiter: RateLimiter,
        post_state_verifier: MockPostStateVerifier,
    ) -> Self {
        Self {
            transition_gate: TransitionGate,
            idempotency,
            rate_limiter,
            post_state_verifier,
        }
    }

    pub fn dry_run(
        &self,
        trace_id: &TraceId,
        command: &NormalizedCommand,
        device: &DeviceRecord,
        policy: PolicyDecision,
    ) -> ExecutorResult<DryRunPlan> {
        DryRunPlanner.plan(trace_id, command, device, policy)
    }

    pub fn execute(
        &mut self,
        executor: &dyn Executor,
        dry_run: &DryRunPlan,
        risk: RiskLevel,
        user_confirmed: bool,
        now: OffsetDateTime,
    ) -> ExecutorResult<ExecutionResult> {
        let context = TransitionContext {
            policy_decision: Some(dry_run.plan.policy.clone()),
            dry_run_ready: true,
            user_confirmed,
            risk,
            executed: false,
            memory_write_source: MemoryWriteSource::EvidenceBacked,
            ..TransitionContext::default()
        };
        self.transition_gate
            .check(TransitionAction::Execute, &context)?;

        self.rate_limiter.check(&dry_run.plan, now)?;
        self.idempotency.remember_or_reject(&dry_run.plan)?;

        let result = executor.execute(&dry_run.plan)?;
        if !self.post_state_verifier.verify(&dry_run.plan, &result) {
            return Err(ExecutorError::PostStateVerificationFailed);
        }

        Ok(result)
    }
}

#[derive(Debug, Default, Clone)]
pub struct IdempotencyChecker {
    seen: HashMap<String, OffsetDateTime>,
}

impl IdempotencyChecker {
    pub fn remember_or_reject(&mut self, plan: &ExecutionPlan) -> ExecutorResult<()> {
        let fingerprint = plan_fingerprint(plan);
        if self.seen.contains_key(&fingerprint) {
            return Err(ExecutorError::DuplicatePlan);
        }
        self.seen.insert(fingerprint, OffsetDateTime::now_utc());
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    cooldown: Duration,
    last_seen: HashMap<String, OffsetDateTime>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self {
            cooldown: Duration::seconds(1),
            last_seen: HashMap::new(),
        }
    }
}

impl RateLimiter {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            cooldown,
            last_seen: HashMap::new(),
        }
    }

    pub fn check(&mut self, plan: &ExecutionPlan, now: OffsetDateTime) -> ExecutorResult<()> {
        let key = rate_limit_key(plan);
        if let Some(last_seen) = self.last_seen.get(&key)
            && now - *last_seen < self.cooldown
        {
            return Err(ExecutorError::RateLimited);
        }
        self.last_seen.insert(key, now);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MockPostStateVerifier;

impl MockPostStateVerifier {
    pub fn verify(&self, _plan: &ExecutionPlan, result: &ExecutionResult) -> bool {
        result.success
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTransactionStatus {
    pub dry_run_ready: bool,
    pub execute_enabled: bool,
    pub requires_confirmation: bool,
}

fn backend_name(backend: &BackendKind) -> &'static str {
    match backend {
        BackendKind::Mock => "mock",
        BackendKind::HomeAssistant => "home_assistant",
        BackendKind::MiioLocal => "miio_local",
        BackendKind::Mqtt => "mqtt",
    }
}

fn mock_payload(device: &DeviceRecord, command: &NormalizedCommand) -> Value {
    json!({
        "backend": "mock",
        "device_id": device.device_id,
        "backend_entity_id": device.backend_entity_id,
        "room": device.room,
        "device_type": device.device_type,
        "action": command.action,
        "params": command.params,
        "operation": operation_name(&command.action),
        "condition": {
            "time_after": command.params.time_after,
        }
    })
}

fn backend_payload(
    device: &DeviceRecord,
    command: &NormalizedCommand,
    plan: &ExecutionPlan,
) -> ExecutorResult<Value> {
    match &device.backend {
        BackendKind::Mock => MockAdapter.dry_run_payload(device, command, plan),
        BackendKind::HomeAssistant => HomeAssistantAdapter.dry_run_payload(device, command, plan),
        BackendKind::MiioLocal | BackendKind::Mqtt => {
            Err(ExecutorError::BackendAdapterNotImplemented {
                backend: backend_name(&device.backend).to_owned(),
            })
        }
    }
}

fn home_assistant_payload(
    device: &DeviceRecord,
    command: &NormalizedCommand,
    plan: &ExecutionPlan,
) -> ExecutorResult<Value> {
    let service_call = home_assistant_service_call(plan, &device.backend_entity_id)?;
    Ok(json!({
        "backend": "home_assistant",
        "device_id": device.device_id,
        "entity_id": device.backend_entity_id,
        "room": device.room,
        "device_type": device.device_type,
        "action": command.action,
        "service": service_call.service_name(),
        "service_path": service_call.service_path(),
        "payload": service_call.payload,
        "condition": {
            "time_after": command.params.time_after,
        }
    }))
}

fn operation_name(action: &Action) -> &'static str {
    match action {
        Action::TurnOn => "set_power_on",
        Action::TurnOff => "set_power_off",
        Action::SetBrightness => "set_brightness",
        Action::IncreaseBrightness => "increase_brightness",
        Action::DecreaseBrightness => "decrease_brightness",
        Action::SetTemperature => "set_temperature",
        Action::SetMode => "set_mode",
        Action::Open => "open",
        Action::Close => "close",
        Action::Lock => "lock",
        Action::Unlock => "unlock",
        Action::Unknown => "unknown",
    }
}

fn plan_fingerprint(plan: &ExecutionPlan) -> String {
    serde_json::to_string(&json!({
        "target": plan.target,
        "action": plan.action,
        "params": plan.params,
    }))
    .unwrap_or_else(|_| format!("{}:{:?}", plan.target.0, plan.action))
}

fn rate_limit_key(plan: &ExecutionPlan) -> String {
    format!("{}:{:?}", plan.target.0, plan.action)
}

pub fn plan_target_id(plan: &ExecutionPlan) -> &DeviceId {
    &plan.target
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{CommandParams, CommandSchemaVersion, DeviceType, Intent, Room};

    fn light_device() -> DeviceRecord {
        DeviceRecord {
            device_id: DeviceId::new("hallway_light").expect("device id"),
            aliases: vec!["走廊灯".to_owned()],
            room: Room::Hallway,
            device_type: DeviceType::Light,
            backend: BackendKind::Mock,
            backend_entity_id: "mock.light.hallway".to_owned(),
            risk_level: RiskLevel::Low,
        }
    }

    fn ha_light_device() -> DeviceRecord {
        DeviceRecord {
            backend: BackendKind::HomeAssistant,
            backend_entity_id: "light.hallway".to_owned(),
            ..light_device()
        }
    }

    fn mqtt_light_device() -> DeviceRecord {
        DeviceRecord {
            backend: BackendKind::Mqtt,
            backend_entity_id: "home/hallway/light/set".to_owned(),
            ..light_device()
        }
    }

    fn miio_light_device() -> DeviceRecord {
        DeviceRecord {
            backend: BackendKind::MiioLocal,
            backend_entity_id: "miio.local.hallway_light".to_owned(),
            ..light_device()
        }
    }

    fn command() -> NormalizedCommand {
        NormalizedCommand {
            schema_version: CommandSchemaVersion::default(),
            intent: Intent::ControlDevice,
            room: Room::Hallway,
            device_id: Some(DeviceId::new("hallway_light").expect("device id")),
            device_type: DeviceType::Light,
            action: Action::SetBrightness,
            params: CommandParams {
                brightness: Some(30),
                time_after: Some("22:00".to_owned()),
                ..CommandParams::default()
            },
            risk: RiskLevel::Low,
        }
    }

    fn dry_run_plan() -> DryRunPlan {
        DryRunPlanner
            .plan(
                &TraceId("tr_test".to_owned()),
                &command(),
                &light_device(),
                PolicyDecision::Allow,
            )
            .expect("dry-run plan")
    }

    #[test]
    fn dry_run_planner_generates_execution_plan_with_trace_id() {
        let plan = dry_run_plan();

        assert_eq!(plan.plan.trace_id, Some("tr_test".to_owned()));
        assert!(plan.plan.dry_run);
        assert_eq!(plan.plan.target.0, "hallway_light");
        assert_eq!(plan.plan.action, Action::SetBrightness);
        assert_eq!(plan.plan.params.brightness, Some(30));
        assert_eq!(plan.backend, "mock");
        assert_eq!(plan.payload["operation"], "set_brightness");
    }

    #[test]
    fn dry_run_planner_translates_home_assistant_payload() {
        let plan = DryRunPlanner
            .plan(
                &TraceId("tr_test".to_owned()),
                &command(),
                &ha_light_device(),
                PolicyDecision::Allow,
            )
            .expect("ha dry-run plan");

        assert_eq!(plan.backend, "home_assistant");
        assert_eq!(plan.payload["entity_id"], "light.hallway");
        assert_eq!(plan.payload["service"], "light.turn_on");
        assert_eq!(plan.payload["service_path"], "/api/services/light/turn_on");
        assert_eq!(plan.payload["payload"]["brightness_pct"], 30);
    }

    #[test]
    fn dry_run_planner_rejects_unimplemented_mqtt_backend() {
        let error = DryRunPlanner
            .plan(
                &TraceId("tr_test".to_owned()),
                &command(),
                &mqtt_light_device(),
                PolicyDecision::Allow,
            )
            .expect_err("mqtt adapter is not implemented");

        assert!(matches!(
            error,
            ExecutorError::BackendAdapterNotImplemented { backend } if backend == "mqtt"
        ));
    }

    #[test]
    fn dry_run_planner_rejects_unimplemented_miio_backend() {
        let error = DryRunPlanner
            .plan(
                &TraceId("tr_test".to_owned()),
                &command(),
                &miio_light_device(),
                PolicyDecision::Allow,
            )
            .expect_err("miio adapter is not implemented");

        assert!(matches!(
            error,
            ExecutorError::BackendAdapterNotImplemented { backend } if backend == "miio_local"
        ));
    }

    #[test]
    fn dry_run_planner_rejects_denied_policy() {
        let error = DryRunPlanner
            .plan(
                &TraceId("tr_test".to_owned()),
                &command(),
                &light_device(),
                PolicyDecision::Deny,
            )
            .expect_err("denied policy");

        assert!(matches!(error, ExecutorError::PolicyDenied));
    }

    #[test]
    fn mock_executor_execute_is_disabled_by_default() {
        let executor = MockExecutor::default();
        let plan = dry_run_plan();

        let error = executor
            .execute(&plan.plan)
            .expect_err("execute disabled by default");

        assert!(matches!(error, ExecutorError::ExecuteDisabled));
    }

    #[test]
    fn transaction_blocks_high_risk_execution_without_confirmation() {
        let executor = MockExecutor::new(true);
        let plan = dry_run_plan();
        let mut transaction = ExecutionTransaction::default();
        let error = transaction
            .execute(
                &executor,
                &plan,
                RiskLevel::High,
                false,
                OffsetDateTime::now_utc(),
            )
            .expect_err("missing confirmation");

        assert!(matches!(
            error,
            ExecutorError::Transition(TransitionViolation::HighRiskExecuteWithoutConfirmation)
        ));
    }

    #[test]
    fn idempotency_checker_rejects_duplicate_plan() {
        let plan = dry_run_plan();
        let mut checker = IdempotencyChecker::default();

        checker
            .remember_or_reject(&plan.plan)
            .expect("first plan accepted");
        let error = checker
            .remember_or_reject(&plan.plan)
            .expect_err("duplicate plan");

        assert!(matches!(error, ExecutorError::DuplicatePlan));
        assert_eq!(checker.len(), 1);
    }

    #[test]
    fn rate_limiter_rejects_fast_repeat() {
        let plan = dry_run_plan();
        let mut limiter = RateLimiter::new(Duration::seconds(5));
        let now = OffsetDateTime::now_utc();

        limiter.check(&plan.plan, now).expect("first accepted");
        let error = limiter
            .check(&plan.plan, now + Duration::seconds(1))
            .expect_err("fast repeat");

        assert!(matches!(error, ExecutorError::RateLimited));
    }

    #[test]
    fn post_state_verifier_requires_success_result() {
        let verifier = MockPostStateVerifier;
        let plan = dry_run_plan();

        assert!(!verifier.verify(
            &plan.plan,
            &ExecutionResult {
                success: false,
                message: "failed".to_owned(),
                raw_backend_response: None,
            }
        ));
    }
}
