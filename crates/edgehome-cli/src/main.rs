use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use edgehome_config::{RuntimeProfile, load_profile};
use edgehome_core::{
    DeviceId, DeviceType, DryRunPlan, ModelCandidate, NormalizedCommand, PolicyDecision, RiskLevel,
    UserInput,
};
use edgehome_eval::{
    EvalGateConfig, EvalReport, evaluate_case_error, evaluate_case_output, evaluate_release_gate,
    load_cases,
};
use edgehome_executor::{
    DryRunPlanner, ExecutionTransaction, HomeAssistantClient, HomeAssistantConfig,
    HomeAssistantExecutor, MatterBridgeConfig, MatterBridgeExecutor, MiotBridgeConfig,
    MiotBridgeExecutor, MockExecutor, MqttConfig, MqttExecutor, MqttSecrets, SecretsLoader,
    validate_bridge_base_url, validate_mqtt_topic,
};
use edgehome_gate::{GateCommandDecision, GateEngine, GateEvaluationRequest};
use edgehome_memory::{
    ContextAssembler, ExplicitMemoryWriteDetection, ExplicitMemoryWriteDetector,
    LongTermPreferenceStore, MemoryItem, MemoryKind, MemoryScope, MemoryWriteRequest,
    NewMemoryItem, PromptContext, ShortSessionMemory,
};
use edgehome_ollama::{
    ChatMessage, MiniCpm5Profile, OllamaClient, OutputGovernor, OutputGovernorReport,
    ResourcePressurePolicy, StructuredOutputRequest,
};
use edgehome_parser::{InputFlag, InputGuard, RulePreParser, SemanticNormalizer};
use edgehome_registry::{
    BackendKind, DeviceRecord, DeviceRegistry, DeviceResolutionInput, DeviceResolutionSource,
    StateFreshness,
};
use edgehome_storage::sqlite::integer;
use edgehome_storage::{
    EvidenceId, EvidenceKind, EvidenceRef, EvidenceStore, NewEvidence, SourceSystem,
};
use edgehome_trace::{
    AuditEvent, AuditSink, CommandStep, CommandTrace, GateCheck, GateOutcome, NewAuditEvent,
    NewCommandStep, StepStatus, TraceFrame, TraceId, TraceStore,
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(name = "edgehome")]
#[command(about = "EdgeHome Harness CLI")]
struct Cli {
    #[arg(long, default_value = "low_memory")]
    profile: String,

    #[arg(long, default_value = "configs")]
    config_dir: PathBuf,

    #[arg(long, default_value = "edgehome.sqlite")]
    db_path: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Backend {
        #[command(subcommand)]
        command: BackendCommand,
    },
    Parse {
        #[arg(long)]
        mock: bool,
        input: String,
    },
    DryRun {
        #[arg(long)]
        mock: bool,
        input: String,
    },
    Eval {
        cases_path: PathBuf,
        #[arg(long)]
        ollama: bool,
        #[arg(long)]
        gate: bool,
    },
    Replay {
        trace_id: String,
    },
    Execute {
        trace_id: String,
        #[arg(long)]
        confirm: bool,
        #[arg(long)]
        backend_config: Option<PathBuf>,
    },
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
    Pressure {
        #[arg(long)]
        free_memory_mb: u32,
    },
}

#[derive(Debug, Subcommand)]
enum BackendCommand {
    Check {
        #[arg(long, default_value = "all")]
        backend: String,
        #[arg(long)]
        registry: Option<PathBuf>,
        #[arg(long)]
        backend_config: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum TraceCommand {
    Show { trace_id: String },
    Export { trace_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipelineMode {
    Parse,
    DryRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendCheckTarget {
    All,
    HomeAssistant,
    Mqtt,
    Miot,
    Matter,
}

impl BackendCheckTarget {
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "home_assistant" | "ha" => Ok(Self::HomeAssistant),
            "mqtt" => Ok(Self::Mqtt),
            "miot" | "xiaomi" | "miio_local" => Ok(Self::Miot),
            "matter" | "matter_bridge" => Ok(Self::Matter),
            other => anyhow::bail!(
                "unknown backend `{other}`; expected all, home_assistant, mqtt, miot, or matter"
            ),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::HomeAssistant => "home_assistant",
            Self::Mqtt => "mqtt",
            Self::Miot => "miot",
            Self::Matter => "matter",
        }
    }

    fn includes(self, target: Self) -> bool {
        self == Self::All || self == target
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Config {
        command: ConfigCommand::Show,
    }) {
        Commands::Config {
            command: ConfigCommand::Show,
        } => {
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            print_json(&profile)?;
        }
        Commands::Config {
            command: ConfigCommand::Pressure { free_memory_mb },
        } => {
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let model_profile = MiniCpm5Profile::from_runtime_profile(&profile);
            let decision =
                ResourcePressurePolicy::default().adapt_profile(&model_profile, free_memory_mb);
            print_json(&json!({
                "profile": profile.name,
                "free_memory_mb": free_memory_mb,
                "decision": decision,
            }))?;
        }
        Commands::Backend {
            command:
                BackendCommand::Check {
                    backend,
                    registry,
                    backend_config,
                },
        } => {
            let output = check_backends(
                &cli.config_dir,
                &backend,
                registry.as_deref(),
                backend_config.as_deref(),
            )?;
            print_json(&output)?;
        }
        Commands::Parse { mock, input } => {
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let output = run_harness_pipeline(
                &cli.db_path,
                &cli.config_dir,
                &profile,
                input,
                PipelineMode::Parse,
                mock,
            )?;
            print_json(&output)?;
        }
        Commands::DryRun { mock, input } => {
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let output = run_harness_pipeline(
                &cli.db_path,
                &cli.config_dir,
                &profile,
                input,
                PipelineMode::DryRun,
                mock,
            )?;
            print_json(&output)?;
        }
        Commands::Eval {
            cases_path,
            ollama,
            gate,
        } => {
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let output = run_eval(
                &cli.db_path,
                &cli.config_dir,
                &profile,
                &cases_path,
                !ollama,
                gate,
            )?;
            print_json(&output)?;
            if gate_failed(&output) {
                std::process::exit(1);
            }
        }
        Commands::Replay { trace_id } => {
            let output = replay_trace(&cli.db_path, TraceId(trace_id))?;
            print_json(&output)?;
        }
        Commands::Execute {
            trace_id,
            confirm,
            backend_config,
        } => {
            let output = execute_trace(
                &cli.db_path,
                &cli.config_dir,
                TraceId(trace_id),
                confirm,
                backend_config.as_deref(),
            )?;
            print_json(&output)?;
        }
        Commands::Trace {
            command: TraceCommand::Show { trace_id },
        } => {
            let output = show_trace(&cli.db_path, TraceId(trace_id))?;
            print_json(&output)?;
        }
        Commands::Trace {
            command: TraceCommand::Export { trace_id },
        } => {
            let output = export_trace_frame(&cli.db_path, TraceId(trace_id))?;
            print_json(&output)?;
        }
    }

    Ok(())
}

fn run_harness_pipeline(
    db_path: &Path,
    config_dir: &Path,
    profile: &RuntimeProfile,
    input: String,
    mode: PipelineMode,
    use_mock: bool,
) -> anyhow::Result<Value> {
    ensure_db_parent(db_path)?;

    let trace_store = TraceStore::open(db_path)?;
    let audit_sink = AuditSink::open(db_path)?;
    let guarded = InputGuard::default().guard(input)?;
    let input = guarded.input;
    let input_flags = guarded
        .flags
        .iter()
        .map(|flag| input_flag_label(flag).to_owned())
        .collect::<Vec<_>>();

    let raw_user_input = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::RawUserInput,
        SourceSystem::User,
        "raw user input",
        json!({
            "text": input.text.as_str(),
            "input_flags": input_flags,
        }),
    ))?;

    let trace = trace_store.start_trace(raw_user_input.id.clone(), profile.name.to_string())?;
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("input_received", StepStatus::Succeeded)
            .with_evidence_refs(vec![raw_user_input.id.clone()]),
    )?;

    let registry_path = config_dir.join("devices.yaml");
    let registry = DeviceRegistry::load_from_path(&registry_path).with_context(|| {
        format!(
            "failed to load device registry `{}`",
            registry_path.display()
        )
    })?;
    let long_memory_store = LongTermPreferenceStore::open(db_path)?;

