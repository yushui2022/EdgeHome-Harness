//! Command trace, gate check, and audit storage for EdgeHome Harness.
//!
//! Trace records the decision path. Evidence records the facts. Keeping both
//! explicit lets later milestones replay why a command was allowed, rejected,
//! retried, or downgraded without asking the model to explain itself.

use std::path::Path;
use std::str::FromStr;

use edgehome_storage::{EvidenceId, EvidenceRef, EvidenceStore, NewEvidence, StorageError};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

pub type TraceResult<T> = Result<T, TraceError>;

#[derive(Debug, Error)]
pub enum TraceError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("time parse error: {0}")]
    TimeParse(#[from] time::error::Parse),

    #[error("time format error: {0}")]
    TimeFormat(#[from] time::error::Format),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("unknown step status: {0}")]
    UnknownStepStatus(String),

    #[error("unknown gate outcome: {0}")]
    UnknownGateOutcome(String),

    #[error("trace not found: {0}")]
    TraceNotFound(String),

    #[error("step not found: {0}")]
    StepNotFound(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceId(pub String);

impl TraceId {
    pub fn new() -> Self {
        Self(format!("tr_{}", Uuid::new_v4().simple()))
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepId(pub String);

impl StepId {
    pub fn new() -> Self {
        Self(format!("st_{}", Uuid::new_v4().simple()))
    }
}

impl Default for StepId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GateCheckId(pub String);

impl GateCheckId {
    pub fn new() -> Self {
        Self(format!("gc_{}", Uuid::new_v4().simple()))
    }
}

impl Default for GateCheckId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuditEventId(pub String);

impl AuditEventId {
    pub fn new() -> Self {
        Self(format!("au_{}", Uuid::new_v4().simple()))
    }
}

impl Default for AuditEventId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Started,
    Succeeded,
    Rejected,
    Failed,
    Fallback,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Succeeded => "succeeded",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Fallback => "fallback",
        }
    }
}

impl FromStr for StepStatus {
    type Err = TraceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "started" => Ok(Self::Started),
            "succeeded" => Ok(Self::Succeeded),
            "rejected" => Ok(Self::Rejected),
            "failed" => Ok(Self::Failed),
            "fallback" => Ok(Self::Fallback),
            other => Err(TraceError::UnknownStepStatus(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    Accepted,
    Rejected,
    Warning,
}

impl GateOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Warning => "warning",
        }
    }
}

impl FromStr for GateOutcome {
    type Err = TraceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "warning" => Ok(Self::Warning),
            other => Err(TraceError::UnknownGateOutcome(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandTrace {
    pub trace_id: TraceId,
    pub raw_user_input_ref: EvidenceId,
    pub profile: String,
    pub status: StepStatus,
    pub started_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandStep {
    pub step_id: StepId,
    pub trace_id: TraceId,
    pub sequence: i64,
    pub name: String,
    pub status: StepStatus,
    pub message: Option<String>,
    pub created_at: OffsetDateTime,
    pub evidence_refs: Vec<EvidenceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewCommandStep {
    pub name: String,
    pub status: StepStatus,
    pub message: Option<String>,
    pub evidence_refs: Vec<EvidenceId>,
}

impl NewCommandStep {
    pub fn new(name: impl Into<String>, status: StepStatus) -> Self {
        Self {
            name: name.into(),
            status,
            message: None,
            evidence_refs: Vec::new(),
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn with_evidence_refs(mut self, evidence_refs: Vec<EvidenceId>) -> Self {
        self.evidence_refs = evidence_refs;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateCheck {
    pub gate_check_id: GateCheckId,
    pub trace_id: TraceId,
    pub step_id: Option<StepId>,
    pub gate_name: String,
    pub outcome: GateOutcome,
    pub reason: String,
    pub evidence_refs: Vec<EvidenceId>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewGateCheck {
    pub step_id: Option<StepId>,
    pub gate_name: String,
    pub outcome: GateOutcome,
    pub reason: String,
    pub evidence_refs: Vec<EvidenceId>,
}

impl NewGateCheck {
    pub fn new(
        gate_name: impl Into<String>,
        outcome: GateOutcome,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            step_id: None,
            gate_name: gate_name.into(),
            outcome,
            reason: reason.into(),
            evidence_refs: Vec::new(),
        }
    }

    pub fn with_step(mut self, step_id: StepId) -> Self {
        self.step_id = Some(step_id);
        self
    }

    pub fn with_evidence_refs(mut self, evidence_refs: Vec<EvidenceId>) -> Self {
        self.evidence_refs = evidence_refs;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: AuditEventId,
    pub trace_id: Option<TraceId>,
    pub event_type: String,
    pub summary: String,
    pub payload: Value,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewAuditEvent {
    pub trace_id: Option<TraceId>,
    pub event_type: String,
    pub summary: String,
    pub payload: Value,
}

impl NewAuditEvent {
    pub fn new(event_type: impl Into<String>, summary: impl Into<String>, payload: Value) -> Self {
        Self {
            trace_id: None,
            event_type: event_type.into(),
            summary: summary.into(),
            payload,
        }
    }

    pub fn with_trace_id(mut self, trace_id: TraceId) -> Self {
        self.trace_id = Some(trace_id);
        self
    }
}

pub struct TraceStore {
    evidence_store: EvidenceStore,
}

impl TraceStore {
    pub fn open(path: impl AsRef<Path>) -> TraceResult<Self> {
        let evidence_store = EvidenceStore::open(path)?;
        let store = Self { evidence_store };
        migrate_trace_schema(store.connection())?;
        Ok(store)
    }

    pub fn in_memory() -> TraceResult<Self> {
        let evidence_store = EvidenceStore::in_memory()?;
        let store = Self { evidence_store };
        migrate_trace_schema(store.connection())?;
        Ok(store)
    }

    pub fn from_evidence_store(evidence_store: EvidenceStore) -> TraceResult<Self> {
        let store = Self { evidence_store };
        migrate_trace_schema(store.connection())?;
        Ok(store)
    }

    pub fn connection(&self) -> &Connection {
        self.evidence_store.connection()
    }

    pub fn record_evidence(&self, evidence: NewEvidence) -> TraceResult<EvidenceRef> {
        Ok(self.evidence_store.record(evidence)?)
    }

    pub fn start_trace(
        &self,
        raw_user_input_ref: EvidenceId,
        profile: impl Into<String>,
    ) -> TraceResult<CommandTrace> {
        let trace = CommandTrace {
            trace_id: TraceId::new(),
            raw_user_input_ref,
            profile: profile.into(),
            status: StepStatus::Started,
            started_at: OffsetDateTime::now_utc(),
            finished_at: None,
        };

        self.connection().execute(
            "INSERT INTO command_traces (
                trace_id,
                raw_user_input_ref,
                profile,
                status,
                started_at,
                finished_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                trace.trace_id.0.as_str(),
                trace.raw_user_input_ref.0.as_str(),
                trace.profile.as_str(),
                trace.status.as_str(),
                format_time(trace.started_at)?,
                format_optional_time(trace.finished_at)?,
            ],
        )?;

        self.read_trace(&trace.trace_id)
    }

    pub fn read_trace(&self, trace_id: &TraceId) -> TraceResult<CommandTrace> {
        self.connection()
            .query_row(
                "SELECT
                    trace_id,
                    raw_user_input_ref,
                    profile,
                    status,
                    started_at,
                    finished_at
                FROM command_traces
                WHERE trace_id = ?1",
                params![trace_id.0.as_str()],
                trace_row_from_row,
            )
            .optional()?
            .map(CommandTrace::try_from)
            .transpose()?
            .ok_or_else(|| TraceError::TraceNotFound(trace_id.0.clone()))
    }

    pub fn append_step(
        &self,
        trace_id: &TraceId,
        step: NewCommandStep,
    ) -> TraceResult<CommandStep> {
        self.read_trace(trace_id)?;

        let step_id = StepId::new();
        let sequence = self.next_step_sequence(trace_id)?;
        let created_at = OffsetDateTime::now_utc();
        let evidence_json = evidence_ids_to_json(&step.evidence_refs)?;

        self.connection().execute(
            "INSERT INTO command_steps (
                step_id,
                trace_id,
                sequence,
                name,
                status,
                message,
                created_at,
                evidence_refs_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                step_id.0.as_str(),
                trace_id.0.as_str(),
                sequence,
                step.name.as_str(),
                step.status.as_str(),
                step.message.as_deref(),
                format_time(created_at)?,
                evidence_json.as_str(),
            ],
        )?;

        for evidence_id in &step.evidence_refs {
            self.connection().execute(
                "INSERT INTO step_evidence_refs (step_id, evidence_id)
                 VALUES (?1, ?2)",
                params![step_id.0.as_str(), evidence_id.0.as_str()],
            )?;
        }

        self.read_step(&step_id)
    }

    pub fn read_step(&self, step_id: &StepId) -> TraceResult<CommandStep> {
        self.connection()
            .query_row(
                "SELECT
                    step_id,
                    trace_id,
                    sequence,
                    name,
                    status,
                    message,
                    created_at,
                    evidence_refs_json
                FROM command_steps
                WHERE step_id = ?1",
                params![step_id.0.as_str()],
                step_row_from_row,
            )
            .optional()?
            .map(CommandStep::try_from)
            .transpose()?
            .ok_or_else(|| TraceError::StepNotFound(step_id.0.clone()))
    }

    pub fn steps_for_trace(&self, trace_id: &TraceId) -> TraceResult<Vec<CommandStep>> {
        let mut statement = self.connection().prepare(
            "SELECT
                step_id,
                trace_id,
                sequence,
                name,
                status,
                message,
                created_at,
                evidence_refs_json
            FROM command_steps
            WHERE trace_id = ?1
            ORDER BY sequence ASC",
        )?;

        let rows = statement.query_map(params![trace_id.0.as_str()], step_row_from_row)?;
        let mut steps = Vec::new();
        for row in rows {
            steps.push(CommandStep::try_from(row?)?);
        }
        Ok(steps)
    }

    pub fn append_gate_check(
        &self,
        trace_id: &TraceId,
        check: NewGateCheck,
    ) -> TraceResult<GateCheck> {
        self.read_trace(trace_id)?;

        let gate_check = GateCheck {
            gate_check_id: GateCheckId::new(),
            trace_id: trace_id.clone(),
            step_id: check.step_id,
            gate_name: check.gate_name,
            outcome: check.outcome,
            reason: check.reason,
            evidence_refs: check.evidence_refs,
            created_at: OffsetDateTime::now_utc(),
        };
        let evidence_json = evidence_ids_to_json(&gate_check.evidence_refs)?;

        self.connection().execute(
            "INSERT INTO gate_checks (
                gate_check_id,
                trace_id,
                step_id,
                gate_name,
                outcome,
                reason,
                evidence_refs_json,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                gate_check.gate_check_id.0.as_str(),
                gate_check.trace_id.0.as_str(),
                gate_check.step_id.as_ref().map(|id| id.0.as_str()),
                gate_check.gate_name.as_str(),
                gate_check.outcome.as_str(),
                gate_check.reason.as_str(),
                evidence_json.as_str(),
                format_time(gate_check.created_at)?,
            ],
        )?;

        Ok(gate_check)
    }

    pub fn gate_checks_for_trace(&self, trace_id: &TraceId) -> TraceResult<Vec<GateCheck>> {
        let mut statement = self.connection().prepare(
            "SELECT
                gate_check_id,
                trace_id,
                step_id,
                gate_name,
                outcome,
                reason,
                evidence_refs_json,
                created_at
            FROM gate_checks
            WHERE trace_id = ?1
            ORDER BY created_at ASC",
        )?;

        let rows = statement.query_map(params![trace_id.0.as_str()], gate_check_row_from_row)?;
        let mut checks = Vec::new();
        for row in rows {
            checks.push(GateCheck::try_from(row?)?);
        }
        Ok(checks)
    }

    fn next_step_sequence(&self, trace_id: &TraceId) -> TraceResult<i64> {
        let next = self.connection().query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1
             FROM command_steps
             WHERE trace_id = ?1",
            params![trace_id.0.as_str()],
            |row| row.get(0),
        )?;
        Ok(next)
    }
}

pub struct AuditSink {
    connection: Connection,
}

impl AuditSink {
    pub fn open(path: impl AsRef<Path>) -> TraceResult<Self> {
        let connection = Connection::open(path)?;
        let sink = Self { connection };
        migrate_trace_schema(&sink.connection)?;
        Ok(sink)
    }

    pub fn in_memory() -> TraceResult<Self> {
        let connection = Connection::open_in_memory()?;
        let sink = Self { connection };
        migrate_trace_schema(&sink.connection)?;
        Ok(sink)
    }

    pub fn from_connection(connection: Connection) -> TraceResult<Self> {
        let sink = Self { connection };
        migrate_trace_schema(&sink.connection)?;
        Ok(sink)
    }

    pub fn append(&self, event: NewAuditEvent) -> TraceResult<AuditEvent> {
        let audit_event = AuditEvent {
            event_id: AuditEventId::new(),
            trace_id: event.trace_id,
            event_type: event.event_type,
            summary: event.summary,
            payload: event.payload,
            created_at: OffsetDateTime::now_utc(),
        };
        let payload_json = serde_json::to_string(&audit_event.payload)?;

        self.connection.execute(
            "INSERT INTO audit_log (
                event_id,
                trace_id,
                event_type,
                summary,
                payload_json,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                audit_event.event_id.0.as_str(),
                audit_event.trace_id.as_ref().map(|id| id.0.as_str()),
                audit_event.event_type.as_str(),
                audit_event.summary.as_str(),
                payload_json.as_str(),
                format_time(audit_event.created_at)?,
            ],
        )?;

        Ok(audit_event)
    }

    pub fn events_for_trace(&self, trace_id: &TraceId) -> TraceResult<Vec<AuditEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT
                event_id,
                trace_id,
                event_type,
                summary,
                payload_json,
                created_at
            FROM audit_log
            WHERE trace_id = ?1
            ORDER BY created_at ASC",
        )?;

        let rows = statement.query_map(params![trace_id.0.as_str()], audit_event_row_from_row)?;
        let mut events = Vec::new();
        for row in rows {
            events.push(AuditEvent::try_from(row?)?);
        }
        Ok(events)
    }
}

fn migrate_trace_schema(connection: &Connection) -> TraceResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS evidence_refs (
            evidence_id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source TEXT NOT NULL,
            summary TEXT NOT NULL,
            content_json TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            observed_at TEXT,
            expires_at TEXT,
            metadata_json TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS command_traces (
            trace_id TEXT PRIMARY KEY,
            raw_user_input_ref TEXT NOT NULL,
            profile TEXT NOT NULL,
            status TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            FOREIGN KEY(raw_user_input_ref) REFERENCES evidence_refs(evidence_id)
        );

        CREATE TABLE IF NOT EXISTS command_steps (
            step_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            sequence INTEGER NOT NULL,
            name TEXT NOT NULL,
            status TEXT NOT NULL,
            message TEXT,
            created_at TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL DEFAULT '[]',
            FOREIGN KEY(trace_id) REFERENCES command_traces(trace_id)
        );

        CREATE TABLE IF NOT EXISTS step_evidence_refs (
            step_id TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            PRIMARY KEY(step_id, evidence_id),
            FOREIGN KEY(step_id) REFERENCES command_steps(step_id),
            FOREIGN KEY(evidence_id) REFERENCES evidence_refs(evidence_id)
        );

        CREATE TABLE IF NOT EXISTS gate_checks (
            gate_check_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            step_id TEXT,
            gate_name TEXT NOT NULL,
            outcome TEXT NOT NULL,
            reason TEXT NOT NULL,
            evidence_refs_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(trace_id) REFERENCES command_traces(trace_id),
            FOREIGN KEY(step_id) REFERENCES command_steps(step_id)
        );

        CREATE TABLE IF NOT EXISTS audit_log (
            event_id TEXT PRIMARY KEY,
            trace_id TEXT,
            event_type TEXT NOT NULL,
            summary TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(trace_id) REFERENCES command_traces(trace_id)
        );

        CREATE INDEX IF NOT EXISTS idx_command_steps_trace_sequence
            ON command_steps(trace_id, sequence);

        CREATE INDEX IF NOT EXISTS idx_gate_checks_trace
            ON gate_checks(trace_id);

        CREATE INDEX IF NOT EXISTS idx_audit_log_trace
            ON audit_log(trace_id);",
    )?;
    Ok(())
}

struct TraceRow {
    trace_id: String,
    raw_user_input_ref: String,
    profile: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
}

impl TryFrom<TraceRow> for CommandTrace {
    type Error = TraceError;

    fn try_from(row: TraceRow) -> Result<Self, Self::Error> {
        Ok(Self {
            trace_id: TraceId(row.trace_id),
            raw_user_input_ref: EvidenceId(row.raw_user_input_ref),
            profile: row.profile,
            status: StepStatus::from_str(&row.status)?,
            started_at: parse_time(&row.started_at)?,
            finished_at: parse_optional_time(row.finished_at.as_deref())?,
        })
    }
}

fn trace_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TraceRow> {
    Ok(TraceRow {
        trace_id: row.get("trace_id")?,
        raw_user_input_ref: row.get("raw_user_input_ref")?,
        profile: row.get("profile")?,
        status: row.get("status")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
    })
}

struct StepRow {
    step_id: String,
    trace_id: String,
    sequence: i64,
    name: String,
    status: String,
    message: Option<String>,
    created_at: String,
    evidence_refs_json: String,
}

impl TryFrom<StepRow> for CommandStep {
    type Error = TraceError;

    fn try_from(row: StepRow) -> Result<Self, Self::Error> {
        Ok(Self {
            step_id: StepId(row.step_id),
            trace_id: TraceId(row.trace_id),
            sequence: row.sequence,
            name: row.name,
            status: StepStatus::from_str(&row.status)?,
            message: row.message,
            created_at: parse_time(&row.created_at)?,
            evidence_refs: evidence_ids_from_json(&row.evidence_refs_json)?,
        })
    }
}

fn step_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepRow> {
    Ok(StepRow {
        step_id: row.get("step_id")?,
        trace_id: row.get("trace_id")?,
        sequence: row.get("sequence")?,
        name: row.get("name")?,
        status: row.get("status")?,
        message: row.get("message")?,
        created_at: row.get("created_at")?,
        evidence_refs_json: row.get("evidence_refs_json")?,
    })
}

struct GateCheckRow {
    gate_check_id: String,
    trace_id: String,
    step_id: Option<String>,
    gate_name: String,
    outcome: String,
    reason: String,
    evidence_refs_json: String,
    created_at: String,
}

impl TryFrom<GateCheckRow> for GateCheck {
    type Error = TraceError;

    fn try_from(row: GateCheckRow) -> Result<Self, Self::Error> {
        Ok(Self {
            gate_check_id: GateCheckId(row.gate_check_id),
            trace_id: TraceId(row.trace_id),
            step_id: row.step_id.map(StepId),
            gate_name: row.gate_name,
            outcome: GateOutcome::from_str(&row.outcome)?,
            reason: row.reason,
            evidence_refs: evidence_ids_from_json(&row.evidence_refs_json)?,
            created_at: parse_time(&row.created_at)?,
        })
    }
}

fn gate_check_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GateCheckRow> {
    Ok(GateCheckRow {
        gate_check_id: row.get("gate_check_id")?,
        trace_id: row.get("trace_id")?,
        step_id: row.get("step_id")?,
        gate_name: row.get("gate_name")?,
        outcome: row.get("outcome")?,
        reason: row.get("reason")?,
        evidence_refs_json: row.get("evidence_refs_json")?,
        created_at: row.get("created_at")?,
    })
}

struct AuditEventRow {
    event_id: String,
    trace_id: Option<String>,
    event_type: String,
    summary: String,
    payload_json: String,
    created_at: String,
}

impl TryFrom<AuditEventRow> for AuditEvent {
    type Error = TraceError;

    fn try_from(row: AuditEventRow) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: AuditEventId(row.event_id),
            trace_id: row.trace_id.map(TraceId),
            event_type: row.event_type,
            summary: row.summary,
            payload: serde_json::from_str(&row.payload_json)?,
            created_at: parse_time(&row.created_at)?,
        })
    }
}

fn audit_event_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEventRow> {
    Ok(AuditEventRow {
        event_id: row.get("event_id")?,
        trace_id: row.get("trace_id")?,
        event_type: row.get("event_type")?,
        summary: row.get("summary")?,
        payload_json: row.get("payload_json")?,
        created_at: row.get("created_at")?,
    })
}

fn evidence_ids_to_json(evidence_refs: &[EvidenceId]) -> TraceResult<String> {
    Ok(serde_json::to_string(evidence_refs)?)
}

fn evidence_ids_from_json(value: &str) -> TraceResult<Vec<EvidenceId>> {
    Ok(serde_json::from_str(value)?)
}

fn format_time(value: OffsetDateTime) -> TraceResult<String> {
    Ok(value.format(&Rfc3339)?)
}

fn format_optional_time(value: Option<OffsetDateTime>) -> TraceResult<Option<String>> {
    value.map(format_time).transpose()
}

fn parse_time(value: &str) -> TraceResult<OffsetDateTime> {
    Ok(OffsetDateTime::parse(value, &Rfc3339)?)
}

fn parse_optional_time(value: Option<&str>) -> TraceResult<Option<OffsetDateTime>> {
    value.map(parse_time).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_storage::{EvidenceKind, SourceSystem};
    use serde_json::json;

    #[test]
    fn mock_dry_run_trace_links_raw_user_input_and_step_evidence() {
        let store = TraceStore::in_memory().expect("trace store");
        let raw_user_input = store
            .record_evidence(NewEvidence::new(
                EvidenceKind::RawUserInput,
                SourceSystem::User,
                "raw user input",
                json!({ "text": "晚上十点后把走廊灯调到30%" }),
            ))
            .expect("raw input evidence");

        let trace = store
            .start_trace(raw_user_input.id.clone(), "low_memory")
            .expect("start trace");

        let normalized = store
            .record_evidence(NewEvidence::new(
                EvidenceKind::NormalizedCommand,
                SourceSystem::Normalizer,
                "normalized command",
                json!({
                    "room": "hallway",
                    "device_type": "light",
                    "action": "set_brightness",
                    "brightness": 30,
                    "time_after": "22:00"
                }),
            ))
            .expect("normalized evidence");

        let step = store
            .append_step(
                &trace.trace_id,
                NewCommandStep::new("normalize", StepStatus::Succeeded)
                    .with_evidence_refs(vec![raw_user_input.id.clone(), normalized.id.clone()]),
            )
            .expect("append step");

        let loaded_trace = store.read_trace(&trace.trace_id).expect("read trace");
        assert_eq!(loaded_trace.raw_user_input_ref, raw_user_input.id);

        let steps = store
            .steps_for_trace(&trace.trace_id)
            .expect("steps for trace");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].step_id, step.step_id);
        assert_eq!(steps[0].evidence_refs.len(), 2);
        assert!(steps[0].evidence_refs.contains(&normalized.id));
    }

    #[test]
    fn gate_check_records_accepted_and_rejected_reasons() {
        let store = TraceStore::in_memory().expect("trace store");
        let raw_user_input = store
            .record_evidence(NewEvidence::new(
                EvidenceKind::RawUserInput,
                SourceSystem::User,
                "raw user input",
                json!({ "text": "关闭燃气报警器" }),
            ))
            .expect("raw input evidence");
        let trace = store
            .start_trace(raw_user_input.id.clone(), "strict_mode")
            .expect("start trace");
        let step = store
            .append_step(
                &trace.trace_id,
                NewCommandStep::new("policy_gate", StepStatus::Rejected)
                    .with_evidence_refs(vec![raw_user_input.id.clone()]),
            )
            .expect("append step");

        store
            .append_gate_check(
                &trace.trace_id,
                NewGateCheck::new(
                    "dangerous_action_policy",
                    GateOutcome::Rejected,
                    "gas safety devices cannot be disabled automatically",
                )
                .with_step(step.step_id)
                .with_evidence_refs(vec![raw_user_input.id]),
            )
            .expect("append gate check");

        let checks = store
            .gate_checks_for_trace(&trace.trace_id)
            .expect("gate checks");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].outcome, GateOutcome::Rejected);
        assert!(checks[0].reason.contains("gas safety"));
    }

    #[test]
    fn audit_sink_appends_events_for_trace() {
        let path = std::env::temp_dir().join(format!(
            "edgehome-trace-test-{}.sqlite",
            Uuid::new_v4().simple()
        ));
        let store = TraceStore::open(&path).expect("trace store");
        let raw_user_input = store
            .record_evidence(NewEvidence::new(
                EvidenceKind::RawUserInput,
                SourceSystem::User,
                "raw user input",
                json!({ "text": "把客厅灯关掉" }),
            ))
            .expect("raw input evidence");
        let trace = store
            .start_trace(raw_user_input.id, "low_memory")
            .expect("start trace");
        let audit = AuditSink::open(&path).expect("audit sink");

        audit
            .append(
                NewAuditEvent::new(
                    "dry_run_ready",
                    "dry-run plan generated",
                    json!({ "dry_run": true }),
                )
                .with_trace_id(trace.trace_id.clone()),
            )
            .expect("append audit event");

        let events = audit
            .events_for_trace(&trace.trace_id)
            .expect("events for trace");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "dry_run_ready");
        assert_eq!(events[0].payload["dry_run"], true);

        drop(audit);
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
