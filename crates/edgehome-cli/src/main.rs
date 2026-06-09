use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use edgehome_config::{RuntimeProfile, load_profile};
use edgehome_core::{
    Action, CommandParams, DeviceId, DeviceType, Intent, ModelCandidate, NormalizedCommand,
    RiskLevel, Room, UserInput,
};
use edgehome_storage::{EvidenceKind, EvidenceStore, NewEvidence, SourceSystem};
use edgehome_trace::{
    AuditSink, GateOutcome, NewAuditEvent, NewCommandStep, NewGateCheck, StepStatus, TraceId,
    TraceStore,
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
            let output = run_mock_pipeline(&cli.db_path, &profile, input, MockMode::Parse)?;
            print_json(&output)?;
        }
        Commands::DryRun { mock, input } => {
            require_mock(mock)?;
            let profile = load_profile(&cli.config_dir, &cli.profile)
                .with_context(|| format!("failed to load profile `{}`", cli.profile))?;
            let output = run_mock_pipeline(&cli.db_path, &profile, input, MockMode::DryRun)?;
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

    let candidate = mock_model_candidate(&input.text);
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

    if normalized.can_enter_policy_gate() {
        trace_store.append_gate_check(
            &trace.trace_id,
            NewGateCheck::new(
                "policy_engine_pending",
                GateOutcome::Warning,
                "M4 mock pipeline stops before M7 policy engine; command is not executable",
            )
            .with_evidence_refs(vec![normalized_ref.id.clone()]),
        )?;
    } else {
        trace_store.append_gate_check(
            &trace.trace_id,
            NewGateCheck::new(
                "normalization_gate",
                GateOutcome::Rejected,
                "normalized command cannot enter policy gate",
            )
            .with_evidence_refs(vec![normalized_ref.id.clone()]),
        )?;
    }

    if mode == MockMode::DryRun {
        trace_store.append_step(
            &trace.trace_id,
            NewCommandStep::new("dry_run_blocked_until_policy", StepStatus::Fallback)
                .with_message("M4 records a traceable mock dry-run request but does not execute")
                .with_evidence_refs(vec![normalized_ref.id.clone()]),
        )?;
    }

    let audit_event_type = match mode {
        MockMode::Parse => "mock_parse_completed",
        MockMode::DryRun => "mock_dry_run_recorded",
    };
    audit_sink.append(
        NewAuditEvent::new(
            audit_event_type,
            "mock pipeline completed with trace",
            json!({
                "mode": mode.as_str(),
                "trace_id": trace.trace_id.0.as_str(),
                "policy_status": "pending_m7_policy_engine",
                "executable": false,
            }),
        )
        .with_trace_id(trace.trace_id.clone()),
    )?;

    Ok(json!({
        "trace_id": trace.trace_id,
        "mode": mode.as_str(),
        "mock": true,
        "model_candidate": candidate,
        "normalized_command": normalized,
        "evidence_refs": {
            "raw_user_input": raw_user_input.id,
            "raw_model_output": raw_model_output.id,
            "parsed_json": parsed_json_ref.id,
            "normalized_command": normalized_ref.id,
        },
        "policy_status": "pending_m7_policy_engine",
        "execution_plan": null,
        "executable": false,
        "note": "M4 mock pipeline records evidence/trace/audit only; real policy and dry-run planner arrive in M7/M8"
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

fn mock_model_candidate(input: &str) -> ModelCandidate {
    match input.trim() {
        "把客厅灯关掉" => ModelCandidate {
            intent: Intent::ControlDevice,
            room: Some(Room::LivingRoom),
            device_alias: Some("客厅灯".to_owned()),
            device_type: DeviceType::Light,
            action: Action::TurnOff,
            ..ModelCandidate::default()
        },
        "晚上十点后把走廊灯调到30%" | "晚上十点后把走廊灯调到 30%" => {
            ModelCandidate {
                intent: Intent::ControlDevice,
                room: Some(Room::Hallway),
                device_alias: Some("走廊灯".to_owned()),
                device_type: DeviceType::Light,
                action: Action::SetBrightness,
                params: CommandParams {
                    brightness: Some(30),
                    time_after: Some("22:00".to_owned()),
                    ..CommandParams::default()
                },
                ..ModelCandidate::default()
            }
        }
        "把卧室空调调到26度" | "把卧室空调调到 26 度" => ModelCandidate {
            intent: Intent::ControlDevice,
            room: Some(Room::Bedroom),
            device_alias: Some("卧室空调".to_owned()),
            device_type: DeviceType::AirConditioner,
            action: Action::SetTemperature,
            params: CommandParams {
                temperature: Some(26),
                ..CommandParams::default()
            },
            ..ModelCandidate::default()
        },
        _ => ModelCandidate::default(),
    }
}

fn normalize_mock_candidate(candidate: &ModelCandidate) -> anyhow::Result<NormalizedCommand> {
    let room = candidate.room.clone().unwrap_or_default();
    let device_id = match (&room, &candidate.device_type, &candidate.action) {
        (Room::LivingRoom, DeviceType::Light, Action::TurnOff | Action::TurnOn) => {
            Some(DeviceId::new("living_room_main_light")?)
        }
        (Room::Hallway, DeviceType::Light, Action::SetBrightness) => {
            Some(DeviceId::new("hallway_light")?)
        }
        (Room::Bedroom, DeviceType::AirConditioner, Action::SetTemperature) => {
            Some(DeviceId::new("bedroom_air_conditioner")?)
        }
        _ => None,
    };
    let risk = match candidate.device_type {
        DeviceType::Light => RiskLevel::Low,
        DeviceType::AirConditioner => RiskLevel::Medium,
        DeviceType::Lock | DeviceType::GasDevice | DeviceType::Camera => RiskLevel::High,
        DeviceType::Unknown => RiskLevel::Unknown,
        _ => RiskLevel::Medium,
    };

    Ok(NormalizedCommand {
        intent: candidate.intent.clone(),
        room,
        device_id,
        device_type: candidate.device_type.clone(),
        action: candidate.action.clone(),
        params: candidate.params.clone(),
        risk,
        ..NormalizedCommand::default()
    })
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