    if let Some(memory_write_output) = handle_explicit_memory_write(
        &trace_store,
        &audit_sink,
        &trace.trace_id,
        &input,
        &registry,
        &long_memory_store,
    )? {
        return Ok(memory_write_output);
    }

    let short_memory = if profile.memory_enabled {
        load_short_session_memory(&trace_store, usize::from(profile.max_short_memory_turns))?
    } else {
        ShortSessionMemory::new(1)
    };
    let long_items = if profile.memory_enabled {
        long_memory_store.list_relevant(&input.text, 3)?
    } else {
        Vec::new()
    };
    let prompt_context =
        ContextAssembler::from_profile(profile).assemble(&short_memory, &long_items);
    let memory_context_ref = if prompt_context.text.is_empty() {
        None
    } else {
        let evidence = trace_store.record_evidence(NewEvidence::new(
            EvidenceKind::MemoryItem,
            SourceSystem::Memory,
            "prompt memory context",
            serde_json::to_value(&prompt_context)?,
        ))?;
        trace_store.append_step(
            &trace.trace_id,
            NewCommandStep::new("context_assembler", StepStatus::Succeeded)
                .with_evidence_refs(vec![evidence.id.clone()]),
        )?;
        Some(evidence.id)
    };

    let candidate_run = match generate_candidate(profile, &input, &prompt_context, use_mock) {
        Ok(candidate_run) => candidate_run,
        Err(error) => {
            return model_failure_output(ModelFailureOutputInput {
                db_path,
                trace_store: &trace_store,
                audit_sink: &audit_sink,
                trace_id: &trace.trace_id,
                raw_user_input_ref: raw_user_input.id.clone(),
                input: &input,
                mode,
                use_mock,
                profile,
                prompt_context: &prompt_context,
                error,
            });
        }
    };
    let candidate = candidate_run.candidate;
    let raw_model_output_text = candidate_run.raw_output;
    let raw_model_output = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::RawModelOutput,
        SourceSystem::Model,
        "model output candidate",
        json!({
            "model": candidate_run.model_name,
            "mock": use_mock,
            "raw_output": raw_model_output_text,
            "output_governor": candidate_run.output_governor_report,
            "model_params": candidate_run.model_params,
            "prompt_hash": candidate_run.prompt_hash,
            "context_chars": prompt_context.text.chars().count(),
        }),
    ))?;
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("model_output", StepStatus::Succeeded)
            .with_evidence_refs(vec![raw_model_output.id.clone()]),
    )?;

    let parsed_json = serde_json::to_value(&candidate)?;
    let parsed_json_ref = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::ParsedJson,
        SourceSystem::Parser,
        "parsed model candidate JSON",
        parsed_json.clone(),
    ))?;
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("parse_json", StepStatus::Succeeded)
            .with_evidence_refs(vec![parsed_json_ref.id.clone()]),
    )?;

    let (candidate_for_normalization, repaired_candidate_ref) =
        if let Some(repaired) = repair_candidate_with_rules(&input, &candidate) {
            let repaired_ref = trace_store.record_evidence(NewEvidence::new(
                EvidenceKind::ParsedJson,
                SourceSystem::Parser,
                "deterministic repaired model candidate JSON",
                json!({
                    "before": candidate.clone(),
                    "after": repaired.clone(),
                    "repair_source": "rule_pre_parser",
                }),
            ))?;
            trace_store.append_step(
                &trace.trace_id,
                NewCommandStep::new("candidate_repair", StepStatus::Fallback)
                    .with_message(
                        "deterministic parser repaired missing or conflicting candidate slots",
                    )
                    .with_evidence_refs(vec![parsed_json_ref.id.clone(), repaired_ref.id.clone()]),
            )?;
            (repaired, Some(repaired_ref.id.clone()))
        } else {
            (candidate.clone(), None)
        };

    let mut normalized = normalize_model_candidate(&candidate_for_normalization)?;
    if let Some((resolved, source)) = resolve_device_target(
        &normalized,
        &candidate_for_normalization,
        &long_items,
        &registry,
    )? {
        let source_system = if source.starts_with("long_memory") {
            SourceSystem::Memory
        } else {
            SourceSystem::Registry
        };
        let evidence_kind = if source_system == SourceSystem::Memory {
            EvidenceKind::MemoryItem
        } else {
            EvidenceKind::DeviceRegistrySnapshot
        };
        let alias_resolution = trace_store.record_evidence(NewEvidence::new(
            evidence_kind,
            source_system,
            "device resolver resolved command target",
            json!({
                "source": source,
                "before": normalized.clone(),
                "after": resolved.clone(),
            }),
        ))?;
        trace_store.append_step(
            &trace.trace_id,
            NewCommandStep::new("device_resolution", StepStatus::Succeeded)
                .with_evidence_refs(vec![alias_resolution.id.clone()]),
        )?;
        normalized = resolved;
    }
    if profile.memory_enabled
        && let Some(resolved) = short_memory.resolve_relative_command(&normalized)
        && resolved != normalized
    {
        let memory_resolution = trace_store.record_evidence(NewEvidence::new(
            EvidenceKind::MemoryItem,
            SourceSystem::Memory,
            "short memory resolved relative command",
            json!({
                "before": normalized.clone(),
                "after": resolved.clone(),
            }),
        ))?;
        trace_store.append_step(
            &trace.trace_id,
            NewCommandStep::new("short_memory_resolution", StepStatus::Succeeded)
                .with_evidence_refs(vec![memory_resolution.id.clone()]),
        )?;
        normalized = resolved;
    }
    let normalized_ref = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::NormalizedCommand,
        SourceSystem::Normalizer,
        "normalized command",
        serde_json::to_value(&normalized)?,
    ))?;

    let normalized_status = if normalized.can_enter_policy_gate() {
        StepStatus::Succeeded
    } else {
        StepStatus::Rejected
    };
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("normalize", normalized_status)
            .with_evidence_refs(vec![parsed_json_ref.id.clone(), normalized_ref.id.clone()]),
    )?;

    let gate_engine = GateEngine::new(&trace_store, &registry);
    let mut gate_evidence_refs = vec![
        raw_user_input.id.clone(),
        raw_model_output.id.clone(),
        parsed_json_ref.id.clone(),
        normalized_ref.id.clone(),
    ];
    if let Some(repaired_candidate_ref) = repaired_candidate_ref.clone() {
        gate_evidence_refs.push(repaired_candidate_ref);
    }
    if let Some(memory_context_ref) = memory_context_ref {
        gate_evidence_refs.push(memory_context_ref);
    }
    let gate_request = GateEvaluationRequest::new(trace.trace_id.clone(), normalized.clone())
        .with_evidence_refs(gate_evidence_refs.clone())
        .with_input_flags(input_flags.clone())
        .with_state_freshness(StateFreshness::Fresh)
        .with_dry_run_ready(mode == PipelineMode::DryRun);
    let (gate_evaluation, gated_command) = if mode == PipelineMode::DryRun {
        match gate_engine.verify(gate_request)? {
            GateCommandDecision::Accepted { gated_command } => {
                (gated_command.evaluation.clone(), Some(gated_command))
            }
            GateCommandDecision::Rejected { evaluation } => (evaluation, None),
        }
    } else {
        (gate_engine.evaluate(gate_request)?, None)
    };

    let gate_status = if gate_evaluation.policy_decision == PolicyDecision::Deny
        || !gate_evaluation.blocking_reasons.is_empty()
    {
        StepStatus::Rejected
    } else {
        StepStatus::Succeeded
    };
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("gate_engine", gate_status).with_evidence_refs(gate_evidence_refs),
    )?;

    let mut dry_run_plan = None;
    if mode == PipelineMode::DryRun {
        if let Some(gated_command) = gated_command.as_ref() {
            if let Some(device_id) = gated_command.command.device_id.as_ref() {
                let device = registry.get_device(device_id)?;
                let planned = DryRunPlanner.plan_gated(gated_command, device)?;
                let dry_run_ref = trace_store.record_evidence(NewEvidence::new(
                    EvidenceKind::DryRunPlan,
                    SourceSystem::Executor,
                    "dry-run execution plan",
                    serde_json::to_value(&planned)?,
                ))?;
                trace_store.append_step(
                    &trace.trace_id,
                    NewCommandStep::new("dry_run_plan", StepStatus::Succeeded).with_evidence_refs(
                        vec![normalized_ref.id.clone(), dry_run_ref.id.clone()],
                    ),
                )?;
                dry_run_plan = Some(planned);
            } else {
                trace_store.append_step(
                    &trace.trace_id,
                    NewCommandStep::new("dry_run_missing_device", StepStatus::Rejected)
                        .with_message("dry-run planner requires a resolved device_id")
                        .with_evidence_refs(vec![normalized_ref.id.clone()]),
                )?;
            }
        } else {
            trace_store.append_step(
                &trace.trace_id,
                NewCommandStep::new("dry_run_rejected_by_gate", StepStatus::Rejected)
                    .with_message("dry-run planner only accepts non-denied gated commands")
                    .with_evidence_refs(vec![normalized_ref.id.clone()]),
            )?;
        }
    }

    let audit_event_type = match mode {
        PipelineMode::Parse => "harness_parse_completed",
        PipelineMode::DryRun if dry_run_plan.is_some() => "harness_dry_run_plan_generated",
        PipelineMode::DryRun => "harness_dry_run_rejected",
    };
    let policy_decision = gate_evaluation.policy_decision.clone();
    let dry_run_ready = dry_run_plan.is_some();
    audit_sink.append(
        NewAuditEvent::new(
            audit_event_type,
            "harness pipeline completed with gated policy decision",
            json!({
                "mode": mode.as_str(),
                "model_mode": if use_mock { "mock" } else { "ollama" },
                "trace_id": trace.trace_id.0.as_str(),
                "policy_decision": policy_decision,
                "dry_run_ready": dry_run_ready,
                "executable": false,
                "execute_enabled": false,
            }),
        )
        .with_trace_id(trace.trace_id.clone()),
    )?;

    let execution_plan = dry_run_plan.as_ref().map(|plan| plan.plan.clone());
    let trace_frame = build_trace_frame(&load_trace_bundle(db_path, trace.trace_id.clone())?);
    Ok(json!({
        "trace_id": trace.trace_id,
        "trace_frame": trace_frame,
        "mode": mode.as_str(),
        "mock": use_mock,
        "model_mode": if use_mock { "mock" } else { "ollama" },
        "model_candidate": candidate,
        "repaired_model_candidate": if repaired_candidate_ref.is_some() {
            Some(candidate_for_normalization)
        } else {
            None
        },
        "normalized_command": normalized,
        "gate_evaluation": gate_evaluation,
        "evidence_refs": {
            "raw_user_input": raw_user_input.id,
            "raw_model_output": raw_model_output.id,
            "parsed_json": parsed_json_ref.id,
            "repaired_model_candidate": repaired_candidate_ref,
            "normalized_command": normalized_ref.id,
        },
        "policy_decision": policy_decision,
        "dry_run_plan": dry_run_plan,
        "execution_plan": execution_plan,
        "executable": false,
        "execute_enabled": false,
        "note": "M11 pipeline runs guard/context/model/parser/memory/gate/dry-run; real execute remains disabled by default"
    }))
}

