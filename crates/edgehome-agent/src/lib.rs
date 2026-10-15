//! An embeddable, single-action agent for a trusted local host.
//!
//! Model proposals are untrusted. The host owns budgets, device state,
//! confirmation, registry configuration, and the executor. One runtime holds at
//! most one pending plan; exported previews are never accepted as execution input.
//! See `examples/bounded_agent.rs` and `docs/agent-reference.md` in the repository.

use std::time::{Duration, Instant};

use edgehome_core::{Action, DryRunPlan, Intent, PolicyDecision, RiskLevel, UserInput};
use edgehome_executor::{DryRunPlanner, ExecutionTransaction, Executor, ExecutorError};
use edgehome_gate::{GateCommandDecision, GateEngine, GateEvaluationRequest};
use edgehome_ollama::{OutputGovernor, OutputGovernorConfig};
use edgehome_parser::{InputGuard, SemanticNormalizer};
use edgehome_registry::{DeviceRegistry, DeviceResolutionInput, StateFreshness};
use edgehome_storage::{EvidenceKind, NewEvidence, SourceSystem};
use edgehome_trace::{NewCommandStep, StepStatus, TraceError, TraceId, TraceStore};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;
use time::OffsetDateTime;

/// Transport limits for a single model call. Providers must enforce these while
/// receiving data, including connection and total deadlines. A synchronous
/// callback cannot be forcibly cancelled by this runtime.
#[derive(Debug, Clone, Copy)]
pub struct ModelBudget {
    pub max_output_bytes: usize,
    pub timeout: Duration,
}

/// Implement with Ollama, another local runtime, or a deterministic fixture.
/// Only user text and resource limits cross this interface, never backend routes.
pub trait CandidateProvider {
    fn propose(&mut self, input: &UserInput, budget: ModelBudget) -> Result<String, ProviderError>;
}

