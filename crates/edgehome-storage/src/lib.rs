//! Evidence storage for EdgeHome Harness.
//!
//! Evidence is the immutable fact layer used by trace, replay, eval, and gates.
//! The model never writes evidence directly; the harness records every relevant
//! input, output, snapshot, and executor response with a stable evidence id.

use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub mod sqlite;

use sqlite::{SqlValue, SqliteConnection, text};

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("time parse error: {0}")]
    TimeParse(#[from] time::error::Parse),

    #[error("time format error: {0}")]
    TimeFormat(#[from] time::error::Format),

    #[error("string contains an interior nul byte: {0}")]
    Nul(#[from] std::ffi::NulError),

    #[error("missing sqlite column at index {0}")]
    MissingColumn(usize),

    #[error("sqlite column {0} has unexpected type")]
    UnexpectedColumnType(usize),

    #[error("unknown evidence kind: {0}")]
    UnknownEvidenceKind(String),

    #[error("unknown source system: {0}")]
    UnknownSourceSystem(String),

    #[error("evidence not found: {0}")]
    EvidenceNotFound(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvidenceId(pub String);

impl EvidenceId {
    pub fn new() -> Self {
        Self(new_id_with_prefix("ev"))
    }
}

impl Default for EvidenceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for EvidenceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    RawUserInput,
    RawModelOutput,
    ParsedJson,
    NormalizedCommand,
    DeviceRegistrySnapshot,
    CapabilitySnapshot,
    DeviceStateSnapshot,
    PolicyRuleSnapshot,
    UserConfirmation,
    DryRunPlan,
    ExecutorRequest,
    ExecutorResponse,
    PostExecuteStateSnapshot,
    EvalCase,
    EvalResult,
    MemoryWriteRequest,
    MemoryItem,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawUserInput => "raw_user_input",
            Self::RawModelOutput => "raw_model_output",
            Self::ParsedJson => "parsed_json",
            Self::NormalizedCommand => "normalized_command",
            Self::DeviceRegistrySnapshot => "device_registry_snapshot",
            Self::CapabilitySnapshot => "capability_snapshot",
            Self::DeviceStateSnapshot => "device_state_snapshot",
            Self::PolicyRuleSnapshot => "policy_rule_snapshot",
            Self::UserConfirmation => "user_confirmation",
            Self::DryRunPlan => "dry_run_plan",
            Self::ExecutorRequest => "executor_request",
            Self::ExecutorResponse => "executor_response",
            Self::PostExecuteStateSnapshot => "post_execute_state_snapshot",
            Self::EvalCase => "eval_case",
            Self::EvalResult => "eval_result",
            Self::MemoryWriteRequest => "memory_write_request",
            Self::MemoryItem => "memory_item",
        }
    }
}

impl FromStr for EvidenceKind {
    type Err = StorageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "raw_user_input" => Ok(Self::RawUserInput),
            "raw_model_output" => Ok(Self::RawModelOutput),
            "parsed_json" => Ok(Self::ParsedJson),
            "normalized_command" => Ok(Self::NormalizedCommand),
            "device_registry_snapshot" => Ok(Self::DeviceRegistrySnapshot),
            "capability_snapshot" => Ok(Self::CapabilitySnapshot),
            "device_state_snapshot" => Ok(Self::DeviceStateSnapshot),
            "policy_rule_snapshot" => Ok(Self::PolicyRuleSnapshot),
            "user_confirmation" => Ok(Self::UserConfirmation),
            "dry_run_plan" => Ok(Self::DryRunPlan),
            "executor_request" => Ok(Self::ExecutorRequest),
            "executor_response" => Ok(Self::ExecutorResponse),
            "post_execute_state_snapshot" => Ok(Self::PostExecuteStateSnapshot),
            "eval_case" => Ok(Self::EvalCase),
            "eval_result" => Ok(Self::EvalResult),
            "memory_write_request" => Ok(Self::MemoryWriteRequest),
            "memory_item" => Ok(Self::MemoryItem),
            other => Err(StorageError::UnknownEvidenceKind(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSystem {
    User,
    Model,
    Parser,
    Normalizer,
    Registry,
    DeviceState,
    Policy,
    Executor,
    Memory,
    Eval,
    Harness,
}

impl SourceSystem {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Model => "model",
            Self::Parser => "parser",
            Self::Normalizer => "normalizer",
            Self::Registry => "registry",
            Self::DeviceState => "device_state",
            Self::Policy => "policy",
            Self::Executor => "executor",
            Self::Memory => "memory",
            Self::Eval => "eval",
            Self::Harness => "harness",
        }
    }
}

impl FromStr for SourceSystem {
    type Err = StorageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "user" => Ok(Self::User),
            "model" => Ok(Self::Model),
            "parser" => Ok(Self::Parser),
            "normalizer" => Ok(Self::Normalizer),
            "registry" => Ok(Self::Registry),
            "device_state" => Ok(Self::DeviceState),
            "policy" => Ok(Self::Policy),
            "executor" => Ok(Self::Executor),
            "memory" => Ok(Self::Memory),
            "eval" => Ok(Self::Eval),
            "harness" => Ok(Self::Harness),
            other => Err(StorageError::UnknownSourceSystem(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Stale,
    NoExpiry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub id: EvidenceId,
    pub kind: EvidenceKind,
    pub source: SourceSystem,
    pub summary: String,
    pub content: Value,
    pub content_hash: String,
    pub created_at: OffsetDateTime,
    pub observed_at: Option<OffsetDateTime>,
    pub expires_at: Option<OffsetDateTime>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewEvidence {
    pub kind: EvidenceKind,
    pub source: SourceSystem,
    pub summary: String,
    pub content: Value,
    pub observed_at: Option<OffsetDateTime>,
    pub expires_at: Option<OffsetDateTime>,
    pub metadata: Value,
}

impl NewEvidence {
    pub fn new(
        kind: EvidenceKind,
        source: SourceSystem,
        summary: impl Into<String>,
        content: Value,
    ) -> Self {
        Self {
            kind,
            source,
            summary: summary.into(),
            content,
            observed_at: None,
            expires_at: None,
            metadata: Value::Object(Default::default()),
        }
    }

    pub fn with_expiry(mut self, expires_at: OffsetDateTime) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn with_observed_at(mut self, observed_at: OffsetDateTime) -> Self {
        self.observed_at = Some(observed_at);
        self
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

pub struct EvidenceStore {
    connection: SqliteConnection,
}

pub fn new_id_with_prefix(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);

    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("{prefix}_{nanos:x}_{counter:x}")
}

impl EvidenceStore {
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let connection = SqliteConnection::open(path)?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> StorageResult<Self> {
        let connection = SqliteConnection::open_in_memory()?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn from_connection(connection: SqliteConnection) -> StorageResult<Self> {
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn connection(&self) -> &SqliteConnection {
        &self.connection
    }

    pub fn record(&self, evidence: NewEvidence) -> StorageResult<EvidenceRef> {
        let id = EvidenceId::new();
        let created_at = OffsetDateTime::now_utc();
        let content_json = serde_json::to_string(&evidence.content)?;
        let metadata_json = serde_json::to_string(&evidence.metadata)?;
        let content_hash = stable_hash_hex(&content_json);

        self.connection.execute(
            "INSERT INTO evidence_refs (
                evidence_id,
                kind,
                source,
                summary,
                content_json,
                content_hash,
                created_at,
                observed_at,
                expires_at,
                metadata_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            &[
                text(&id.0),
                text(evidence.kind.as_str()),
                text(evidence.source.as_str()),
                text(&evidence.summary),
                text(&content_json),
                text(&content_hash),
                text(format_time(created_at)?),
                optional_text(format_optional_time(evidence.observed_at)?),
                optional_text(format_optional_time(evidence.expires_at)?),
                text(&metadata_json),
            ],
        )?;

        self.read(&id)
    }

    pub fn read(&self, id: &EvidenceId) -> StorageResult<EvidenceRef> {
        let Some(row) = self.connection.query_one(
            "SELECT
                    evidence_id,
                    kind,
                    source,
                    summary,
                    content_json,
                    content_hash,
                    created_at,
                    observed_at,
                    expires_at,
                    metadata_json
                FROM evidence_refs
                WHERE evidence_id = ?1",
            &[text(&id.0)],
        )?
        else {
            return Err(StorageError::EvidenceNotFound(id.0.clone()));
        };

        EvidenceRef::try_from(EvidenceRow::try_from(row)?)
    }

    pub fn freshness(&self, id: &EvidenceId) -> StorageResult<Freshness> {
        let evidence = self.read(id)?;
        match evidence.expires_at {
            Some(expires_at) if expires_at < OffsetDateTime::now_utc() => Ok(Freshness::Stale),
            Some(_) => Ok(Freshness::Fresh),
            None => Ok(Freshness::NoExpiry),
        }
    }

    fn migrate(&self) -> StorageResult<()> {
        self.connection.execute_batch(
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

            CREATE INDEX IF NOT EXISTS idx_evidence_refs_kind
                ON evidence_refs(kind);

            CREATE INDEX IF NOT EXISTS idx_evidence_refs_source
                ON evidence_refs(source);

            CREATE INDEX IF NOT EXISTS idx_evidence_refs_created_at
                ON evidence_refs(created_at);",
        )?;
        Ok(())
    }
}

struct EvidenceRow {
    evidence_id: String,
    kind: String,
    source: String,
    summary: String,
    content_json: String,
    content_hash: String,
    created_at: String,
    observed_at: Option<String>,
    expires_at: Option<String>,
    metadata_json: String,
}

impl TryFrom<sqlite::SqlRow> for EvidenceRow {
    type Error = StorageError;

    fn try_from(row: sqlite::SqlRow) -> Result<Self, Self::Error> {
        Ok(Self {
            evidence_id: row.text(0)?,
            kind: row.text(1)?,
            source: row.text(2)?,
            summary: row.text(3)?,
            content_json: row.text(4)?,
            content_hash: row.text(5)?,
            created_at: row.text(6)?,
            observed_at: row.optional_text(7)?,
            expires_at: row.optional_text(8)?,
            metadata_json: row.text(9)?,
        })
    }
}

impl TryFrom<EvidenceRow> for EvidenceRef {
    type Error = StorageError;

    fn try_from(row: EvidenceRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: EvidenceId(row.evidence_id),
            kind: EvidenceKind::from_str(&row.kind)?,
            source: SourceSystem::from_str(&row.source)?,
            summary: row.summary,
            content: serde_json::from_str(&row.content_json)?,
            content_hash: row.content_hash,
            created_at: parse_time(&row.created_at)?,
            observed_at: parse_optional_time(row.observed_at.as_deref())?,
            expires_at: parse_optional_time(row.expires_at.as_deref())?,
            metadata: serde_json::from_str(&row.metadata_json)?,
        })
    }
}

fn optional_text(value: Option<String>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::Text)
}

fn stable_hash_hex(value: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn format_time(value: OffsetDateTime) -> StorageResult<String> {
    Ok(value.format(&Rfc3339)?)
}

fn format_optional_time(value: Option<OffsetDateTime>) -> StorageResult<Option<String>> {
    value.map(format_time).transpose()
}

fn parse_time(value: &str) -> StorageResult<OffsetDateTime> {
    Ok(OffsetDateTime::parse(value, &Rfc3339)?)
}

fn parse_optional_time(value: Option<&str>) -> StorageResult<Option<OffsetDateTime>> {
    value.map(parse_time).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::Duration;

    #[test]
    fn record_read_and_freshness_round_trip() {
        let store = EvidenceStore::in_memory().expect("store");
        let expires_at = OffsetDateTime::now_utc() + Duration::minutes(5);

        let evidence = store
            .record(
                NewEvidence::new(
                    EvidenceKind::RawUserInput,
                    SourceSystem::User,
                    "user asked to turn off living room light",
                    json!({ "text": "把客厅灯关掉" }),
                )
                .with_expiry(expires_at),
            )
            .expect("record evidence");

        assert_eq!(evidence.kind, EvidenceKind::RawUserInput);
        assert_eq!(evidence.source, SourceSystem::User);
        assert_eq!(
            store.freshness(&evidence.id).expect("freshness"),
            Freshness::Fresh
        );

        let loaded = store.read(&evidence.id).expect("read evidence");
        assert_eq!(loaded.summary, "user asked to turn off living room light");
        assert_eq!(loaded.content["text"], "把客厅灯关掉");
        assert_eq!(loaded.content_hash, evidence.content_hash);
    }

    #[test]
    fn expired_evidence_is_stale() {
        let store = EvidenceStore::in_memory().expect("store");
        let expires_at = OffsetDateTime::now_utc() - Duration::seconds(1);

        let evidence = store
            .record(
                NewEvidence::new(
                    EvidenceKind::DeviceStateSnapshot,
                    SourceSystem::DeviceState,
                    "old state",
                    json!({ "state": "on" }),
                )
                .with_expiry(expires_at),
            )
            .expect("record evidence");

        assert_eq!(
            store.freshness(&evidence.id).expect("freshness"),
            Freshness::Stale
        );
    }

    #[test]
    fn evidence_without_expiry_has_no_expiry_freshness() {
        let store = EvidenceStore::in_memory().expect("store");

        let evidence = store
            .record(NewEvidence::new(
                EvidenceKind::PolicyRuleSnapshot,
                SourceSystem::Policy,
                "policy snapshot",
                json!({ "rule": "deny gas automation" }),
            ))
            .expect("record evidence");

        assert_eq!(
            store.freshness(&evidence.id).expect("freshness"),
            Freshness::NoExpiry
        );
    }
}