struct ModelFailureOutputInput<'a> {
    db_path: &'a Path,
    trace_store: &'a TraceStore,
    audit_sink: &'a AuditSink,
    trace_id: &'a TraceId,
    raw_user_input_ref: EvidenceId,
    input: &'a UserInput,
    mode: PipelineMode,
    use_mock: bool,
    profile: &'a RuntimeProfile,
    prompt_context: &'a PromptContext,
    error: anyhow::Error,
}

#[derive(Debug)]
struct ModelGenerationFailure {
    message: String,
    model_name: Option<String>,
    raw_output: Option<String>,
    output_governor_report: Option<OutputGovernorReport>,
    model_params: Option<Value>,
    prompt_hash: Option<String>,
}

impl fmt::Display for ModelGenerationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ModelGenerationFailure {}

fn model_failure_output(input: ModelFailureOutputInput<'_>) -> anyhow::Result<Value> {
    let model_failure = input.error.downcast_ref::<ModelGenerationFailure>();
    let failure_reason = model_failure
        .map(|failure| failure.message.clone())
        .unwrap_or_else(|| input.error.to_string());
    let model_profile = MiniCpm5Profile::from_runtime_profile(input.profile);
    let model_name = model_failure
        .and_then(|failure| failure.model_name.clone())
        .unwrap_or_else(|| {
            if input.use_mock {
                "MockModel".to_owned()
            } else {
                input.profile.model_name.clone()
            }
        });
    let model_params = model_failure
        .and_then(|failure| failure.model_params.clone())
        .unwrap_or_else(|| {
            if input.use_mock {
                json!({ "mock": true })
            } else {
                json!(model_profile.options())
            }
        });
    let prompt_hash = model_failure
        .and_then(|failure| failure.prompt_hash.clone())
        .unwrap_or_else(|| {
            if input.use_mock {
                stable_prompt_hash(&input.input.text)
            } else {
                stable_prompt_hash(&format!(
                    "{}\n{}",
                    system_prompt(input.prompt_context),
                    input.input.text
                ))
            }
        });
    let raw_output = model_failure.and_then(|failure| failure.raw_output.as_ref());
    let output_governor = model_failure
        .and_then(|failure| failure.output_governor_report.as_ref())
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or_else(|| {
            json!({
                "accepted": false,
                "failure_kind": "model_output_failed",
                "failure_message": failure_reason.as_str(),
                "recommended_fallback": null,
            })
        });
    let model_error_kind = if raw_output.is_some() {
        "output_governor_rejected"
    } else {
        "model_output_failed"
    };

    let model_failure_ref = input.trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::RawModelOutput,
        SourceSystem::Model,
        "model output unavailable or rejected",
        json!({
            "model": model_name,
            "mock": input.use_mock,
            "raw_output": raw_output,
            "model_error": {
                "kind": model_error_kind,
                "message": failure_reason.as_str(),
            },
            "output_governor": output_governor,
            "model_params": model_params,
            "prompt_hash": prompt_hash,
            "context_chars": input.prompt_context.text.chars().count(),
        }),
    ))?;
    input.trace_store.append_step(
        input.trace_id,
        NewCommandStep::new("model_output_failed", StepStatus::Failed)
            .with_message(failure_reason.clone())
            .with_evidence_refs(vec![
                input.raw_user_input_ref.clone(),
                model_failure_ref.id.clone(),
            ]),
    )?;

    let policy_snapshot_ref = input.trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::PolicyRuleSnapshot,
        SourceSystem::Policy,
        "fail-closed policy decision after model failure",
        json!({
            "policy_version": "policy.v1",
            "decision": PolicyDecision::Deny,
            "reason": "model output was unavailable or rejected before normalization",
            "model_failure": failure_reason.as_str(),
        }),
    ))?;
    input.trace_store.append_step(
        input.trace_id,
        NewCommandStep::new("fail_closed_before_gate", StepStatus::Rejected)
            .with_message("model failure prevented normalization; dry-run and execution disabled")
            .with_evidence_refs(vec![
                model_failure_ref.id.clone(),
                policy_snapshot_ref.id.clone(),
            ]),
    )?;

    input.audit_sink.append(
        NewAuditEvent::new(
            "harness_model_output_rejected",
            "model output was unavailable or rejected; harness failed closed",
            json!({
                "mode": input.mode.as_str(),
                "model_mode": if input.use_mock { "mock" } else { "ollama" },
                "trace_id": input.trace_id.0.as_str(),
                "policy_decision": PolicyDecision::Deny,
                "dry_run_ready": false,
                "executable": false,
                "execute_enabled": false,
                "failure_reason": failure_reason.as_str(),
            }),
        )
        .with_trace_id(input.trace_id.clone()),
    )?;

    let trace_frame = build_trace_frame(&load_trace_bundle(input.db_path, input.trace_id.clone())?);
    Ok(json!({
        "trace_id": input.trace_id,
        "trace_frame": trace_frame,
        "mode": input.mode.as_str(),
        "mock": input.use_mock,
        "model_mode": if input.use_mock { "mock" } else { "ollama" },
        "model_candidate": null,
        "normalized_command": null,
        "gate_evaluation": {
            "trace_id": input.trace_id,
            "policy_decision": PolicyDecision::Deny,
            "authoritative_risk": RiskLevel::Unknown,
            "device_id": null,
            "executable": false,
            "requires_confirmation": false,
            "blocking_reasons": [
                "model output was unavailable or rejected before normalization"
            ],
            "gate_checks": []
        },
        "evidence_refs": {
            "raw_user_input": input.raw_user_input_ref,
            "raw_model_output": model_failure_ref.id,
            "policy_snapshot": policy_snapshot_ref.id,
        },
        "policy_decision": PolicyDecision::Deny,
        "dry_run_plan": null,
        "execution_plan": null,
        "executable": false,
        "execute_enabled": false,
        "failure_reason": failure_reason,
        "note": "model-path failures are traceable and fail closed; no dry-run or real execution was produced"
    }))
}