/// Deliberately excludes transport error strings that may contain credentials.
#[derive(Debug, Error)]
#[error("candidate provider unavailable")]
pub struct ProviderError;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_proposals: u32,
    pub max_execution_attempts: u32,
    pub max_input_chars: usize,
    pub model_budget: ModelBudget,
    pub plan_ttl: Duration,
    pub execute_enabled: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_proposals: 16,
            max_execution_attempts: 4,
            max_input_chars: 500,
            model_budget: ModelBudget {
                max_output_bytes: 4096,
                timeout: Duration::from_secs(8),
            },
            plan_ttl: Duration::from_secs(30),
            execute_enabled: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent budgets and plan TTL must be nonzero")]
    InvalidConfig,
    #[error("proposal budget exhausted; host must explicitly start another session")]
    ProposalBudgetExhausted,
    #[error("execution attempt budget exhausted")]
    ExecutionBudgetExhausted,
    #[error("execution is disabled for this runtime")]
    ExecuteDisabled,
    #[error("no pending plan with this trace ID in this runtime")]
    UnknownPlan,
    #[error("pending plan has expired; obtain a new proposal")]
    PlanExpired,
    #[error("host confirmation is required for this exact pending plan")]
    ConfirmationRequired,
    #[error("host must refresh device state before dispatch")]
    StateNotFresh,
    #[error("trace storage failed: {0}")]
    Trace(#[from] TraceError),
    #[error("gate failed: {0}")]
    Gate(#[from] edgehome_gate::GateError),
    #[error("execution preflight rejected: {0}")]
    ExecutionPreflight(ExecutorError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    InvalidInput,
    InputBoundary,
    ProviderUnavailable,
    ModelDeadlineExceeded,
    InvalidOutput,
    UnsupportedIntent,
    UnsupportedCondition,
    InvalidParameters,
    UnresolvedTarget,
    PolicyOrCapability,
    UnsupportedAdapter,
}

/// A display-only snapshot. Mutating or deserializing a copy cannot change the
/// private plan retained by the runtime.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Proposal {
    Ready {
        trace_id: TraceId,
        plan: Box<DryRunPlan>,
    },
    Rejected {
        trace_id: TraceId,
        reason: RejectionReason,
    },
}

impl Proposal {
    pub fn trace_id(&self) -> &TraceId {
        match self {
            Self::Ready { trace_id, .. } | Self::Rejected { trace_id, .. } => trace_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Backend acknowledged the call; physical convergence is not established.
    Acknowledged,
    /// The backend may have acted. Never automatically resend the action.
    OutcomeUnknown,
}

#[derive(Debug, Serialize)]
pub struct ExecutionReceipt {
    pub trace_id: TraceId,
    pub status: ExecutionStatus,
    /// False means dispatch occurred but completion recording failed. The plan
    /// is still consumed and must not be retried to repair the audit trail.
    pub completion_recorded: bool,
}

struct PendingPlan {
    trace_id: TraceId,
    plan: DryRunPlan,
    risk: RiskLevel,
    created_at: Instant,
}

/// Keep this object alive for a session. `&mut self` serializes proposal and
/// dispatch calls; limits and execution transaction state survive between turns.
/// Limits are per instance, not a global or cross-process exactly-once guarantee.
pub struct AgentRuntime<P, E> {
    provider: P,
    executor: E,
    registry: DeviceRegistry,
    traces: TraceStore,
    config: AgentConfig,
    transaction: ExecutionTransaction,
    proposals: u32,
    execution_attempts: u32,
    pending: Option<PendingPlan>,
}

impl<P: CandidateProvider, E: Executor> AgentRuntime<P, E> {
    pub fn new(
        provider: P,
        executor: E,
        registry: DeviceRegistry,
        traces: TraceStore,
        config: AgentConfig,
    ) -> Result<Self, AgentError> {
        if config.max_proposals == 0
            || config.max_execution_attempts == 0
            || config.max_input_chars == 0
            || config.model_budget.max_output_bytes == 0
            || config.model_budget.timeout.is_zero()
            || config.plan_ttl.is_zero()
        {
            return Err(AgentError::InvalidConfig);
        }
        Ok(Self {
            provider,
            executor,
            registry,
            traces,
            config,
            transaction: ExecutionTransaction::default(),
            proposals: 0,
            execution_attempts: 0,
            pending: None,
        })
    }

    /// Every new turn revokes the previous pending action, including rejected
    /// turns. The runtime deliberately has no implicit history or rule repair.
    /// `freshness` is an observation supplied by trusted host code, never a model.
    pub fn propose(
        &mut self,
        text: &str,
        freshness: StateFreshness,
    ) -> Result<Proposal, AgentError> {
        self.pending = None;
        if self.proposals >= self.config.max_proposals {
            return Err(AgentError::ProposalBudgetExhausted);
        }
        self.proposals += 1;
        // Bound retained input even when the guard rejects an oversized request.
        let input_ref = self.traces.record_evidence(NewEvidence::new(
            EvidenceKind::RawUserInput,
            SourceSystem::User,
            "bounded agent input",
            json!({"text": text.chars().take(self.config.max_input_chars).collect::<String>()}),
        ))?;
        let trace = self.traces.start_trace(input_ref.id, "bounded_agent")?;
        let trace_id = trace.trace_id;
        if text
            .chars()
            .take(self.config.max_input_chars.saturating_add(1))
            .count()
            > self.config.max_input_chars
        {
            return self.reject(trace_id, RejectionReason::InvalidInput);
        }
        let guarded = match InputGuard::new(self.config.max_input_chars).guard(text) {
            Ok(input) => input,
            Err(_) => return self.reject(trace_id, RejectionReason::InvalidInput),
        };
        if !guarded.flags.is_empty() {
            return self.reject(trace_id, RejectionReason::InputBoundary);
        }
        let started = Instant::now();
        let output = self
            .provider
            .propose(&guarded.input, self.config.model_budget);
        if started.elapsed() >= self.config.model_budget.timeout {
            return self.reject(trace_id, RejectionReason::ModelDeadlineExceeded);
        }
        let output = match output {
            Ok(output) if output.len() <= self.config.model_budget.max_output_bytes => output,
            Ok(_) => return self.reject(trace_id, RejectionReason::InvalidOutput),
            Err(_) => return self.reject(trace_id, RejectionReason::ProviderUnavailable),
        };
        let governor = OutputGovernor::new(OutputGovernorConfig {
            max_output_bytes: self.config.model_budget.max_output_bytes,
            max_output_chars: self.config.model_budget.max_output_bytes,
            retry_count: 0,
        });
        let candidate = match governor.govern(&output) {
            Ok(candidate) => candidate,
            Err(_) => return self.reject(trace_id, RejectionReason::InvalidOutput),
        };
        let candidate_ref = self.traces.record_evidence(NewEvidence::new(
            EvidenceKind::ParsedJson,
            SourceSystem::Model,
            "bounded agent candidate without rule repair",
            json!({
                "candidate": candidate,
                "output_bytes": output.len(),
                "model_call_ms": started.elapsed().as_millis(),
            }),
        ))?;
        self.traces.append_step(
            &trace_id,
            NewCommandStep::new("agent_candidate", StepStatus::Succeeded)
                .with_evidence_refs(vec![candidate_ref.id]),
        )?;
        if candidate.intent != Intent::ControlDevice {
            return self.reject(trace_id, RejectionReason::UnsupportedIntent);
        }
        if candidate.params.time_after.is_some() {
            return self.reject(trace_id, RejectionReason::UnsupportedCondition);
        }
        // No inherited targets: each proposal must identify its device.
        if candidate.params.raw_value.as_deref() == Some("relative_command")
            || candidate
                .device_alias
                .as_deref()
                .is_some_and(|alias| alias.starts_with("relative:"))
        {
            return self.reject(trace_id, RejectionReason::UnresolvedTarget);
        }
        let mut command = match SemanticNormalizer.normalize(&candidate) {
            Ok(command) => command,
            Err(_) => return self.reject(trace_id, RejectionReason::InvalidOutput),
        };
        let params = &command.params;
        if (command.action == Action::SetBrightness && params.brightness.is_none())
            || (command.action == Action::SetTemperature && params.temperature.is_none())
            || (command.action == Action::SetMode
                && params
                    .mode
                    .as_deref()
                    .is_none_or(|mode| mode.trim().is_empty()))
            || (params.brightness.is_some()
                && !matches!(command.action, Action::SetBrightness | Action::TurnOn))
            || (params.temperature.is_some() && command.action != Action::SetTemperature)
            || (params.mode.is_some() && command.action != Action::SetMode)
            || params.raw_value.is_some()
        {
            return self.reject(trace_id, RejectionReason::InvalidParameters);
        }
        let resolution = match self
            .registry
            .device_resolver()
            .resolve(DeviceResolutionInput {
                candidate_alias: candidate.device_alias.as_deref(),
                room: &command.room,
                device_type: &command.device_type,
            }) {
            Ok(resolution) => resolution,
            Err(_) => return self.reject(trace_id, RejectionReason::UnresolvedTarget),
        };
        let device = resolution.device;
        command.device_id = Some(device.device_id.clone());
        command.room = device.room.clone();
        command.device_type = device.device_type.clone();
        command.risk = device.risk_level.clone();
        let command_ref = self.traces.record_evidence(NewEvidence::new(
            EvidenceKind::NormalizedCommand,
            SourceSystem::Normalizer,
            "bounded agent resolved command",
            json!(command),
        ))?;
        let decision = GateEngine::new(&self.traces, &self.registry).verify(
            GateEvaluationRequest::new(trace_id.clone(), command)
                .with_evidence_refs(vec![command_ref.id])
                .with_state_freshness(freshness)
                .with_dry_run_ready(true),
        )?;
        let gated = match decision {
            GateCommandDecision::Accepted { gated_command } => gated_command,
            GateCommandDecision::Rejected { .. } => {
                return self.reject(trace_id, RejectionReason::PolicyOrCapability);
            }
        };
        let plan = match DryRunPlanner.plan_gated(&gated, device) {
            Ok(plan) => plan,
            Err(_) => return self.reject(trace_id, RejectionReason::UnsupportedAdapter),
        };
        let plan_ref = self.traces.record_evidence(NewEvidence::new(
            EvidenceKind::DryRunPlan,
            SourceSystem::Executor,
            "bounded agent plan preview",
            json!(plan),
        ))?;
        self.traces.append_step(
            &trace_id,
            NewCommandStep::new("agent_plan_ready", StepStatus::Succeeded)
                .with_evidence_refs(vec![plan_ref.id]),
        )?;
        self.pending = Some(PendingPlan {
            trace_id: trace_id.clone(),
            plan: plan.clone(),
            risk: gated.evaluation.authoritative_risk,
            created_at: Instant::now(),
        });
        Ok(Proposal::Ready {
            trace_id,
            plan: Box::new(plan),
        })
    }

    /// Dispatch only a plan retained by this instance. The host must show the
    /// preview and confirm its trace ID, then supply fresh device-state evidence.
    /// After dispatch begins, the plan is consumed even if the result is unknown.
    pub fn execute(
        &mut self,
        trace_id: &TraceId,
        host_confirmed: bool,
        freshness: StateFreshness,
    ) -> Result<ExecutionReceipt, AgentError> {
        if !self.config.execute_enabled {
            return Err(AgentError::ExecuteDisabled);
        }
        let pending = self
            .pending
            .as_ref()
            .filter(|p| p.trace_id == *trace_id)
            .ok_or(AgentError::UnknownPlan)?;
        if pending.created_at.elapsed() >= self.config.plan_ttl {
            self.pending = None;
            return Err(AgentError::PlanExpired);
        }
        if !host_confirmed || pending.plan.plan.policy == PolicyDecision::Deny {
            return Err(AgentError::ConfirmationRequired);
        }
        if freshness != StateFreshness::Fresh {
            return Err(AgentError::StateNotFresh);
        }
        if self.execution_attempts >= self.config.max_execution_attempts {
            self.pending = None;
            return Err(AgentError::ExecutionBudgetExhausted);
        }
        let pending = self.pending.take().ok_or(AgentError::UnknownPlan)?;
        self.execution_attempts += 1;
        // Record intent before any backend call; failure here is fail-closed.
        self.traces.append_step(
            trace_id,
            NewCommandStep::new("agent_dispatch", StepStatus::Started),
        )?;
        let result = self.transaction.execute(
            &self.executor,
            &pending.plan,
            pending.risk,
            host_confirmed,
            OffsetDateTime::now_utc(),
        );
        let status = match result {
            Ok(result) if result.success => ExecutionStatus::Acknowledged,
            Err(error @ (ExecutorError::RateLimited | ExecutorError::DuplicatePlan)) => {
                self.traces.append_step(
                    trace_id,
                    NewCommandStep::new("agent_preflight", StepStatus::Rejected),
                )?;
                return Err(AgentError::ExecutionPreflight(error));
            }
            _ => ExecutionStatus::OutcomeUnknown,
        };
        let step_status = match status {
            ExecutionStatus::Acknowledged => StepStatus::Succeeded,
            ExecutionStatus::OutcomeUnknown => StepStatus::Failed,
        };
        // Arbitrary custom executor errors/responses are not copied into traces.
        let completion_recorded = self
            .traces
            .append_step(
                trace_id,
                NewCommandStep::new("agent_completion", step_status).with_message(match status {
                    ExecutionStatus::Acknowledged => {
                        "backend acknowledged; physical state unverified"
                    }
                    ExecutionStatus::OutcomeUnknown => {
                        "outcome unknown; reconcile state before a new action"
                    }
                }),
            )
            .is_ok();
        Ok(ExecutionReceipt {
            trace_id: trace_id.clone(),
            status,
            completion_recorded,
        })
    }

    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    fn reject(&self, trace_id: TraceId, reason: RejectionReason) -> Result<Proposal, AgentError> {
        self.traces.append_step(
            &trace_id,
            NewCommandStep::new("agent_rejected", StepStatus::Rejected)
                .with_message(format!("{reason:?}")),
        )?;
        Ok(Proposal::Rejected { trace_id, reason })
    }
}

#[cfg(test)]
mod tests;
