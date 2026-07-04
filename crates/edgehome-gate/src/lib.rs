//! Gate engine and policy engine for EdgeHome Harness.
//!
//! This crate is the safety boundary between a normalized model candidate and
//! anything that may become a dry-run, confirmation request, execution plan, or
//! memory write. The model never decides policy; it only produces candidates.

use edgehome_core::{
    Action, COMMAND_SCHEMA_VERSION, DeviceId, DeviceType, Intent, NormalizedCommand,
    PolicyDecision, RiskLevel, Room,
};
use edgehome_registry::{DeviceRecord, DeviceRegistry, RegistryError, StateFreshness};
use edgehome_storage::{EvidenceId, EvidenceKind, NewEvidence, SourceSystem};
use edgehome_trace::{GateOutcome, NewGateCheck, TraceError, TraceId, TraceStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

pub type GateResult<T> = Result<T, GateError>;

#[derive(Debug, Error)]
pub enum GateError {
    #[error("trace error: {0}")]
    Trace(#[from] TraceError),

    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("transition violation: {0}")]
    Transition(#[from] TransitionViolation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceFreshness {
    Fresh,
    Stale,
    Expired,
    Unknown,
    NoExpiry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateCheckSummary {
    pub gate_name: String,
    pub outcome: GateOutcome,
    pub reason: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateEvaluation {
    pub trace_id: TraceId,
    pub policy_decision: PolicyDecision,
    pub authoritative_risk: RiskLevel,
    pub device_id: Option<DeviceId>,
    pub executable: bool,
    pub requires_confirmation: bool,
    pub blocking_reasons: Vec<String>,
    pub gate_checks: Vec<GateCheckSummary>,
}

impl GateEvaluation {
    pub fn can_plan_dry_run(&self) -> bool {
        self.policy_decision != PolicyDecision::Deny
            && self.blocking_reasons.is_empty()
            && self.gate_checks.iter().any(|check| {
                check.gate_name == DryRunGate::NAME && check.outcome == GateOutcome::Accepted
            })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatedCommand {
    pub command: NormalizedCommand,
    pub evaluation: GateEvaluation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GateCommandDecision {
    Accepted { gated_command: GatedCommand },
    Rejected { evaluation: GateEvaluation },
}

#[derive(Debug, Clone)]
pub struct GateEvaluationRequest {
    pub trace_id: TraceId,
    pub command: NormalizedCommand,
    pub evidence_refs: Vec<EvidenceId>,
    pub input_flags: Vec<String>,
    pub state_freshness: StateFreshness,
    pub user_confirmed: bool,
    pub dry_run_ready: bool,
    pub execution_requested: bool,
    pub memory_write_requested: bool,
    pub memory_write_has_user_confirmation: bool,
    pub memory_write_source_has_evidence: bool,
}

impl GateEvaluationRequest {
    pub fn new(trace_id: TraceId, command: NormalizedCommand) -> Self {
        Self {
            trace_id,
            command,
            evidence_refs: Vec::new(),
            input_flags: Vec::new(),
            state_freshness: StateFreshness::Fresh,
            user_confirmed: false,
            dry_run_ready: false,
            execution_requested: false,
            memory_write_requested: false,
            memory_write_has_user_confirmation: false,
            memory_write_source_has_evidence: false,
        }
    }

    pub fn with_evidence_refs(mut self, evidence_refs: Vec<EvidenceId>) -> Self {
        self.evidence_refs = evidence_refs;
        self
    }

    pub fn with_input_flags(mut self, input_flags: Vec<String>) -> Self {
        self.input_flags = input_flags;
        self
    }

    pub fn with_state_freshness(mut self, state_freshness: StateFreshness) -> Self {
        self.state_freshness = state_freshness;
        self
    }

    pub fn with_user_confirmed(mut self, user_confirmed: bool) -> Self {
        self.user_confirmed = user_confirmed;
        self
    }

    pub fn with_dry_run_ready(mut self, dry_run_ready: bool) -> Self {
        self.dry_run_ready = dry_run_ready;
        self
    }

    pub fn with_execution_requested(mut self, execution_requested: bool) -> Self {
        self.execution_requested = execution_requested;
        self
    }

    pub fn with_memory_write_request(
        mut self,
        requested: bool,
        has_user_confirmation: bool,
        source_has_evidence: bool,
    ) -> Self {
        self.memory_write_requested = requested;
        self.memory_write_has_user_confirmation = has_user_confirmation;
        self.memory_write_source_has_evidence = source_has_evidence;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateDecision {
    pub gate_name: &'static str,
    pub outcome: GateOutcome,
    pub reason: String,
    pub blocking: bool,
}

impl GateDecision {
    fn accepted(gate_name: &'static str, reason: impl Into<String>) -> Self {
        Self {
            gate_name,
            outcome: GateOutcome::Accepted,
            reason: reason.into(),
            blocking: false,
        }
    }

    fn warning(gate_name: &'static str, reason: impl Into<String>, blocking: bool) -> Self {
        Self {
            gate_name,
            outcome: GateOutcome::Warning,
            reason: reason.into(),
            blocking,
        }
    }

    fn rejected(gate_name: &'static str, reason: impl Into<String>) -> Self {
        Self {
            gate_name,
            outcome: GateOutcome::Rejected,
            reason: reason.into(),
            blocking: true,
        }
    }
}

pub struct GateEngine<'a> {
    trace_store: &'a TraceStore,
    registry: &'a DeviceRegistry,
    policy_engine: PolicyEngine,
}

impl<'a> GateEngine<'a> {
    pub fn new(trace_store: &'a TraceStore, registry: &'a DeviceRegistry) -> Self {
        Self {
            trace_store,
            registry,
            policy_engine: PolicyEngine,
        }
    }

    pub fn evaluate(&self, request: GateEvaluationRequest) -> GateResult<GateEvaluation> {
        let mut gate_checks = Vec::new();
        let mut blocking_reasons = Vec::new();
        let mut gate_evidence_refs = request.evidence_refs.clone();

        let input_boundary = InputBoundaryGate::check(&request.input_flags);
        record_gate(
            self.trace_store,
            &request.trace_id,
            input_boundary.clone(),
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let schema = SchemaGate::check(&request.command);
        record_gate(
            self.trace_store,
            &request.trace_id,
            schema.clone(),
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let (device_decision, device) = DeviceResolvedGate::check(self.registry, &request.command);
        if let Some(device) = &device {
            let evidence = self.trace_store.record_evidence(NewEvidence::new(
                EvidenceKind::DeviceRegistrySnapshot,
                SourceSystem::Registry,
                "device resolved by registry",
                device_snapshot(device),
            ))?;
            gate_evidence_refs.push(evidence.id);
        }
        record_gate(
            self.trace_store,
            &request.trace_id,
            device_decision.clone(),
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let capability_decision = CapabilityGate::check(self.registry, &request.command);
        let capability_evidence = self.trace_store.record_evidence(NewEvidence::new(
            EvidenceKind::CapabilitySnapshot,
            SourceSystem::Registry,
            "capability gate snapshot",
            capability_snapshot(&request.command, &capability_decision),
        ))?;
        gate_evidence_refs.push(capability_evidence.id);
        record_gate(
            self.trace_store,
            &request.trace_id,
            capability_decision.clone(),
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let authoritative_risk = device
            .as_ref()
            .map(|device| device.risk_level.clone())
            .unwrap_or_else(|| request.command.risk.clone());

        let freshness_decision = FreshnessGate::check(&authoritative_risk, request.state_freshness);
        let freshness_evidence = self.trace_store.record_evidence(NewEvidence::new(
            EvidenceKind::DeviceStateSnapshot,
            SourceSystem::DeviceState,
            "device state freshness observed by gate",
            json!({
                "device_id": request.command.device_id.as_ref(),
                "state_freshness": request.state_freshness,
                "risk": authoritative_risk,
            }),
        ))?;
        gate_evidence_refs.push(freshness_evidence.id);
        record_gate(
            self.trace_store,
            &request.trace_id,
            freshness_decision.clone(),
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let policy = self.policy_engine.decide(&authoritative_risk);
        let mut policy_decision = policy.decision.clone();
        if input_boundary.blocking
            || schema.blocking
            || device_decision.blocking
            || capability_decision.blocking
        {
            policy_decision = PolicyDecision::Deny;
        }

        let policy_gate_decision = PolicyGate::check(&policy_decision, &authoritative_risk);
        let policy_evidence = self.trace_store.record_evidence(NewEvidence::new(
            EvidenceKind::PolicyRuleSnapshot,
            SourceSystem::Policy,
            "policy decision snapshot",
            json!({
                "policy_version": "policy.v1",
                "risk": authoritative_risk,
                "decision": policy_decision,
                "base_policy_decision": policy.decision,
                "input_flags": request.input_flags.clone(),
                "reason": policy.reason,
            }),
        ))?;
        gate_evidence_refs.push(policy_evidence.id);
        record_gate(
            self.trace_store,
            &request.trace_id,
            policy_gate_decision.clone(),
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let confirmation_decision = ConfirmationGate::check(
            &policy_decision,
            &authoritative_risk,
            request.user_confirmed,
            request.execution_requested,
        );
        record_gate(
            self.trace_store,
            &request.trace_id,
            confirmation_decision,
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let dry_run_decision = DryRunGate::check(&policy_decision, request.dry_run_ready);
        record_gate(
            self.trace_store,
            &request.trace_id,
            dry_run_decision,
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let execution_decision = ExecutionGate::check(
            &policy_decision,
            &authoritative_risk,
            request.dry_run_ready,
            request.user_confirmed,
            request.execution_requested,
        );
        record_gate(
            self.trace_store,
            &request.trace_id,
            execution_decision,
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let memory_decision = MemoryWriteGate::check(
            request.memory_write_requested,
            request.memory_write_has_user_confirmation,
            request.memory_write_source_has_evidence,
        );
        record_gate(
            self.trace_store,
            &request.trace_id,
            memory_decision,
            &gate_evidence_refs,
            &mut gate_checks,
            &mut blocking_reasons,
        )?;

        let executable = blocking_reasons.is_empty()
            && policy_decision == PolicyDecision::Allow
            && request.dry_run_ready
            && request.execution_requested;
        let requires_confirmation =
            policy_decision == PolicyDecision::RequireConfirmation && !request.user_confirmed;

        Ok(GateEvaluation {
            trace_id: request.trace_id,
            policy_decision,
            authoritative_risk,
            device_id: request.command.device_id,
            executable,
            requires_confirmation,
            blocking_reasons,
            gate_checks,
        })
    }

    pub fn verify(&self, request: GateEvaluationRequest) -> GateResult<GateCommandDecision> {
        let command = request.command.clone();
        let evaluation = self.evaluate(request)?;
        if evaluation.can_plan_dry_run() {
            Ok(GateCommandDecision::Accepted {
                gated_command: GatedCommand {
                    command,
                    evaluation,
                },
            })
        } else {
            Ok(GateCommandDecision::Rejected { evaluation })
        }
    }
}

fn record_gate(
    trace_store: &TraceStore,
    trace_id: &TraceId,
    decision: GateDecision,
    evidence_refs: &[EvidenceId],
    gate_checks: &mut Vec<GateCheckSummary>,
    blocking_reasons: &mut Vec<String>,
) -> GateResult<()> {
    trace_store.append_gate_check(
        trace_id,
        NewGateCheck::new(
            decision.gate_name,
            decision.outcome,
            decision.reason.clone(),
        )
        .with_evidence_refs(evidence_refs.to_vec()),
    )?;

    if decision.blocking {
        blocking_reasons.push(format!("{}: {}", decision.gate_name, decision.reason));
    }

    gate_checks.push(GateCheckSummary {
        gate_name: decision.gate_name.to_owned(),
        outcome: decision.outcome,
        reason: decision.reason,
        blocking: decision.blocking,
    });

    Ok(())
}

fn device_snapshot(device: &DeviceRecord) -> Value {
    json!({
        "device_id": device.device_id,
        "room": device.room,
        "device_type": device.device_type,
        "backend": device.backend,
        "risk_level": device.risk_level,
    })
}

fn capability_snapshot(command: &NormalizedCommand, decision: &GateDecision) -> Value {
    json!({
        "device_id": command.device_id.as_ref(),
        "device_type": command.device_type,
        "action": command.action,
        "params": command.params,
        "accepted": decision.outcome == GateOutcome::Accepted,
        "reason": decision.reason,
    })
}

pub struct InputBoundaryGate;

impl InputBoundaryGate {
    pub const NAME: &'static str = "InputBoundaryGate";

    pub fn check(input_flags: &[String]) -> GateDecision {
        if input_flags
            .iter()
            .any(|flag| flag == "prompt_injection_like")
        {
            return GateDecision::rejected(
                Self::NAME,
                "raw input contains prompt-injection-like instructions",
            );
        }

        if input_flags
            .iter()
            .any(|flag| flag == "dangerous_direct_backend_access")
        {
            return GateDecision::rejected(Self::NAME, "raw input attempts direct backend access");
        }

        GateDecision::accepted(Self::NAME, "raw input stays inside command boundary")
    }
}

pub struct SchemaGate;

impl SchemaGate {
    pub const NAME: &'static str = "SchemaGate";

    pub fn check(command: &NormalizedCommand) -> GateDecision {
        if command.schema_version.0 != COMMAND_SCHEMA_VERSION {
            return GateDecision::rejected(
                Self::NAME,
                format!("unsupported command schema `{}`", command.schema_version.0),
            );
        }
        if command.intent == Intent::Unknown {
            return GateDecision::rejected(Self::NAME, "intent is unknown");
        }
        if command.room == Room::Unknown {
            return GateDecision::rejected(Self::NAME, "room is unknown");
        }
        if command.device_type == DeviceType::Unknown {
            return GateDecision::rejected(Self::NAME, "device_type is unknown");
        }
        if command.action == Action::Unknown {
            return GateDecision::rejected(Self::NAME, "action is unknown");
        }
        if command.params.has_out_of_range_brightness() {
            return GateDecision::rejected(Self::NAME, "brightness is outside 0..100");
        }

        GateDecision::accepted(Self::NAME, "normalized command schema is acceptable")
    }
}

pub struct DeviceResolvedGate;

impl DeviceResolvedGate {
    pub const NAME: &'static str = "DeviceResolvedGate";

    pub fn check(
        registry: &DeviceRegistry,
        command: &NormalizedCommand,
    ) -> (GateDecision, Option<DeviceRecord>) {
        let Some(device_id) = command.device_id.as_ref() else {
            return (
                GateDecision::rejected(Self::NAME, "command has no resolved device_id"),
                None,
            );
        };

        match registry.get_device(device_id) {
            Ok(device) if device.device_type == command.device_type => (
                GateDecision::accepted(Self::NAME, "device_id resolved in registry"),
                Some(device.clone()),
            ),
            Ok(device) => (
                GateDecision::rejected(
                    Self::NAME,
                    format!(
                        "device `{}` has registry type `{:?}` but command expects `{:?}`",
                        device.device_id.0, device.device_type, command.device_type
                    ),
                ),
                Some(device.clone()),
            ),
            Err(error) => (
                GateDecision::rejected(Self::NAME, format!("device resolution failed: {error}")),
                None,
            ),
        }
    }
}

pub struct CapabilityGate;

impl CapabilityGate {
    pub const NAME: &'static str = "CapabilityGate";

    pub fn check(registry: &DeviceRegistry, command: &NormalizedCommand) -> GateDecision {
        match registry.validate_capability(command) {
            Ok(_) => GateDecision::accepted(Self::NAME, "device supports requested action"),
            Err(error) => {
                GateDecision::rejected(Self::NAME, format!("capability validation failed: {error}"))
            }
        }
    }
}

pub struct FreshnessGate;

impl FreshnessGate {
    pub const NAME: &'static str = "FreshnessGate";

    pub fn check(risk: &RiskLevel, freshness: StateFreshness) -> GateDecision {
        match freshness {
            StateFreshness::Fresh => GateDecision::accepted(Self::NAME, "device state is fresh"),
            StateFreshness::Stale if matches!(risk, RiskLevel::Read | RiskLevel::Low) => {
                GateDecision::warning(
                    Self::NAME,
                    "device state is stale but acceptable for low/read risk",
                    false,
                )
            }
            StateFreshness::Stale => GateDecision::rejected(
                Self::NAME,
                "device state is stale for medium/high risk command",
            ),
            StateFreshness::Expired => {
                GateDecision::rejected(Self::NAME, "device state is expired")
            }
            StateFreshness::Unknown if matches!(risk, RiskLevel::Read | RiskLevel::Low) => {
                GateDecision::warning(
                    Self::NAME,
                    "device state is unknown; execution must refresh before real action",
                    false,
                )
            }
            StateFreshness::Unknown => GateDecision::rejected(
                Self::NAME,
                "device state is unknown for medium/high risk command",
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    pub decision: PolicyDecision,
    pub reason: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyEngine;

impl PolicyEngine {
    pub fn decide(&self, risk: &RiskLevel) -> PolicyEvaluation {
        match risk {
            RiskLevel::Read => PolicyEvaluation {
                decision: PolicyDecision::Allow,
                reason: "read-only command is allowed".to_owned(),
            },
            RiskLevel::Low => PolicyEvaluation {
                decision: PolicyDecision::Allow,
                reason: "low-risk device action can proceed to dry-run".to_owned(),
            },
            RiskLevel::Medium => PolicyEvaluation {
                decision: PolicyDecision::RequireConfirmation,
                reason: "medium-risk action requires audit and confirmation before execution"
                    .to_owned(),
            },
            RiskLevel::High => PolicyEvaluation {
                decision: PolicyDecision::RequireConfirmation,
                reason: "high-risk action requires explicit confirmation".to_owned(),
            },
            RiskLevel::Blocked => PolicyEvaluation {
                decision: PolicyDecision::Deny,
                reason: "blocked-risk action is never executed automatically".to_owned(),
            },
            RiskLevel::Unknown => PolicyEvaluation {
                decision: PolicyDecision::Deny,
                reason: "unknown risk fails closed".to_owned(),
            },
        }
    }
}

pub struct PolicyGate;

impl PolicyGate {
    pub const NAME: &'static str = "PolicyGate";

    pub fn check(decision: &PolicyDecision, risk: &RiskLevel) -> GateDecision {
        match decision {
            PolicyDecision::Allow => {
                GateDecision::accepted(Self::NAME, format!("policy allows risk level `{:?}`", risk))
            }
            PolicyDecision::RequireConfirmation => GateDecision::warning(
                Self::NAME,
                format!("policy requires confirmation for risk level `{:?}`", risk),
                false,
            ),
            PolicyDecision::Deny => {
                GateDecision::rejected(Self::NAME, format!("policy denies risk level `{:?}`", risk))
            }
        }
    }
}

pub struct ConfirmationGate;

impl ConfirmationGate {
    pub const NAME: &'static str = "ConfirmationGate";

    pub fn check(
        decision: &PolicyDecision,
        risk: &RiskLevel,
        user_confirmed: bool,
        execution_requested: bool,
    ) -> GateDecision {
        match decision {
            PolicyDecision::Allow => {
                GateDecision::accepted(Self::NAME, "confirmation is not required")
            }
            PolicyDecision::RequireConfirmation if user_confirmed => GateDecision::accepted(
                Self::NAME,
                format!("user confirmed `{:?}` risk action", risk),
            ),
            PolicyDecision::RequireConfirmation if execution_requested => GateDecision::rejected(
                Self::NAME,
                "execution requested but required confirmation is missing",
            ),
            PolicyDecision::RequireConfirmation => GateDecision::warning(
                Self::NAME,
                "confirmation is required before execution",
                false,
            ),
            PolicyDecision::Deny => GateDecision::warning(
                Self::NAME,
                "confirmation cannot override denied policy",
                false,
            ),
        }
    }
}

pub struct DryRunGate;

impl DryRunGate {
    pub const NAME: &'static str = "DryRunGate";

    pub fn check(decision: &PolicyDecision, dry_run_ready: bool) -> GateDecision {
        if *decision == PolicyDecision::Deny {
            return GateDecision::warning(
                Self::NAME,
                "dry-run is skipped because policy denied the command",
                false,
            );
        }
        if dry_run_ready {
            return GateDecision::accepted(Self::NAME, "dry-run plan is ready");
        }

        GateDecision::warning(
            Self::NAME,
            "dry-run planner has not produced an ExecutionPlan yet",
            false,
        )
    }
}

pub struct ExecutionGate;

impl ExecutionGate {
    pub const NAME: &'static str = "ExecutionGate";

    pub fn check(
        decision: &PolicyDecision,
        risk: &RiskLevel,
        dry_run_ready: bool,
        user_confirmed: bool,
        execution_requested: bool,
    ) -> GateDecision {
        if !execution_requested {
            return GateDecision::accepted(Self::NAME, "execution is not requested");
        }
        if *decision == PolicyDecision::Deny {
            return GateDecision::rejected(Self::NAME, "denied policy cannot execute");
        }
        if !dry_run_ready {
            return GateDecision::rejected(Self::NAME, "execution cannot happen before dry-run");
        }
        if matches!(risk, RiskLevel::Medium | RiskLevel::High) && !user_confirmed {
            return GateDecision::rejected(
                Self::NAME,
                "medium/high-risk execution requires confirmation",
            );
        }

        GateDecision::accepted(Self::NAME, "execution transition is allowed")
    }
}

pub struct MemoryWriteGate;

impl MemoryWriteGate {
    pub const NAME: &'static str = "MemoryWriteGate";

    pub fn check(
        requested: bool,
        has_user_confirmation: bool,
        source_has_evidence: bool,
    ) -> GateDecision {
        if !requested {
            return GateDecision::accepted(Self::NAME, "no memory write requested");
        }
        if !source_has_evidence {
            return GateDecision::rejected(
                Self::NAME,
                "memory write cannot directly trust model output without evidence",
            );
        }
        if !has_user_confirmation {
            return GateDecision::rejected(
                Self::NAME,
                "long-term memory write requires explicit user confirmation",
            );
        }

        GateDecision::accepted(Self::NAME, "memory write has confirmation and evidence")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionAction {
    PolicyAllowed,
    Execute,
    VerifyState,
    MemoryWrite,
    StateBasedAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryWriteSource {
    UserConfirmed,
    EvidenceBacked,
    LlmOutput,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionContext {
    pub policy_decision: Option<PolicyDecision>,
    pub dry_run_ready: bool,
    pub user_confirmed: bool,
    pub risk: RiskLevel,
    pub executed: bool,
    pub memory_write_source: MemoryWriteSource,
    pub policy_snapshot_freshness: EvidenceFreshness,
    pub device_state_freshness: StateFreshness,
}

impl Default for TransitionContext {
    fn default() -> Self {
        Self {
            policy_decision: None,
            dry_run_ready: false,
            user_confirmed: false,
            risk: RiskLevel::Unknown,
            executed: false,
            memory_write_source: MemoryWriteSource::Unknown,
            policy_snapshot_freshness: EvidenceFreshness::Unknown,
            device_state_freshness: StateFreshness::Unknown,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionViolation {
    #[error("execute cannot happen before policy decision")]
    ExecuteBeforePolicy,

    #[error("execute cannot happen before dry-run")]
    ExecuteBeforeDryRun,

    #[error("high-risk execute cannot happen before confirmation")]
    HighRiskExecuteWithoutConfirmation,

    #[error("verify_state cannot happen before execute")]
    VerifyStateBeforeExecute,

    #[error("memory_write cannot directly trust LLM output")]
    MemoryWriteTrustedLlmOutput,

    #[error("policy_allowed cannot use expired policy snapshot")]
    ExpiredPolicySnapshot,

    #[error("state_based_action cannot use expired device state")]
    ExpiredDeviceState,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TransitionGate;

impl TransitionGate {
    pub fn check(
        &self,
        action: TransitionAction,
        context: &TransitionContext,
    ) -> Result<(), TransitionViolation> {
        match action {
            TransitionAction::Execute => {
                if context.policy_decision.is_none() {
                    return Err(TransitionViolation::ExecuteBeforePolicy);
                }
                if !context.dry_run_ready {
                    return Err(TransitionViolation::ExecuteBeforeDryRun);
                }
                if matches!(context.risk, RiskLevel::Medium | RiskLevel::High)
                    && !context.user_confirmed
                {
                    return Err(TransitionViolation::HighRiskExecuteWithoutConfirmation);
                }
                Ok(())
            }
            TransitionAction::VerifyState => {
                if !context.executed {
                    return Err(TransitionViolation::VerifyStateBeforeExecute);
                }
                Ok(())
            }
            TransitionAction::MemoryWrite => {
                if matches!(
                    context.memory_write_source,
                    MemoryWriteSource::LlmOutput | MemoryWriteSource::Unknown
                ) {
                    return Err(TransitionViolation::MemoryWriteTrustedLlmOutput);
                }
                Ok(())
            }
            TransitionAction::PolicyAllowed => {
                if context.policy_snapshot_freshness == EvidenceFreshness::Expired {
                    return Err(TransitionViolation::ExpiredPolicySnapshot);
                }
                Ok(())
            }
            TransitionAction::StateBasedAction => {
                if context.device_state_freshness == StateFreshness::Expired {
                    return Err(TransitionViolation::ExpiredDeviceState);
                }
                Ok(())
            }
        }
    }
}

pub const REQUIRED_GATE_NAMES: [&str; 10] = [
    InputBoundaryGate::NAME,
    SchemaGate::NAME,
    DeviceResolvedGate::NAME,
    CapabilityGate::NAME,
    FreshnessGate::NAME,
    PolicyGate::NAME,
    ConfirmationGate::NAME,
    DryRunGate::NAME,
    ExecutionGate::NAME,
    MemoryWriteGate::NAME,
];

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{CommandParams, CommandSchemaVersion};
    use edgehome_storage::{EvidenceKind, NewEvidence, SourceSystem};
    use edgehome_trace::StepStatus;
    use std::path::PathBuf;

    fn workspace_devices_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
            .join("devices.yaml")
    }

    fn load_registry() -> DeviceRegistry {
        DeviceRegistry::load_from_path(workspace_devices_path()).expect("registry")
    }

    fn start_trace(trace_store: &TraceStore, text: &str) -> (TraceId, Vec<EvidenceId>) {
        let raw = trace_store
            .record_evidence(NewEvidence::new(
                EvidenceKind::RawUserInput,
                SourceSystem::User,
                "raw user input",
                json!({ "text": text }),
            ))
            .expect("raw evidence");
        let trace = trace_store
            .start_trace(raw.id.clone(), "low_memory")
            .expect("trace");
        trace_store
            .append_step(
                &trace.trace_id,
                edgehome_trace::NewCommandStep::new("m7_gate_eval", StepStatus::Started)
                    .with_evidence_refs(vec![raw.id.clone()]),
            )
            .expect("step");
        (trace.trace_id, vec![raw.id])
    }

    fn command(
        device_id: Option<&str>,
        room: Room,
        device_type: DeviceType,
        action: Action,
        risk: RiskLevel,
    ) -> NormalizedCommand {
        NormalizedCommand {
            schema_version: CommandSchemaVersion::default(),
            intent: Intent::ControlDevice,
            room,
            device_id: device_id.map(|value| DeviceId::new(value).expect("device id")),
            device_type,
            action,
            params: CommandParams::default(),
            risk,
        }
    }

    fn evaluate(command: NormalizedCommand, text: &str) -> (GateEvaluation, TraceStore) {
        evaluate_with_input_flags(command, text, Vec::new())
    }

    fn evaluate_with_input_flags(
        command: NormalizedCommand,
        text: &str,
        input_flags: Vec<String>,
    ) -> (GateEvaluation, TraceStore) {
        let trace_store = TraceStore::in_memory().expect("trace store");
        let registry = load_registry();
        let (trace_id, evidence_refs) = start_trace(&trace_store, text);
        let engine = GateEngine::new(&trace_store, &registry);
        let evaluation = engine
            .evaluate(
                GateEvaluationRequest::new(trace_id, command)
                    .with_evidence_refs(evidence_refs)
                    .with_input_flags(input_flags)
                    .with_state_freshness(StateFreshness::Fresh),
            )
            .expect("evaluation");
        (evaluation, trace_store)
    }

    fn verify(
        command: NormalizedCommand,
        text: &str,
        dry_run_ready: bool,
    ) -> (GateCommandDecision, TraceStore) {
        let trace_store = TraceStore::in_memory().expect("trace store");
        let registry = load_registry();
        let (trace_id, evidence_refs) = start_trace(&trace_store, text);
        let engine = GateEngine::new(&trace_store, &registry);
        let decision = engine
            .verify(
                GateEvaluationRequest::new(trace_id, command)
                    .with_evidence_refs(evidence_refs)
                    .with_state_freshness(StateFreshness::Fresh)
                    .with_dry_run_ready(dry_run_ready),
            )
            .expect("gate decision");
        (decision, trace_store)
    }

    fn assert_all_gates_recorded(evaluation: &GateEvaluation, trace_store: &TraceStore) {
        let checks = trace_store
            .gate_checks_for_trace(&evaluation.trace_id)
            .expect("gate checks");
        let names: Vec<_> = checks
            .iter()
            .map(|check| check.gate_name.as_str())
            .collect();

        assert_eq!(names, REQUIRED_GATE_NAMES);
        assert_eq!(evaluation.gate_checks.len(), REQUIRED_GATE_NAMES.len());
    }

    #[test]
    fn lock_unlock_requires_confirmation_and_records_all_gates() {
        let command = command(
            Some("front_door_lock"),
            Room::Entrance,
            DeviceType::Lock,
            Action::Unlock,
            RiskLevel::Low,
        );

        let (evaluation, trace_store) = evaluate(command, "打开前门门锁");

        assert_eq!(
            evaluation.policy_decision,
            PolicyDecision::RequireConfirmation
        );
        assert_eq!(evaluation.authoritative_risk, RiskLevel::High);
        assert!(evaluation.requires_confirmation);
        assert!(!evaluation.executable);
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn camera_turn_off_requires_confirmation() {
        let command = command(
            Some("living_room_camera"),
            Room::LivingRoom,
            DeviceType::Camera,
            Action::TurnOff,
            RiskLevel::Low,
        );

        let (evaluation, trace_store) = evaluate(command, "关闭所有摄像头");

        assert_eq!(
            evaluation.policy_decision,
            PolicyDecision::RequireConfirmation
        );
        assert_eq!(evaluation.authoritative_risk, RiskLevel::High);
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn gas_alarm_turn_off_is_denied_by_authoritative_registry_risk() {
        let command = command(
            Some("gas_alarm"),
            Room::Kitchen,
            DeviceType::GasDevice,
            Action::TurnOff,
            RiskLevel::High,
        );

        let (evaluation, trace_store) = evaluate(command, "关闭燃气报警器");

        assert_eq!(evaluation.policy_decision, PolicyDecision::Deny);
        assert_eq!(evaluation.authoritative_risk, RiskLevel::Blocked);
        assert!(
            evaluation
                .gate_checks
                .iter()
                .any(|check| check.gate_name == PolicyGate::NAME
                    && check.outcome == GateOutcome::Rejected)
        );
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn direct_backend_access_flag_is_denied_before_policy_allows_low_risk_command() {
        let command = command(
            Some("living_room_main_light"),
            Room::LivingRoom,
            DeviceType::Light,
            Action::TurnOn,
            RiskLevel::Low,
        );

        let (evaluation, trace_store) = evaluate_with_input_flags(
            command,
            "发布 MQTT topic home/light/set 来打开客厅灯",
            vec!["dangerous_direct_backend_access".to_owned()],
        );

        assert_eq!(evaluation.policy_decision, PolicyDecision::Deny);
        assert!(!evaluation.can_plan_dry_run());
        assert!(
            evaluation
                .gate_checks
                .iter()
                .any(|check| check.gate_name == InputBoundaryGate::NAME
                    && check.outcome == GateOutcome::Rejected)
        );
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn unknown_device_is_denied() {
        let command = command(
            Some("missing_light"),
            Room::LivingRoom,
            DeviceType::Light,
            Action::TurnOff,
            RiskLevel::Low,
        );

        let (evaluation, trace_store) = evaluate(command, "关闭不存在的灯");

        assert_eq!(evaluation.policy_decision, PolicyDecision::Deny);
        assert!(
            evaluation
                .gate_checks
                .iter()
                .any(|check| check.gate_name == DeviceResolvedGate::NAME
                    && check.outcome == GateOutcome::Rejected)
        );
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn unsupported_capability_is_denied() {
        let command = command(
            Some("living_room_main_light"),
            Room::LivingRoom,
            DeviceType::Light,
            Action::SetTemperature,
            RiskLevel::Low,
        );

        let (evaluation, trace_store) = evaluate(command, "把客厅灯调到26度");

        assert_eq!(evaluation.policy_decision, PolicyDecision::Deny);
        assert!(
            evaluation
                .gate_checks
                .iter()
                .any(|check| check.gate_name == CapabilityGate::NAME
                    && check.outcome == GateOutcome::Rejected)
        );
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn verify_accepts_allowed_dry_run_ready_command() {
        let command = command(
            Some("living_room_main_light"),
            Room::LivingRoom,
            DeviceType::Light,
            Action::TurnOn,
            RiskLevel::Low,
        );

        let (decision, trace_store) = verify(command, "打开客厅灯", true);

        let GateCommandDecision::Accepted { gated_command } = decision else {
            panic!("allowed dry-run-ready command should be accepted");
        };
        assert!(gated_command.evaluation.can_plan_dry_run());
        assert_eq!(
            gated_command.command.device_id.as_ref().unwrap().0,
            "living_room_main_light"
        );
        assert_all_gates_recorded(&gated_command.evaluation, &trace_store);
    }

    #[test]
    fn verify_rejects_when_dry_run_is_not_ready() {
        let command = command(
            Some("living_room_main_light"),
            Room::LivingRoom,
            DeviceType::Light,
            Action::TurnOn,
            RiskLevel::Low,
        );

        let (decision, trace_store) = verify(command, "打开客厅灯", false);

        let GateCommandDecision::Rejected { evaluation } = decision else {
            panic!("command should not produce a GatedCommand before dry-run is ready");
        };
        assert!(!evaluation.can_plan_dry_run());
        assert_eq!(evaluation.policy_decision, PolicyDecision::Allow);
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn verify_rejects_denied_command() {
        let command = command(
            Some("gas_alarm"),
            Room::Kitchen,
            DeviceType::GasDevice,
            Action::TurnOff,
            RiskLevel::High,
        );

        let (decision, trace_store) = verify(command, "关闭燃气报警器", true);

        let GateCommandDecision::Rejected { evaluation } = decision else {
            panic!("denied command should not produce a GatedCommand");
        };
        assert_eq!(evaluation.policy_decision, PolicyDecision::Deny);
        assert!(!evaluation.can_plan_dry_run());
        assert_all_gates_recorded(&evaluation, &trace_store);
    }

    #[test]
    fn transition_gate_blocks_invalid_sequences() {
        let gate = TransitionGate;

        assert_eq!(
            gate.check(TransitionAction::Execute, &TransitionContext::default())
                .expect_err("execute before policy"),
            TransitionViolation::ExecuteBeforePolicy
        );

        let execute_without_dry_run = TransitionContext {
            policy_decision: Some(PolicyDecision::Allow),
            risk: RiskLevel::Low,
            ..TransitionContext::default()
        };
        assert_eq!(
            gate.check(TransitionAction::Execute, &execute_without_dry_run)
                .expect_err("execute before dry run"),
            TransitionViolation::ExecuteBeforeDryRun
        );

        let high_without_confirmation = TransitionContext {
            policy_decision: Some(PolicyDecision::RequireConfirmation),
            dry_run_ready: true,
            risk: RiskLevel::High,
            ..TransitionContext::default()
        };
        assert_eq!(
            gate.check(TransitionAction::Execute, &high_without_confirmation)
                .expect_err("high risk without confirmation"),
            TransitionViolation::HighRiskExecuteWithoutConfirmation
        );

        assert_eq!(
            gate.check(
                TransitionAction::VerifyState,
                &TransitionContext {
                    executed: false,
                    ..TransitionContext::default()
                }
            )
            .expect_err("verify before execute"),
            TransitionViolation::VerifyStateBeforeExecute
        );

        assert_eq!(
            gate.check(
                TransitionAction::MemoryWrite,
                &TransitionContext {
                    memory_write_source: MemoryWriteSource::LlmOutput,
                    ..TransitionContext::default()
                }
            )
            .expect_err("memory trusts LLM"),
            TransitionViolation::MemoryWriteTrustedLlmOutput
        );

        assert_eq!(
            gate.check(
                TransitionAction::PolicyAllowed,
                &TransitionContext {
                    policy_snapshot_freshness: EvidenceFreshness::Expired,
                    ..TransitionContext::default()
                }
            )
            .expect_err("expired policy"),
            TransitionViolation::ExpiredPolicySnapshot
        );

        assert_eq!(
            gate.check(
                TransitionAction::StateBasedAction,
                &TransitionContext {
                    device_state_freshness: StateFreshness::Expired,
                    ..TransitionContext::default()
                }
            )
            .expect_err("expired state"),
            TransitionViolation::ExpiredDeviceState
        );
    }
}