fn run_eval(
    db_path: &Path,
    config_dir: &Path,
    profile: &RuntimeProfile,
    cases_path: &Path,
    use_mock: bool,
    gate_enabled: bool,
) -> anyhow::Result<Value> {
    let cases = load_cases(cases_path)?;
    let mut results = Vec::with_capacity(cases.len());

    for case in cases {
        let result = match run_harness_pipeline(
            db_path,
            config_dir,
            profile,
            case.input.clone(),
            PipelineMode::DryRun,
            use_mock,
        ) {
            Ok(output) => evaluate_case_output(&case, &output)
                .unwrap_or_else(|error| evaluate_case_error(&case, error.to_string())),
            Err(error) => evaluate_case_error(&case, error.to_string()),
        };
        results.push(result);
    }

    let report = EvalReport::from_results(results);
    let gate = gate_enabled.then(|| evaluate_release_gate(&report, &EvalGateConfig::default()));
    let mut output = json!({
        "cases_path": cases_path.display().to_string(),
        "profile": profile.name.to_string(),
        "model_mode": if use_mock { "mock" } else { "ollama" },
        "report": report,
    });

    if let Some(gate) = gate {
        output
            .as_object_mut()
            .expect("eval output is an object")
            .insert("gate".to_owned(), json!(gate));
    }

    Ok(output)
}

fn gate_failed(output: &Value) -> bool {
    output
        .get("gate")
        .and_then(|gate| gate.get("passed"))
        .and_then(Value::as_bool)
        == Some(false)
}

fn check_backends(
    config_dir: &Path,
    backend: &str,
    registry_override: Option<&Path>,
    backend_config: Option<&Path>,
) -> anyhow::Result<Value> {
    let target = BackendCheckTarget::parse(backend)?;
    if target == BackendCheckTarget::All && backend_config.is_some() {
        anyhow::bail!("--backend-config is only valid when checking one backend");
    }

    let registry_path = registry_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| config_dir.join("devices.yaml"));
    let registry = DeviceRegistry::load_from_path(&registry_path).with_context(|| {
        format!(
            "failed to load backend check registry `{}`",
            registry_path.display()
        )
    })?;
    let devices = registry.devices();
    let mut checks = Vec::new();

    if target.includes(BackendCheckTarget::HomeAssistant) {
        checks.push(check_home_assistant_backend(
            config_dir,
            devices,
            backend_config,
        )?);
    }
    if target.includes(BackendCheckTarget::Mqtt) {
        checks.push(check_mqtt_backend(config_dir, devices, backend_config)?);
    }
    if target.includes(BackendCheckTarget::Miot) {
        checks.push(check_miot_backend(config_dir, devices, backend_config)?);
    }
    if target.includes(BackendCheckTarget::Matter) {
        checks.push(check_matter_backend(config_dir, devices, backend_config)?);
    }

    let configured_backend_count = checks
        .iter()
        .filter(|check| json_usize(check, "route_count") > 0)
        .count();
    let dry_run_ready_count = checks
        .iter()
        .filter(|check| json_bool(check, "dry_run_ready"))
        .count();
    let execute_ready_count = checks
        .iter()
        .filter(|check| json_bool(check, "execute_ready"))
        .count();

    Ok(json!({
        "registry_path": registry_path.display().to_string(),
        "target": target.as_str(),
        "checks": checks,
        "summary": {
            "checked_backends": checks.len(),
            "configured_backends": configured_backend_count,
            "dry_run_ready_backends": dry_run_ready_count,
            "execute_ready_backends": execute_ready_count,
            "real_execution_default": "disabled",
            "note": "backend check is read-only; it validates config and routes but never contacts devices"
        }
    }))
}

fn check_home_assistant_backend(
    config_dir: &Path,
    devices: &[DeviceRecord],
    backend_config: Option<&Path>,
) -> anyhow::Result<Value> {
    let config_path =
        backend_config_path(config_dir, backend_config, "home_assistant.yaml.example");
    let config = HomeAssistantConfig::load_from_path(&config_path)
        .with_context(|| format!("failed to load `{}`", config_path.display()))?;
    let readiness = HomeAssistantExecutor::validate_routes(devices)?;
    let secret_available = SecretsLoader::load(&config)?.is_some();
    let dry_run_ready = readiness.route_count > 0 && readiness.routes_valid;
    let execute_ready = dry_run_ready && config.execute_enabled && secret_available;

    Ok(json!({
        "backend": "home_assistant",
        "status": backend_status(readiness.route_count, dry_run_ready),
        "config_path": config_path.display().to_string(),
        "route_count": readiness.route_count,
        "routes_valid": readiness.routes_valid,
        "dry_run_ready": dry_run_ready,
        "execute_enabled": config.execute_enabled,
        "secret_available": secret_available,
        "post_state_verification": config.verify_state_after_execute,
        "execute_ready": execute_ready,
        "execution_blocker": execution_blocker(
            dry_run_ready,
            config.execute_enabled,
            secret_available,
        ),
    }))
}

fn check_mqtt_backend(
    config_dir: &Path,
    devices: &[DeviceRecord],
    backend_config: Option<&Path>,
) -> anyhow::Result<Value> {
    let config_path = backend_config_path(config_dir, backend_config, "adapters/mqtt.example.yaml");
    let config = MqttConfig::load_from_path(&config_path)
        .with_context(|| format!("failed to load `{}`", config_path.display()))?;
    if config.qos > 2 {
        anyhow::bail!("invalid MQTT QoS `{}`; expected 0, 1, or 2", config.qos);
    }
    let route_count = validate_mqtt_routes(devices)?;
    let secret_available = MqttSecrets::load(&config)?.is_some();
    let dry_run_ready = route_count > 0;
    let execute_ready = dry_run_ready && config.execute_enabled && secret_available;

    Ok(json!({
        "backend": "mqtt",
        "status": backend_status(route_count, dry_run_ready),
        "config_path": config_path.display().to_string(),
        "route_count": route_count,
        "routes_valid": true,
        "dry_run_ready": dry_run_ready,
        "execute_enabled": config.execute_enabled,
        "secret_available": secret_available,
        "qos": config.qos,
        "retain": config.retain,
        "execute_ready": execute_ready,
        "execution_blocker": execution_blocker(
            dry_run_ready,
            config.execute_enabled,
            secret_available,
        ),
    }))
}

fn check_miot_backend(
    config_dir: &Path,
    devices: &[DeviceRecord],
    backend_config: Option<&Path>,
) -> anyhow::Result<Value> {
    let config_path = backend_config_path(config_dir, backend_config, "adapters/miot.example.yaml");
    let config = MiotBridgeConfig::load_from_path(&config_path)
        .with_context(|| format!("failed to load `{}`", config_path.display()))?;
    validate_bridge_base_url_for_check("miot_bridge", &config.base_url)?;
    let route_count = validate_miot_routes(&config, devices)?;
    let secret_available = env_secret_available(&config.token_env);
    let dry_run_ready = route_count > 0;
    let execute_ready = dry_run_ready && config.execute_enabled && secret_available;

    Ok(json!({
        "backend": "miot",
        "status": backend_status(route_count, dry_run_ready),
        "config_path": config_path.display().to_string(),
        "route_count": route_count,
        "routes_valid": true,
        "bridge_url_configured": !config.base_url.trim().is_empty(),
        "dry_run_ready": dry_run_ready,
        "execute_enabled": config.execute_enabled,
        "secret_available": secret_available,
        "execute_ready": execute_ready,
        "execution_blocker": execution_blocker(
            dry_run_ready,
            config.execute_enabled,
            secret_available,
        ),
        "claim_boundary": "bridge_request_adapter_only",
    }))
}

