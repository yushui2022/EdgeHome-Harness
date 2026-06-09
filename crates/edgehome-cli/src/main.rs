use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use edgehome_config::{RuntimeProfile, load_profile};
use edgehome_core::{ModelCandidate, NormalizedCommand, PolicyDecision, UserInput};
use edgehome_executor::DryRunPlanner;
use edgehome_gate::{GateEngine, GateEvaluationRequest};
use edgehome_parser::{RulePreParser, SemanticNormalizer};
use edgehome_registry::{DeviceRegistry, StateFreshness};
use edgehome_storage::{EvidenceKind, EvidenceStore, NewEvidence, SourceSystem};
use edgehome_trace::{AuditSink, NewAuditEvent, NewCommandStep, StepStatus, TraceId, TraceStore};
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
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
}

#[derive(Debug, Subcommand)]
enum TraceCommand {
    Show { trace_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockMode {
    Parse,
    DryRun,
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
        Commands::Parse { mock, input } => {
            require_mock(mock)?;
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let output = run_mock_pipeline(
                &cli.db_path,
                &cli.config_dir,
                &profile,
                input,
                MockMode::Parse,
            )?;
            print_json(&output)?;
        }
        Commands::DryRun { mock, input } => {
            require_mock(mock)?;
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let output = run_mock_pipeline(
                &cli.db_path,
                &cli.config_dir,
                &profile,
                input,
                MockMode::DryRun,
            )?;
            print_json(&output)?;
        }
        Commands::Trace {
            command: TraceCommand::Show { trace_id },
        } => {
            let output = show_trace(&cli.db_path, TraceId(trace_id))?;
            print_json(&output)?;
        }
    }

    Ok(())
}

fn require_mock(mock: bool) -> anyhow::Result<()> {
    if mock {
        Ok(())
    } else {
        bail!("M4 only supports --mock; Ollama integration is planned for M10")
    }
}

fn run_mock_pipeline(
    db_path: &Path,
    config_dir: &Path,
    profile: &RuntimeProfile,
    input: String,
    mode: MockMode,
) -> anyhow::Result<Value> {
    ensure_db_parent(db_path)?;

    let trace_store = TraceStore::open(db_path)?;
    let audit_sink = AuditSink::open(db_path)?;
    let input = UserInput::new(input)?;

    let raw_user_input = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::RawUserInput,
        SourceSystem::User,
        "raw user input",
        json!({ "text": input.text.as_str() }),
    ))?;

    let trace = trace_store.start_trace(raw_user_input.id.clone(), profile.name.to_string())?;
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("input_received", StepStatus::Succeeded)
            .with_evidence_refs(vec![raw_user_input.id.clone()]),
    )?;

    let candidate = mock_model_candidate(&input);
    let raw_model_output_text = serde_json::to_string(&candidate)?;
    let raw_model_output = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::RawModelOutput,
        SourceSystem::Model,
        "mock model output",
        json!({
            "model": "MockModel",
            "raw_output": raw_model_output_text,
        }),
    ))?;
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("mock_model_output", StepStatus::Succeeded)
            .with_evidence_refs(vec![raw_model_output.id.clone()]),
    )?;

    let parsed_json = serde_json::to_value(&candidate)?;
    let parsed_json_ref = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::ParsedJson,
        SourceSystem::Parser,
        "parsed mock model JSON",
        parsed_json.clone(),
    ))?;
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("parse_json", StepStatus::Succeeded)
            .with_evidence_refs(vec![parsed_json_ref.id.clone()]),
    )?;

    let normalized = normalize_mock_candidate(&candidate)?;
    let normalized_ref = trace_store.record_evidence(NewEvidence::new(
        EvidenceKind::NormalizedCommand,
        SourceSystem::Normalizer,
        "normalized mock command",
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

    let registry_path = config_dir.join("devices.yaml");
    let registry = DeviceRegistry::load_from_path(&registry_path).with_context(|| {
        format!(
            "failed to load device registry `{}`",
            registry_path.display()
        )
    })?;
    let gate_engine = GateEngine::new(&trace_store, &registry);
    let gate_evaluation = gate_engine.evaluate(
        GateEvaluationRequest::new(trace.trace_id.clone(), normalized.clone())
            .with_evidence_refs(vec![
                raw_user_input.id.clone(),
                raw_model_output.id.clone(),
                parsed_json_ref.id.clone(),
                normalized_ref.id.clone(),
            ])
            .with_state_freshness(StateFreshness::Fresh)
            .with_dry_run_ready(mode == MockMode::DryRun),
    )?;

    let gate_status = if gate_evaluation.policy_decision == PolicyDecision::Deny {
        StepStatus::Rejected
    } else {
        StepStatus::Succeeded
    };
    trace_store.append_step(
        &trace.trace_id,
        NewCommandStep::new("gate_engine", gate_status).with_evidence_refs(vec![
            raw_user_input.id.clone(),
            raw_model_output.id.clone(),
            parsed_json_ref.id.clone(),
            normalized_ref.id.clone(),
        ]),
    )?;

    let mut dry_run_plan = None;
    if mode == MockMode::DryRun {
        if gate_evaluation.policy_decision == PolicyDecision::Deny
            || !gate_evaluation.blocking_reasons.is_empty()
        {
            trace_store.append_step(
                &trace.trace_id,
                NewCommandStep::new("dry_run_rejected_by_gate", StepStatus::Rejected)
                    .with_message("dry-run planner only accepts non-denied gated commands")
                    .with_evidence_refs(vec![normalized_ref.id.clone()]),
            )?;
        } else if let Some(device_id) = normalized.device_id.as_ref() {
            let device = registry.get_device(device_id)?;
            let planned = DryRunPlanner.plan(
                &trace.trace_id,
                &normalized,
                device,
                gate_evaluation.policy_decision.clone(),
            )?;
            let dry_run_ref = trace_store.record_evidence(NewEvidence::new(
                EvidenceKind::DryRunPlan,
                SourceSystem::Executor,
                "mock dry-run plan",
                serde_json::to_value(&planned)?,
            ))?;
            trace_store.append_step(
                &trace.trace_id,
                NewCommandStep::new("dry_run_plan", StepStatus::Succeeded)
                    .with_evidence_refs(vec![normalized_ref.id.clone(), dry_run_ref.id.clone()]),
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
    }

    let audit_event_type = match mode {
        MockMode::Parse => "mock_parse_completed",
        MockMode::DryRun if dry_run_plan.is_some() => "mock_dry_run_plan_generated",
        MockMode::DryRun => "mock_dry_run_rejected",
    };
    let policy_decision = gate_evaluation.policy_decision.clone();
    let dry_run_ready = dry_run_plan.is_some();
    audit_sink.append(
        NewAuditEvent::new(
            audit_event_type,
            "mock pipeline completed with gated policy decision",
            json!({
                "mode": mode.as_str(),
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
    Ok(json!({
        "trace_id": trace.trace_id,
        "mode": mode.as_str(),
        "mock": true,
        "model_candidate": candidate,
        "normalized_command": normalized,
        "gate_evaluation": gate_evaluation,
        "evidence_refs": {
            "raw_user_input": raw_user_input.id,
            "raw_model_output": raw_model_output.id,
            "parsed_json": parsed_json_ref.id,
            "normalized_command": normalized_ref.id,
        },
        "policy_decision": policy_decision,
        "dry_run_plan": dry_run_plan,
        "execution_plan": execution_plan,
        "executable": false,
        "execute_enabled": false,
        "note": "M8 mock pipeline runs gate/policy and can produce dry-run; real execute remains disabled by default"
    }))
}

fn show_trace(db_path: &Path, trace_id: TraceId) -> anyhow::Result<Value> {
    ensure_db_parent(db_path)?;

    let trace_store = TraceStore::open(db_path)?;
    let evidence_store = EvidenceStore::open(db_path)?;
    let audit_sink = AuditSink::open(db_path)?;

    let trace = trace_store.read_trace(&trace_id)?;
    let raw_user_input = evidence_store.read(&trace.raw_user_input_ref)?;
    let steps = trace_store.steps_for_trace(&trace_id)?;
    let gate_checks = trace_store.gate_checks_for_trace(&trace_id)?;
    let audit_events = audit_sink.events_for_trace(&trace_id)?;

    Ok(json!({
        "trace": trace,
        "raw_user_input": raw_user_input,
        "steps": steps,
        "gate_checks": gate_checks,
        "audit_events": audit_events,
    }))
}

fn mock_model_candidate(input: &UserInput) -> ModelCandidate {
    RulePreParser.pre_parse(input).unwrap_or_default()
}

fn normalize_mock_candidate(candidate: &ModelCandidate) -> anyhow::Result<NormalizedCommand> {
    Ok(SemanticNormalizer.normalize(candidate)?)
}

impl MockMode {
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

fn print_json(value: &impl Serialize) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