fn check_matter_backend(
    config_dir: &Path,
    devices: &[DeviceRecord],
    backend_config: Option<&Path>,
) -> anyhow::Result<Value> {
    let config_path =
        backend_config_path(config_dir, backend_config, "adapters/matter.example.yaml");
    let config = MatterBridgeConfig::load_from_path(&config_path)
        .with_context(|| format!("failed to load `{}`", config_path.display()))?;
    validate_bridge_base_url_for_check("matter_bridge", &config.base_url)?;
    let route_count = validate_matter_routes(&config, devices)?;
    let secret_available = env_secret_available(&config.token_env);
    let dry_run_ready = route_count > 0;
    let execute_ready = dry_run_ready && config.execute_enabled && secret_available;

    Ok(json!({
        "backend": "matter",
        "status": backend_status(route_count, dry_run_ready),
        "config_path": config_path.display().to_string(),
        "route_count": route_count,
        "routes_valid": true,
        "bridge_url_configured": !config.base_url.trim().is_empty(),
        "dry_run_ready": dry_run_ready,
        "execute_enabled": config.execute_enabled,
        "secret_available": secret_available,
        "execute_ready": execute_ready,
        "execution_blocker": execution_blocker(
            dry_run_ready,
            config.execute_enabled,
            secret_available,
        ),
        "claim_boundary": "controller_bridge_request_adapter_only",
    }))
}

fn validate_mqtt_routes(devices: &[DeviceRecord]) -> anyhow::Result<usize> {
    let mut route_count = 0;
    for device in devices
        .iter()
        .filter(|device| device.backend == BackendKind::Mqtt)
    {
        validate_mqtt_topic(&device.backend_entity_id)
            .with_context(|| format!("invalid MQTT topic route for `{}`", device.device_id.0))?;
        route_count += 1;
    }
    Ok(route_count)
}

fn validate_miot_routes(
    config: &MiotBridgeConfig,
    devices: &[DeviceRecord],
) -> anyhow::Result<usize> {
    let mut executor = MiotBridgeExecutor::new(config.clone(), None);
    let mut route_count = 0;
    for device in devices
        .iter()
        .filter(|device| device.backend == BackendKind::MiioLocal)
    {
        executor = executor
            .with_route(device.device_id.clone(), &device.backend_entity_id)
            .with_context(|| format!("invalid MIoT bridge route for `{}`", device.device_id.0))?;
        route_count += 1;
    }
    Ok(route_count)
}

fn validate_matter_routes(
    config: &MatterBridgeConfig,
    devices: &[DeviceRecord],
) -> anyhow::Result<usize> {
    let mut executor = MatterBridgeExecutor::new(config.clone(), None);
    let mut route_count = 0;
    for device in devices
        .iter()
        .filter(|device| device.backend == BackendKind::MatterBridge)
    {
        executor = executor
            .with_route(device.device_id.clone(), &device.backend_entity_id)
            .with_context(|| format!("invalid Matter bridge route for `{}`", device.device_id.0))?;
        route_count += 1;
    }
    Ok(route_count)
}

fn validate_bridge_base_url_for_check(backend: &'static str, base_url: &str) -> anyhow::Result<()> {
    validate_bridge_base_url(backend, base_url)?;
    Ok(())
}

fn backend_status(route_count: usize, dry_run_ready: bool) -> &'static str {
    if route_count == 0 {
        "not_configured"
    } else if dry_run_ready {
        "ready"
    } else {
        "invalid"
    }
}

fn execution_blocker(
    dry_run_ready: bool,
    execute_enabled: bool,
    secret_available: bool,
) -> Option<&'static str> {
    if !dry_run_ready {
        Some("no_valid_routes")
    } else if !execute_enabled {
        Some("execute_enabled_false")
    } else if !secret_available {
        Some("secret_missing")
    } else {
        None
    }
}

fn env_secret_available(env_var: &str) -> bool {
    let env_var = env_var.trim();
    !env_var.is_empty()
        && std::env::var(env_var)
            .ok()
            .is_some_and(|value| !value.trim().is_empty())
}

fn json_bool(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn json_usize(value: &Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0)
}

fn show_trace(db_path: &Path, trace_id: TraceId) -> anyhow::Result<Value> {
    let bundle = load_trace_bundle(db_path, trace_id)?;
    let trace_frame = build_trace_frame(&bundle);
    let raw_user_input = latest_evidence(&bundle.evidence, EvidenceKind::RawUserInput)
        .map(|item| item.content.clone());

    Ok(json!({
        "trace_frame": trace_frame,
        "trace": bundle.trace,
        "raw_user_input": raw_user_input,
        "steps": bundle.steps,
        "gate_checks": bundle.gate_checks,
        "audit_events": bundle.audit_events,
    }))
}

fn replay_trace(db_path: &Path, trace_id: TraceId) -> anyhow::Result<Value> {
    let bundle = load_trace_bundle(db_path, trace_id)?;
    let trace_frame = build_trace_frame(&bundle);
    let normalized_command = latest_evidence(&bundle.evidence, EvidenceKind::NormalizedCommand)
        .map(|item| item.content.clone());
    let dry_run_plan = latest_evidence(&bundle.evidence, EvidenceKind::DryRunPlan)
        .map(|item| item.content.clone());
    let policy_snapshot = latest_evidence(&bundle.evidence, EvidenceKind::PolicyRuleSnapshot)
        .map(|item| item.content.clone());

    Ok(json!({
        "trace_frame": trace_frame,
        "trace": bundle.trace,
        "steps": bundle.steps,
        "gate_checks": bundle.gate_checks,
        "audit_events": bundle.audit_events,
        "replay_summary": {
            "normalized_command": normalized_command,
            "policy_snapshot": policy_snapshot,
            "dry_run_plan": dry_run_plan,
            "gate_count": bundle.gate_checks.len(),
            "audit_count": bundle.audit_events.len(),
        },
        "evidence": bundle.evidence,
    }))
}

fn execute_trace(
    db_path: &Path,
    config_dir: &Path,
    trace_id: TraceId,
    user_confirmed: bool,
    backend_config: Option<&Path>,
) -> anyhow::Result<Value> {
    ensure_db_parent(db_path)?;

    let bundle = load_trace_bundle(db_path, trace_id.clone())?;
    let dry_run_ref = latest_evidence(&bundle.evidence, EvidenceKind::DryRunPlan)
        .context("trace has no dry-run plan to execute")?;
    let dry_run: DryRunPlan = serde_json::from_value(dry_run_ref.content.clone())
        .context("failed to parse dry-run plan evidence")?;
    let risk = latest_evidence(&bundle.evidence, EvidenceKind::NormalizedCommand)
        .and_then(|evidence| {
            serde_json::from_value::<NormalizedCommand>(evidence.content.clone()).ok()
        })
        .map(|command| command.risk)
        .unwrap_or(RiskLevel::High);

    let registry_path = config_dir.join("devices.yaml");
    let registry = DeviceRegistry::load_from_path(&registry_path).with_context(|| {
        format!(
            "failed to load device registry `{}`",
            registry_path.display()
        )
    })?;
    let device = registry.get_device(&dry_run.plan.target)?;
    let mut transaction = ExecutionTransaction::default();

    let result = match dry_run.backend.as_str() {
        "mock" => {
            let executor = MockExecutor::new(true);
            transaction.execute(
                &executor,
                &dry_run,
                risk,
                user_confirmed,
                time::OffsetDateTime::now_utc(),
            )?
        }
        "home_assistant" => {
            let config_path =
                backend_config_path(config_dir, backend_config, "home_assistant.yaml.example");
            let config = HomeAssistantConfig::load_from_path(&config_path)
                .with_context(|| format!("failed to load `{}`", config_path.display()))?;
            let secrets = SecretsLoader::load(&config)?;
            let client = HomeAssistantClient::new(&config, secrets);
            let executor = HomeAssistantExecutor::new(client, config.execute_enabled)
                .with_post_state_verification(config.verify_state_after_execute)
                .with_route(device.device_id.clone(), &device.backend_entity_id)?;
            transaction.execute(
                &executor,
                &dry_run,
                risk,
                user_confirmed,
                time::OffsetDateTime::now_utc(),
            )?
        }
        "mqtt" => {
            let config_path =
                backend_config_path(config_dir, backend_config, "adapters/mqtt.example.yaml");
            let config = MqttConfig::load_from_path(&config_path)
                .with_context(|| format!("failed to load `{}`", config_path.display()))?;
            let secrets = MqttSecrets::load(&config)?;
            let executor = MqttExecutor::from_config(&config, secrets)
                .with_route(device.device_id.clone(), &device.backend_entity_id)?;
            transaction.execute(
                &executor,
                &dry_run,
                risk,
                user_confirmed,
                time::OffsetDateTime::now_utc(),
            )?
        }
        "miio_local" => {
            let config_path =
                backend_config_path(config_dir, backend_config, "adapters/miot.example.yaml");
            let config = MiotBridgeConfig::load_from_path(&config_path)
                .with_context(|| format!("failed to load `{}`", config_path.display()))?;
            let executor = MiotBridgeExecutor::from_config(&config)?
                .with_route(device.device_id.clone(), &device.backend_entity_id)?;
            transaction.execute(
                &executor,
                &dry_run,
                risk,
                user_confirmed,
                time::OffsetDateTime::now_utc(),
            )?
        }
        "matter_bridge" => {
            let config_path =
                backend_config_path(config_dir, backend_config, "adapters/matter.example.yaml");
            let config = MatterBridgeConfig::load_from_path(&config_path)
                .with_context(|| format!("failed to load `{}`", config_path.display()))?;
            let executor = MatterBridgeExecutor::from_config(&config)?
                .with_route(device.device_id.clone(), &device.backend_entity_id)?;
            transaction.execute(
                &executor,
                &dry_run,
                risk,
                user_confirmed,
                time::OffsetDateTime::now_utc(),
            )?
        }
        backend => anyhow::bail!("execute is not wired for backend `{backend}`"),
    };

    let trace_store = TraceStore::open(db_path)?;
    let audit_sink = AuditSink::open(db_path)?;
    let result_ref = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::ExecutorResponse,
        SourceSystem::Executor,
        "real executor response",
        serde_json::to_value(&result)?,
    ))?;
    trace_store.append_step(
        &trace_id,
        NewCommandStep::new("real_execute", StepStatus::Succeeded)
            .with_evidence_refs(vec![dry_run_ref.id.clone(), result_ref.id.clone()]),
    )?;
    audit_sink.append(
        NewAuditEvent::new(
            "real_execution_completed",
            "explicit execute command completed through backend executor",
            json!({
                "trace_id": trace_id.0.as_str(),
                "backend": dry_run.backend.as_str(),
                "target": dry_run.plan.target.0.as_str(),
                "user_confirmed": user_confirmed,
                "success": result.success,
            }),
        )
        .with_trace_id(trace_id.clone()),
    )?;

    Ok(json!({
        "trace_id": trace_id,
        "mode": "execute",
        "backend": dry_run.backend,
        "target": dry_run.plan.target,
        "user_confirmed": user_confirmed,
        "result": result,
        "evidence_refs": {
            "dry_run_plan": dry_run_ref.id,
            "executor_response": result_ref.id,
        },
        "note": "real execution is explicit opt-in and remains disabled unless backend private config enables it"
    }))
}

fn export_trace_frame(db_path: &Path, trace_id: TraceId) -> anyhow::Result<TraceFrame> {
    let bundle = load_trace_bundle(db_path, trace_id)?;
    Ok(build_trace_frame(&bundle))
}

struct TraceBundle {
    trace: CommandTrace,
    steps: Vec<CommandStep>,
    gate_checks: Vec<GateCheck>,
    audit_events: Vec<AuditEvent>,
    evidence: Vec<EvidenceRef>,
}

fn load_trace_bundle(db_path: &Path, trace_id: TraceId) -> anyhow::Result<TraceBundle> {
    ensure_db_parent(db_path)?;

    let trace_store = TraceStore::open(db_path)?;
    let evidence_store = EvidenceStore::open(db_path)?;
    let audit_sink = AuditSink::open(db_path)?;

    let trace = trace_store.read_trace(&trace_id)?;
    let steps = trace_store.steps_for_trace(&trace_id)?;
    let gate_checks = trace_store.gate_checks_for_trace(&trace_id)?;
    let audit_events = audit_sink.events_for_trace(&trace_id)?;

    let mut evidence_ids = HashSet::new();
    evidence_ids.insert(trace.raw_user_input_ref.0.clone());
    for step in &steps {
        for evidence_id in &step.evidence_refs {
            evidence_ids.insert(evidence_id.0.clone());
        }
    }
    for check in &gate_checks {
        for evidence_id in &check.evidence_refs {
            evidence_ids.insert(evidence_id.0.clone());
        }
    }

    let mut evidence = Vec::with_capacity(evidence_ids.len());
    for evidence_id in evidence_ids {
        evidence.push(evidence_store.read(&edgehome_storage::EvidenceId(evidence_id))?);
    }
    evidence.sort_by_key(|item| item.created_at);

    Ok(TraceBundle {
        trace,
        steps,
        gate_checks,
        audit_events,
        evidence,
    })
}

fn build_trace_frame(bundle: &TraceBundle) -> TraceFrame {
    let raw_user_input = latest_evidence(&bundle.evidence, EvidenceKind::RawUserInput);
    let raw_model_output = latest_evidence(&bundle.evidence, EvidenceKind::RawModelOutput);
    let parsed_json = latest_evidence(&bundle.evidence, EvidenceKind::ParsedJson);
    let normalized_command = latest_evidence(&bundle.evidence, EvidenceKind::NormalizedCommand);
    let dry_run_plan = latest_evidence(&bundle.evidence, EvidenceKind::DryRunPlan);
    let executor_result = latest_evidence(&bundle.evidence, EvidenceKind::ExecutorResponse);
    let memory_context = bundle.evidence.iter().rev().find(|item| {
        item.kind == EvidenceKind::MemoryItem && item.summary == "prompt memory context"
    });

    TraceFrame {
        trace_id: bundle.trace.trace_id.clone(),
        timestamp: bundle.trace.started_at,
        input_text: raw_user_input
            .and_then(|item| item.content.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        input_flags: raw_user_input
            .and_then(|item| item.content.get("input_flags"))
            .and_then(Value::as_array)
            .map(|flags| {
                flags
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        model_name: raw_model_output
            .and_then(|item| item.content.get("model"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        model_params: raw_model_output
            .and_then(|item| item.content.get("model_params"))
            .cloned(),
        runtime_profile: bundle.trace.profile.clone(),
        memory_snapshot_summary: memory_context.map(|item| item.content.clone()),
        prompt_hash: raw_model_output
            .and_then(|item| item.content.get("prompt_hash"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        raw_model_output: raw_model_output
            .and_then(|item| item.content.get("raw_output"))
            .cloned(),
        output_governor: raw_model_output
            .and_then(|item| item.content.get("output_governor"))
            .cloned(),
        cleaned_json: parsed_json.map(|item| item.content.clone()),
        schema_result: if parsed_json.is_some() {
            "passed".to_owned()
        } else {
            "missing".to_owned()
        },
        normalized_command: normalized_command.map(|item| item.content.clone()),
        device_resolution: gate_check_value(&bundle.gate_checks, "DeviceResolvedGate"),
        capability_result: gate_check_value(&bundle.gate_checks, "CapabilityGate"),
        execution_plan: dry_run_plan.map(|item| item.content.clone()),
        executor_result: executor_result.map(|item| item.content.clone()),
        failure_reason: trace_failure_reason(&bundle.steps, &bundle.gate_checks),
        latency_ms: trace_latency_ms(&bundle.trace, &bundle.steps),
        memory_pressure: None,
        retry_count: Some(0),
        step_count: bundle.steps.len(),
        gate_count: bundle.gate_checks.len(),
        audit_count: bundle.audit_events.len(),
    }
}

fn latest_evidence(evidence: &[EvidenceRef], kind: EvidenceKind) -> Option<&EvidenceRef> {
    evidence.iter().rev().find(|item| item.kind == kind)
}

fn gate_check_value(gate_checks: &[GateCheck], gate_name: &str) -> Option<Value> {
    gate_checks
        .iter()
        .rev()
        .find(|check| check.gate_name == gate_name)
        .map(|check| {
            json!({
                "gate_name": check.gate_name,
                "outcome": check.outcome,
                "reason": check.reason,
            })
        })
}

fn trace_failure_reason(steps: &[CommandStep], gate_checks: &[GateCheck]) -> Option<String> {
    gate_checks
        .iter()
        .find(|check| check.outcome == GateOutcome::Rejected)
        .map(|check| format!("{}: {}", check.gate_name, check.reason))
        .or_else(|| {
            steps
                .iter()
                .find(|step| matches!(step.status, StepStatus::Rejected | StepStatus::Failed))
                .and_then(|step| step.message.clone().or_else(|| Some(step.name.clone())))
        })
}

fn trace_latency_ms(trace: &CommandTrace, steps: &[CommandStep]) -> Option<i128> {
    let last_step = steps.last()?;
    Some((last_step.created_at - trace.started_at).whole_milliseconds())
}

struct CandidateRun {
    candidate: ModelCandidate,
    raw_output: String,
    model_name: String,
    output_governor_report: Option<OutputGovernorReport>,
    model_params: Value,
    prompt_hash: Option<String>,
}

fn generate_candidate(
    profile: &RuntimeProfile,
    input: &UserInput,
    prompt_context: &PromptContext,
    use_mock: bool,
) -> anyhow::Result<CandidateRun> {
    if use_mock {
        let candidate = mock_model_candidate(input);
        return Ok(CandidateRun {
            raw_output: serde_json::to_string(&candidate)?,
            candidate,
            model_name: "MockModel".to_owned(),
            output_governor_report: None,
            model_params: json!({ "mock": true }),
            prompt_hash: Some(stable_prompt_hash(&input.text)),
        });
    }

    let model_profile = MiniCpm5Profile::from_runtime_profile(profile);
    let system = system_prompt(prompt_context);
    let user = input.text.clone();
    let prompt_hash = stable_prompt_hash(&format!("{system}\n{user}"));
    let request = StructuredOutputRequest::new(
        &model_profile,
        vec![ChatMessage::system(system), ChatMessage::user(user)],
    );
    let response = OllamaClient::new(&profile.ollama_base_url)
        .with_timeout_ms(profile.timeout_ms)
        .chat_structured(&request)
        .with_context(|| format!("failed to call Ollama at `{}`", profile.ollama_base_url))?;
    let governor = OutputGovernor::from_profile(&model_profile);
    let (output_governor_report, governed_result) =
        governor.try_govern_with_report(&response.raw_content);
    let model_params = serde_json::to_value(model_profile.options())?;
    let candidate = governed_result.map_err(|error| ModelGenerationFailure {
        message: format!("Ollama output governor rejected model response: {error}"),
        model_name: Some(response.model.clone()),
        raw_output: Some(response.raw_content.clone()),
        output_governor_report: Some(output_governor_report.clone()),
        model_params: Some(model_params.clone()),
        prompt_hash: Some(prompt_hash.clone()),
    })?;

    Ok(CandidateRun {
        candidate,
        raw_output: response.raw_content,
        model_name: response.model,
        output_governor_report: Some(output_governor_report),
        model_params,
        prompt_hash: Some(prompt_hash),
    })
}

fn system_prompt(prompt_context: &PromptContext) -> String {
    let mut prompt = String::from(
        r#"Output only one JSON object. No prose.
schema_version must be exactly "model_output.v1".
For smart-home control, intent="control_device"; do not use unknown if the command is clear.
Rooms: 客厅=living_room, 卧室/主卧=bedroom, 走廊/玄关=hallway, 厨房=kitchen, 浴室/卫生间=bathroom, 入户门/前门=entrance.
Devices: 灯/灯带/主灯=light, 空调=air_conditioner, 窗帘=curtain, 摄像头=camera, 门锁=lock, 燃气报警器=gas_device.
Actions: 打开/开启=turn_on, 关闭/关掉=turn_off, 调到N%=set_brightness, 调亮=increase_brightness, 调暗=decrease_brightness, 调到N度=set_temperature, 制冷/制热/除湿=set_mode.
Use open/close only for curtain or lock, never for lights.
Keep device_alias as the user phrase. Never output backend IDs, entity_id, MQTT topic, URL, token, MIoT ID, or Matter ID.
Template: {"schema_version":"model_output.v1","intent":"control_device","room":"hallway","device_alias":"走廊灯","device_type":"light","action":"turn_on","params":{"brightness":null,"temperature":null,"mode":null,"time_after":null,"raw_value":null}}"#,
    );
    if !prompt_context.text.is_empty() {
        prompt.push_str("\nMemory context:\n");
        prompt.push_str(&prompt_context.text);
    }
    prompt
}

fn stable_prompt_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn handle_explicit_memory_write(
    trace_store: &TraceStore,
    audit_sink: &AuditSink,
    trace_id: &TraceId,
    input: &UserInput,
    registry: &DeviceRegistry,
    long_memory_store: &LongTermPreferenceStore,
) -> anyhow::Result<Option<Value>> {
    match ExplicitMemoryWriteDetector::detect(&input.text) {
        ExplicitMemoryWriteDetection::None => Ok(None),
        ExplicitMemoryWriteDetection::Rejected { reason } => {
            memory_write_rejected(trace_store, audit_sink, trace_id, &input.text, reason).map(Some)
        }
        ExplicitMemoryWriteDetection::DeviceAlias {
            target_alias,
            new_alias,
        } => {
            let Ok(target) = registry.resolve_alias(&target_alias) else {
                return memory_write_rejected(
                    trace_store,
                    audit_sink,
                    trace_id,
                    &input.text,
                    format!("unknown target alias `{target_alias}`"),
                )
                .map(Some);
            };

            let request_ref = trace_store.record_evidence(NewEvidence::new(
                EvidenceKind::MemoryWriteRequest,
                SourceSystem::User,
                "explicit device alias memory write",
                json!({
                    "input": input.text.as_str(),
                    "target_alias": target_alias.as_str(),
                    "new_alias": new_alias.as_str(),
                    "target_device_id": target.device_id.0.as_str(),
                }),
            ))?;

            let item = NewMemoryItem::new(
                MemoryScope::Device,
                MemoryKind::DeviceAlias,
                new_alias.clone(),
                json!({
                    "device_id": target.device_id.0.as_str(),
                    "target_alias": target_alias.as_str(),
                    "room": target.room.clone(),
                    "device_type": target.device_type.clone(),
                }),
                request_ref.id.clone(),
            );
            let saved = long_memory_store.put_confirmed(MemoryWriteRequest {
                item,
                user_confirmed: true,
            })?;
            let saved_ref = trace_store.record_evidence(NewEvidence::new(
                EvidenceKind::MemoryItem,
                SourceSystem::Memory,
                "confirmed long-term memory item",
                serde_json::to_value(&saved)?,
            ))?;
            trace_store.append_step(
                trace_id,
                NewCommandStep::new("long_memory_write", StepStatus::Succeeded)
                    .with_evidence_refs(vec![request_ref.id.clone(), saved_ref.id.clone()]),
            )?;
            audit_sink.append(
                NewAuditEvent::new(
                    "long_memory_write_completed",
                    "explicit user memory write persisted",
                    json!({
                        "trace_id": trace_id.0.as_str(),
                        "memory_id": saved.id.as_str(),
                        "kind": saved.kind,
                        "key": saved.key.as_str(),
                        "executable": false,
                    }),
                )
                .with_trace_id(trace_id.clone()),
            )?;

            Ok(Some(json!({
                "trace_id": trace_id,
                "mode": "memory_write",
                "memory_write": {
                    "status": "saved",
                    "item": saved,
                },
                "executable": false,
                "execute_enabled": false,
                "note": "explicit long-term memory write handled by Rust harness; no device execution was attempted"
            })))
        }
    }
}

fn memory_write_rejected(
    trace_store: &TraceStore,
    audit_sink: &AuditSink,
    trace_id: &TraceId,
    input_text: &str,
    reason: String,
) -> anyhow::Result<Value> {
    let rejected_ref = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::MemoryWriteRequest,
        SourceSystem::User,
        "rejected explicit memory write",
        json!({
            "input": input_text,
            "reason": reason,
        }),
    ))?;
    trace_store.append_step(
        trace_id,
        NewCommandStep::new("long_memory_write_rejected", StepStatus::Rejected)
            .with_message(reason.clone())
            .with_evidence_refs(vec![rejected_ref.id.clone()]),
    )?;
    audit_sink.append(
        NewAuditEvent::new(
            "long_memory_write_rejected",
            "explicit user memory write rejected",
            json!({
                "trace_id": trace_id.0.as_str(),
                "reason": reason.as_str(),
                "executable": false,
            }),
        )
        .with_trace_id(trace_id.clone()),
    )?;

    Ok(json!({
        "trace_id": trace_id,
        "mode": "memory_write",
        "memory_write": {
            "status": "rejected",
            "reason": reason,
        },
        "executable": false,
        "execute_enabled": false,
        "note": "long-term memory writes must be explicit, resolvable, and safety-preserving"
    }))
}

fn resolve_device_target(
    command: &NormalizedCommand,
    candidate: &ModelCandidate,
    long_items: &[MemoryItem],
    registry: &DeviceRegistry,
) -> anyhow::Result<Option<(NormalizedCommand, String)>> {
    if command.device_id.is_some() {
        return Ok(None);
    }

    if let Some(alias) = candidate.device_alias.as_deref()
        && !alias.starts_with("relative:")
        && let Some(item) = long_items
            .iter()
            .find(|item| item.kind == MemoryKind::DeviceAlias && item.key == alias)
        && let Some(device_id) = item.value.get("device_id").and_then(Value::as_str)
    {
        let device_id = DeviceId::new(device_id)?;
        let device = registry.get_device(&device_id)?;
        if command.device_type != DeviceType::Unknown && command.device_type != device.device_type {
            return Ok(None);
        }
        let resolved = command_for_device(command, device);
        return Ok(Some((resolved, format!("long_memory:{}", item.id))));
    }

    if let Ok(resolution) = registry.device_resolver().resolve(DeviceResolutionInput {
        candidate_alias: candidate.device_alias.as_deref(),
        room: &command.room,
        device_type: &command.device_type,
    }) {
        let resolved = command_for_device(command, resolution.device);
        return Ok(Some((
            resolved,
            resolution_source_label(resolution.source).to_owned(),
        )));
    }

    Ok(None)
}

fn resolution_source_label(source: DeviceResolutionSource) -> &'static str {
    match source {
        DeviceResolutionSource::Alias => "device_registry_alias",
        DeviceResolutionSource::RoomTypeUniqueMatch => "device_registry_room_type",
    }
}

fn command_for_device(
    command: &NormalizedCommand,
    device: &edgehome_registry::DeviceRecord,
) -> NormalizedCommand {
    let mut resolved = command.clone();
    resolved.room = device.room.clone();
    resolved.device_id = Some(device.device_id.clone());
    resolved.device_type = device.device_type.clone();
    resolved.risk = device.risk_level.clone();
    resolved
}

fn load_short_session_memory(
    trace_store: &TraceStore,
    max_turns: usize,
) -> anyhow::Result<ShortSessionMemory> {
    let mut memory = ShortSessionMemory::new(max_turns);
    let rows = trace_store.connection().query_all(
        "SELECT content_json
         FROM evidence_refs
         WHERE kind = 'normalized_command'
         ORDER BY created_at DESC
         LIMIT ?1",
        &[integer(max_turns as i64)],
    )?;

    for row in rows.into_iter().rev() {
        let content_json = row.text(0)?;
        let command: NormalizedCommand = serde_json::from_str(&content_json)?;
        memory.append("previous command", command, None);
    }

    Ok(memory)
}

fn mock_model_candidate(input: &UserInput) -> ModelCandidate {
    RulePreParser.pre_parse(input).unwrap_or_default()
}

fn repair_candidate_with_rules(
    input: &UserInput,
    candidate: &ModelCandidate,
) -> Option<ModelCandidate> {
    let rule_candidate = RulePreParser.pre_parse(input)?;
    let mut repaired = candidate.clone();

    if !rule_candidate.intent.is_unknown() {
        repaired.intent = rule_candidate.intent;
    }
    if rule_candidate.room.is_some() {
        repaired.room = rule_candidate.room;
    }
    if rule_candidate.device_alias.is_some() {
        repaired.device_alias = rule_candidate.device_alias;
    }
    if !rule_candidate.device_type.is_unknown() {
        repaired.device_type = rule_candidate.device_type;
    }
    if !rule_candidate.action.is_unknown() {
        repaired.action = rule_candidate.action;
    }
    repaired.params = rule_candidate.params;

    (repaired != *candidate).then_some(repaired)
}

fn normalize_model_candidate(candidate: &ModelCandidate) -> anyhow::Result<NormalizedCommand> {
    Ok(SemanticNormalizer.normalize(candidate)?)
}

fn input_flag_label(flag: &InputFlag) -> &'static str {
    match flag {
        InputFlag::PromptInjectionLike => "prompt_injection_like",
        InputFlag::DangerousDirectBackendAccess => "dangerous_direct_backend_access",
    }
}

impl PipelineMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::DryRun => "dry_run",
        }
    }
}

fn ensure_db_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create db parent `{}`", parent.display()))?;
    }
    Ok(())
}

fn backend_config_path(
    config_dir: &Path,
    backend_config: Option<&Path>,
    default_relative_path: &str,
) -> PathBuf {
    backend_config
        .map(Path::to_path_buf)
        .unwrap_or_else(|| config_dir.join(default_relative_path))
}

fn print_json(value: &impl Serialize) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{Action, CommandParams, CommandSchemaVersion, Intent, RiskLevel, Room};

    fn workspace_config_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
    }

    fn load_registry() -> DeviceRegistry {
        DeviceRegistry::load_from_path(workspace_config_dir().join("devices.yaml"))
            .expect("registry")
    }

    fn unresolved_light_command() -> NormalizedCommand {
        NormalizedCommand {
            schema_version: CommandSchemaVersion::default(),
            intent: Intent::ControlDevice,
            room: Room::LivingRoom,
            device_id: None,
            device_type: DeviceType::Light,
            action: Action::TurnOff,
            params: CommandParams::default(),
            risk: RiskLevel::Low,
        }
    }

    fn light_candidate_without_alias() -> ModelCandidate {
        ModelCandidate {
            intent: Intent::ControlDevice,
            room: Some(Room::LivingRoom),
            device_alias: None,
            device_type: DeviceType::Light,
            action: Action::TurnOff,
            params: CommandParams::default(),
            ..ModelCandidate::default()
        }
    }

    #[test]
    fn device_target_resolution_uses_registry_without_memory_items() {
        let registry = load_registry();
        let (resolved, source) = resolve_device_target(
            &unresolved_light_command(),
            &light_candidate_without_alias(),
            &[],
            &registry,
        )
        .expect("resolution result")
        .expect("resolved by registry");

        assert_eq!(source, "device_registry_room_type");
        assert_eq!(
            resolved.device_id,
            Some(DeviceId::new("living_room_main_light").expect("device id"))
        );
        assert_eq!(resolved.risk, RiskLevel::Low);
        assert!(resolved.can_enter_policy_gate());
    }

    #[test]
    fn system_prompt_pins_candidate_contract_and_enums() {
        let prompt = system_prompt(&PromptContext {
            text: String::new(),
            short_turns_used: 0,
            long_items_used: 0,
            evidence_refs: Vec::new(),
            truncated: false,
            budget_chars: 500,
        });

        assert!(prompt.contains("schema_version must be exactly"));
        assert!(prompt.contains("intent=\"control_device\""));
        assert!(prompt.contains("灯/灯带/主灯=light"));
        assert!(prompt.contains("打开/开启=turn_on"));
        assert!(prompt.contains("Never output backend IDs"));
        assert!(prompt.contains("\"schema_version\":\"model_output.v1\""));
    }

    #[test]
    fn rule_repair_overrides_missing_or_conflicting_model_slots() {
        let candidate = ModelCandidate {
            intent: Intent::Unknown,
            room: Some(Room::Hallway),
            device_alias: None,
            device_type: DeviceType::Unknown,
            action: Action::Open,
            params: CommandParams::default(),
            ..ModelCandidate::default()
        };
        let input = UserInput::new("把客厅灯关掉").expect("input");

        let repaired = repair_candidate_with_rules(&input, &candidate).expect("repaired");

        assert_eq!(repaired.intent, Intent::ControlDevice);
        assert_eq!(repaired.room, Some(Room::LivingRoom));
        assert_eq!(repaired.device_alias.as_deref(), Some("客厅灯"));
        assert_eq!(repaired.device_type, DeviceType::Light);
        assert_eq!(repaired.action, Action::TurnOff);
    }
}
