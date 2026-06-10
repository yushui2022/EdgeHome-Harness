//! Lightweight local memory for EdgeHome Harness.
//!
//! Memory in this project is not an unbounded chat transcript. It is a bounded,
//! evidence-backed state layer: short session memory keeps only structured
//! recent commands, long-term memory persists confirmed preferences in SQLite,
//! and context assembly emits compact summaries under the active RAM profile.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use edgehome_config::RuntimeProfile;
use edgehome_core::{DeviceId, DeviceType, NormalizedCommand, RiskLevel, Room};
use edgehome_gate::MemoryWriteGate;
use edgehome_storage::sqlite::{SqlRow, SqlValue, SqliteConnection, integer, text};
use edgehome_storage::{EvidenceId, StorageError, new_id_with_prefix};
use edgehome_trace::TraceId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub type MemoryResult<T> = Result<T, MemoryError>;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("time parse error: {0}")]
    TimeParse(#[from] time::error::Parse),

    #[error("time format error: {0}")]
    TimeFormat(#[from] time::error::Format),

    #[error("failed to read memory store `{path}`: {source}")]
    ReadStore {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("memory write denied: {0}")]
    WriteDenied(String),

    #[error("safety memory cannot weaken safety policy")]
    SafetyWeakening,

    #[error("memory item not found: {0}")]
    MemoryItemNotFound(String),

    #[error("unknown memory scope: {0}")]
    UnknownScope(String),

    #[error("unknown memory kind: {0}")]
    UnknownKind(String),

    #[error("unknown safety effect: {0}")]
    UnknownSafetyEffect(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShortMemoryTurn {
    pub trace_id: Option<TraceId>,
    pub user_text: String,
    pub command: NormalizedCommand,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryTarget {
    pub room: Room,
    pub device_id: DeviceId,
    pub device_type: DeviceType,
    pub risk: RiskLevel,
}

#[derive(Debug, Clone)]
pub struct ShortSessionMemory {
    max_turns: usize,
    turns: VecDeque<ShortMemoryTurn>,
}

impl ShortSessionMemory {
    pub fn new(max_turns: usize) -> Self {
        Self {
            max_turns: max_turns.max(1),
            turns: VecDeque::new(),
        }
    }

    pub fn append(
        &mut self,
        user_text: impl Into<String>,
        command: NormalizedCommand,
        trace_id: Option<TraceId>,
    ) {
        self.turns.push_back(ShortMemoryTurn {
            trace_id,
            user_text: user_text.into(),
            command,
            created_at: OffsetDateTime::now_utc(),
        });
        while self.turns.len() > self.max_turns {
            self.turns.pop_front();
        }
    }

    pub fn clear_idle(&mut self, now: OffsetDateTime, idle_after: Duration) {
        let Some(last_turn) = self.turns.back() else {
            return;
        };
        if now - last_turn.created_at >= idle_after {
            self.turns.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.turns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    pub fn turns(&self) -> impl DoubleEndedIterator<Item = &ShortMemoryTurn> {
        self.turns.iter()
    }

    pub fn last_target(&self) -> Option<MemoryTarget> {
        self.turns.iter().rev().find_map(|turn| {
            Some(MemoryTarget {
                room: turn.command.room.clone(),
                device_id: turn.command.device_id.clone()?,
                device_type: turn.command.device_type.clone(),
                risk: turn.command.risk.clone(),
            })
        })
    }

    pub fn resolve_relative_command(
        &self,
        command: &NormalizedCommand,
    ) -> Option<NormalizedCommand> {
        let is_relative = command.device_id.is_none()
            || command
                .params
                .raw_value
                .as_deref()
                .is_some_and(|value| value == "relative_command");
        if !is_relative {
            return Some(command.clone());
        }

        let target = self
            .last_target_matching(&command.device_type)
            .or_else(|| self.last_target())?;
        let mut resolved = command.clone();
        resolved.room = target.room;
        resolved.device_id = Some(target.device_id);
        resolved.device_type = target.device_type;
        resolved.risk = target.risk;
        Some(resolved)
    }

    fn last_target_matching(&self, device_type: &DeviceType) -> Option<MemoryTarget> {
        if *device_type == DeviceType::Unknown {
            return None;
        }

        self.turns.iter().rev().find_map(|turn| {
            if turn.command.device_type != *device_type {
                return None;
            }

            Some(MemoryTarget {
                room: turn.command.room.clone(),
                device_id: turn.command.device_id.clone()?,
                device_type: turn.command.device_type.clone(),
                risk: turn.command.risk.clone(),
            })
        })
    }
}

impl Default for ShortSessionMemory {
    fn default() -> Self {
        Self::new(3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    User,
    Device,
    Room,
    Scene,
    Safety,
}

impl MemoryScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Device => "device",
            Self::Room => "room",
            Self::Scene => "scene",
            Self::Safety => "safety",
        }
    }
}

impl FromStr for MemoryScope {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "user" => Ok(Self::User),
            "device" => Ok(Self::Device),
            "room" => Ok(Self::Room),
            "scene" => Ok(Self::Scene),
            "safety" => Ok(Self::Safety),
            other => Err(MemoryError::UnknownScope(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    DeviceAlias,
    RoomAlias,
    UserPreference,
    SceneDefault,
    SafetyRule,
}

impl MemoryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::DeviceAlias => "device_alias",
            Self::RoomAlias => "room_alias",
            Self::UserPreference => "user_preference",
            Self::SceneDefault => "scene_default",
            Self::SafetyRule => "safety_rule",
        }
    }
}

impl FromStr for MemoryKind {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "device_alias" => Ok(Self::DeviceAlias),
            "room_alias" => Ok(Self::RoomAlias),
            "user_preference" => Ok(Self::UserPreference),
            "scene_default" => Ok(Self::SceneDefault),
            "safety_rule" => Ok(Self::SafetyRule),
            other => Err(MemoryError::UnknownKind(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyEffect {
    StrengthenOnly,
    Neutral,
}

impl SafetyEffect {
    fn as_str(self) -> &'static str {
        match self {
            Self::StrengthenOnly => "strengthen_only",
            Self::Neutral => "neutral",
        }
    }
}

impl FromStr for SafetyEffect {
    type Err = MemoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "strengthen_only" => Ok(Self::StrengthenOnly),
            "neutral" => Ok(Self::Neutral),
            other => Err(MemoryError::UnknownSafetyEffect(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub key: String,
    pub value: Value,
    pub source_evidence_id: EvidenceId,
    pub confidence_milli: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub safety_effect: SafetyEffect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewMemoryItem {
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub key: String,
    pub value: Value,
    pub source_evidence_id: EvidenceId,
    pub confidence_milli: i64,
    pub expires_at: Option<OffsetDateTime>,
    pub safety_effect: SafetyEffect,
}

impl NewMemoryItem {
    pub fn new(
        scope: MemoryScope,
        kind: MemoryKind,
        key: impl Into<String>,
        value: Value,
        source_evidence_id: EvidenceId,
    ) -> Self {
        Self {
            scope,
            kind,
            key: key.into(),
            value,
            source_evidence_id,
            confidence_milli: 1000,
            expires_at: None,
            safety_effect: SafetyEffect::Neutral,
        }
    }

    pub fn with_safety_effect(mut self, safety_effect: SafetyEffect) -> Self {
        self.safety_effect = safety_effect;
        self
    }

    pub fn with_confidence_milli(mut self, confidence_milli: i64) -> Self {
        self.confidence_milli = confidence_milli.clamp(0, 1000);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryWriteRequest {
    pub item: NewMemoryItem,
    pub user_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ExplicitMemoryWriteDetection {
    None,
    DeviceAlias {
        target_alias: String,
        new_alias: String,
    },
    Rejected {
        reason: String,
    },
}

pub struct ExplicitMemoryWriteDetector;

impl ExplicitMemoryWriteDetector {
    pub fn detect(raw: &str) -> ExplicitMemoryWriteDetection {
        let text = normalize_memory_text(raw);
        if text.is_empty() {
            return ExplicitMemoryWriteDetection::None;
        }

        if looks_like_long_term_safety_weakening(&text) {
            return ExplicitMemoryWriteDetection::Rejected {
                reason: "long-term memory cannot weaken safety policy".to_owned(),
            };
        }

        parse_device_alias_memory(&text).unwrap_or(ExplicitMemoryWriteDetection::None)
    }
}

pub struct LongTermPreferenceStore {
    connection: SqliteConnection,
}

impl LongTermPreferenceStore {
    pub fn open(path: impl AsRef<Path>) -> MemoryResult<Self> {
        let connection = SqliteConnection::open(path)?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> MemoryResult<Self> {
        let connection = SqliteConnection::open_in_memory()?;
        let store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    pub fn put_confirmed(&self, request: MemoryWriteRequest) -> MemoryResult<MemoryItem> {
        let decision = MemoryWriteGate::check(true, request.user_confirmed, true);
        if decision.blocking {
            return Err(MemoryError::WriteDenied(decision.reason));
        }
        SafetyMemory::validate_new_item(&request.item)?;

        let item = MemoryItem {
            id: new_id_with_prefix("mem"),
            scope: request.item.scope,
            kind: request.item.kind,
            key: request.item.key,
            value: request.item.value,
            source_evidence_id: request.item.source_evidence_id,
            confidence_milli: request.item.confidence_milli,
            created_at: OffsetDateTime::now_utc(),
            expires_at: request.item.expires_at,
            safety_effect: request.item.safety_effect,
        };
        let value_json = serde_json::to_string(&item.value)?;

        self.connection.execute(
            "INSERT INTO memory_items (
                id,
                scope,
                kind,
                key,
                value_json,
                source_evidence_id,
                confidence_milli,
                created_at,
                expires_at,
                safety_effect
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            &[
                text(&item.id),
                text(item.scope.as_str()),
                text(item.kind.as_str()),
                text(&item.key),
                text(value_json),
                text(&item.source_evidence_id.0),
                integer(item.confidence_milli),
                text(format_time(item.created_at)?),
                optional_text(format_optional_time(item.expires_at)?),
                text(item.safety_effect.as_str()),
            ],
        )?;

        self.read(&item.id)
    }

    pub fn read(&self, id: &str) -> MemoryResult<MemoryItem> {
        let Some(row) = self.connection.query_one(
            "SELECT
                id,
                scope,
                kind,
                key,
                value_json,
                source_evidence_id,
                confidence_milli,
                created_at,
                expires_at,
                safety_effect
             FROM memory_items
             WHERE id = ?1",
            &[text(id)],
        )?
        else {
            return Err(MemoryError::MemoryItemNotFound(id.to_owned()));
        };

        MemoryItem::try_from(MemoryItemRow::try_from(row)?)
    }

    pub fn list_all(&self) -> MemoryResult<Vec<MemoryItem>> {
        self.connection
            .query_all(
                "SELECT
                    id,
                    scope,
                    kind,
                    key,
                    value_json,
                    source_evidence_id,
                    confidence_milli,
                    created_at,
                    expires_at,
                    safety_effect
                 FROM memory_items
                 ORDER BY created_at DESC",
                &[],
            )?
            .into_iter()
            .map(MemoryItemRow::try_from)
            .map(|row| row.and_then(MemoryItem::try_from))
            .collect()
    }

    pub fn list_relevant(&self, query: &str, limit: usize) -> MemoryResult<Vec<MemoryItem>> {
        let query = query.trim();
        let mut items = self
            .list_all()?
            .into_iter()
            .filter(|item| {
                query.is_empty()
                    || item.key.contains(query)
                    || query.contains(&item.key)
                    || item.value.to_string().contains(query)
            })
            .collect::<Vec<_>>();
        items.truncate(limit);
        Ok(items)
    }

    fn migrate(&self) -> MemoryResult<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_items (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                key TEXT NOT NULL,
                value_json TEXT NOT NULL,
                source_evidence_id TEXT NOT NULL,
                confidence_milli INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT,
                safety_effect TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memory_items_scope_kind
                ON memory_items(scope, kind);

            CREATE INDEX IF NOT EXISTS idx_memory_items_key
                ON memory_items(key);",
        )?;
        Ok(())
    }
}

pub struct SafetyMemory;

impl SafetyMemory {
    pub fn validate_new_item(item: &NewMemoryItem) -> MemoryResult<()> {
        if item.kind == MemoryKind::SafetyRule && item.safety_effect != SafetyEffect::StrengthenOnly
        {
            return Err(MemoryError::SafetyWeakening);
        }

        if item.kind == MemoryKind::SafetyRule && looks_like_safety_weakening(&item.value) {
            return Err(MemoryError::SafetyWeakening);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAssemblerConfig {
    pub memory_enabled: bool,
    pub max_short_memory_turns: usize,
    pub max_long_term_items: usize,
    pub max_context_chars: usize,
    pub include_long_term: bool,
}

impl From<&RuntimeProfile> for ContextAssemblerConfig {
    fn from(profile: &RuntimeProfile) -> Self {
        Self {
            memory_enabled: profile.memory_enabled,
            max_short_memory_turns: usize::from(profile.max_short_memory_turns),
            max_long_term_items: 3,
            max_context_chars: profile.max_context_chars,
            include_long_term: profile.memory_enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptContext {
    pub text: String,
    pub short_turns_used: usize,
    pub long_items_used: usize,
    pub evidence_refs: Vec<EvidenceId>,
    pub truncated: bool,
    pub budget_chars: usize,
}

#[derive(Debug, Clone)]
pub struct ContextAssembler {
    config: ContextAssemblerConfig,
}

pub type ContextCompiler = ContextAssembler;
pub type MemoryContextBlock = PromptContext;

impl ContextAssembler {
    pub fn new(config: ContextAssemblerConfig) -> Self {
        Self { config }
    }

    pub fn from_profile(profile: &RuntimeProfile) -> Self {
        Self::new(ContextAssemblerConfig::from(profile))
    }

    pub fn assemble(
        &self,
        short_memory: &ShortSessionMemory,
        long_items: &[MemoryItem],
    ) -> PromptContext {
        if !self.config.memory_enabled {
            return PromptContext {
                text: String::new(),
                short_turns_used: 0,
                long_items_used: 0,
                evidence_refs: Vec::new(),
                truncated: false,
                budget_chars: self.config.max_context_chars,
            };
        }

        let mut lines = Vec::new();
        let mut evidence_refs = Vec::new();
        let short_turns = short_memory
            .turns()
            .rev()
            .take(self.config.max_short_memory_turns)
            .collect::<Vec<_>>();

        if let Some(target) = short_memory.last_target() {
            lines.push(format!(
                "last_target device_id={} room={:?} type={:?} risk={:?}",
                target.device_id.0, target.room, target.device_type, target.risk
            ));
        }

        for turn in short_turns.iter().rev() {
            lines.push(format!(
                "turn trace={} action={:?} device={} params={}",
                turn.trace_id
                    .as_ref()
                    .map(|trace_id| trace_id.0.as_str())
                    .unwrap_or("none"),
                turn.command.action,
                turn.command
                    .device_id
                    .as_ref()
                    .map(|device_id| device_id.0.as_str())
                    .unwrap_or("unresolved"),
                compact_json(&turn.command.params)
            ));
        }

        let mut long_items_used = 0;
        if self.config.include_long_term {
            for item in long_items.iter().take(self.config.max_long_term_items) {
                lines.push(format!(
                    "memory kind={:?} key={} value={} ref={}",
                    item.kind,
                    item.key,
                    compact_json(&item.value),
                    item.source_evidence_id.0
                ));
                evidence_refs.push(item.source_evidence_id.clone());
                long_items_used += 1;
            }
        }

        let joined = lines.join("\n");
        let (text, truncated) = truncate_chars(&joined, self.config.max_context_chars);
        PromptContext {
            text,
            short_turns_used: short_turns.len(),
            long_items_used,
            evidence_refs,
            truncated,
            budget_chars: self.config.max_context_chars,
        }
    }
}

struct MemoryItemRow {
    id: String,
    scope: String,
    kind: String,
    key: String,
    value_json: String,
    source_evidence_id: String,
    confidence_milli: i64,
    created_at: String,
    expires_at: Option<String>,
    safety_effect: String,
}

impl TryFrom<SqlRow> for MemoryItemRow {
    type Error = MemoryError;

    fn try_from(row: SqlRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.text(0)?,
            scope: row.text(1)?,
            kind: row.text(2)?,
            key: row.text(3)?,
            value_json: row.text(4)?,
            source_evidence_id: row.text(5)?,
            confidence_milli: row.i64(6)?,
            created_at: row.text(7)?,
            expires_at: row.optional_text(8)?,
            safety_effect: row.text(9)?,
        })
    }
}

impl TryFrom<MemoryItemRow> for MemoryItem {
    type Error = MemoryError;

    fn try_from(row: MemoryItemRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.id,
            scope: MemoryScope::from_str(&row.scope)?,
            kind: MemoryKind::from_str(&row.kind)?,
            key: row.key,
            value: serde_json::from_str(&row.value_json)?,
            source_evidence_id: EvidenceId(row.source_evidence_id),
            confidence_milli: row.confidence_milli,
            created_at: parse_time(&row.created_at)?,
            expires_at: parse_optional_time(row.expires_at.as_deref())?,
            safety_effect: SafetyEffect::from_str(&row.safety_effect)?,
        })
    }
}

fn optional_text(value: Option<String>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::Text)
}

fn format_time(value: OffsetDateTime) -> MemoryResult<String> {
    Ok(value.format(&Rfc3339)?)
}

fn format_optional_time(value: Option<OffsetDateTime>) -> MemoryResult<Option<String>> {
    value.map(format_time).transpose()
}

fn parse_time(value: &str) -> MemoryResult<OffsetDateTime> {
    Ok(OffsetDateTime::parse(value, &Rfc3339)?)
}

fn parse_optional_time(value: Option<&str>) -> MemoryResult<Option<OffsetDateTime>> {
    value.map(parse_time).transpose()
}

fn compact_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    if value.chars().count() <= max_chars {
        return (value.to_owned(), false);
    }

    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(14))
        .collect::<String>();
    output.push_str("\n[truncated]");
    (output, true)
}

fn normalize_memory_text(raw: &str) -> String {
    raw.trim()
        .trim_end_matches(['。', '.', '！', '!', '，', ','])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
}

fn parse_device_alias_memory(text: &str) -> Option<ExplicitMemoryWriteDetection> {
    let body = text
        .strip_prefix("以后把")
        .or_else(|| text.strip_prefix("记住把"))?;
    let (target_alias, new_alias) = split_alias_pair(body)?;
    if target_alias.is_empty() || new_alias.is_empty() || target_alias == new_alias {
        return Some(ExplicitMemoryWriteDetection::Rejected {
            reason: "invalid device alias memory".to_owned(),
        });
    }

    Some(ExplicitMemoryWriteDetection::DeviceAlias {
        target_alias: target_alias.to_owned(),
        new_alias: new_alias.to_owned(),
    })
}

fn split_alias_pair(body: &str) -> Option<(&str, &str)> {
    if let Some((target, alias)) = body.split_once("叫做") {
        return Some((target.trim(), alias.trim()));
    }
    if let Some((target, alias)) = body.split_once("叫") {
        return Some((target.trim(), alias.trim()));
    }
    None
}

fn looks_like_long_term_safety_weakening(text: &str) -> bool {
    text.starts_with("以后")
        && ["门锁", "燃气", "报警器", "摄像头"]
            .iter()
            .any(|needle| text.contains(needle))
        && ["自动打开", "不需要确认", "不用确认", "跳过确认", "直接打开"]
            .iter()
            .any(|needle| text.contains(needle))
}

fn looks_like_safety_weakening(value: &Value) -> bool {
    let text = value.to_string().to_lowercase();
    [
        "skip safety",
        "bypass",
        "no confirmation",
        "自动打开门锁",
        "不需要确认",
        "跳过安全",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{Action, CommandParams, CommandSchemaVersion, Intent};
    use serde_json::json;

    fn evidence_id() -> EvidenceId {
        EvidenceId("ev_test".to_owned())
    }

    fn command(device_id: Option<&str>, action: Action) -> NormalizedCommand {
        NormalizedCommand {
            schema_version: CommandSchemaVersion::default(),
            intent: Intent::ControlDevice,
            room: if device_id.is_some() {
                Room::LivingRoom
            } else {
                Room::Unknown
            },
            device_id: device_id.map(|value| DeviceId::new(value).expect("device id")),
            device_type: DeviceType::Light,
            action,
            params: CommandParams::default(),
            risk: if device_id.is_some() {
                RiskLevel::Low
            } else {
                RiskLevel::Unknown
            },
        }
    }

    #[test]
    fn short_memory_resolves_relative_light_command() {
        let mut memory = ShortSessionMemory::new(3);
        memory.append(
            "把客厅灯调到70%",
            command(Some("living_room_main_light"), Action::SetBrightness),
            Some(TraceId("tr_prev".to_owned())),
        );

        let relative = NormalizedCommand {
            action: Action::DecreaseBrightness,
            params: CommandParams {
                raw_value: Some("relative_command".to_owned()),
                ..CommandParams::default()
            },
            ..command(None, Action::DecreaseBrightness)
        };

        let resolved = memory
            .resolve_relative_command(&relative)
            .expect("relative resolved");

        assert_eq!(
            resolved.device_id,
            Some(DeviceId::new("living_room_main_light").expect("device id"))
        );
        assert_eq!(resolved.room, Room::LivingRoom);
        assert_eq!(resolved.device_type, DeviceType::Light);
        assert_eq!(resolved.risk, RiskLevel::Low);
    }

    #[test]
    fn short_memory_evicts_old_turns() {
        let mut memory = ShortSessionMemory::new(2);
        memory.append("one", command(Some("one"), Action::TurnOn), None);
        memory.append("two", command(Some("two"), Action::TurnOn), None);
        memory.append("three", command(Some("three"), Action::TurnOn), None);

        assert_eq!(memory.len(), 2);
        assert_eq!(
            memory
                .turns()
                .next()
                .and_then(|turn| turn.command.device_id.as_ref())
                .map(|id| id.0.as_str()),
            Some("two")
        );
    }

    #[test]
    fn long_term_store_requires_user_confirmation() {
        let store = LongTermPreferenceStore::in_memory().expect("store");
        let item = NewMemoryItem::new(
            MemoryScope::Device,
            MemoryKind::DeviceAlias,
            "屋里大灯",
            json!({ "device_id": "living_room_main_light" }),
            evidence_id(),
        );

        let error = store
            .put_confirmed(MemoryWriteRequest {
                item,
                user_confirmed: false,
            })
            .expect_err("confirmation required");

        assert!(matches!(error, MemoryError::WriteDenied(_)));
    }

    #[test]
    fn long_term_store_persists_confirmed_memory_item() {
        let store = LongTermPreferenceStore::in_memory().expect("store");
        let item = NewMemoryItem::new(
            MemoryScope::Device,
            MemoryKind::DeviceAlias,
            "屋里大灯",
            json!({ "device_id": "living_room_main_light" }),
            evidence_id(),
        );

        let saved = store
            .put_confirmed(MemoryWriteRequest {
                item,
                user_confirmed: true,
            })
            .expect("saved");
        let loaded = store.read(&saved.id).expect("loaded");

        assert_eq!(loaded.key, "屋里大灯");
        assert_eq!(loaded.value["device_id"], "living_room_main_light");
        assert_eq!(store.list_all().expect("all").len(), 1);
    }

    #[test]
    fn explicit_memory_detector_extracts_device_alias_write() {
        assert_eq!(
            ExplicitMemoryWriteDetector::detect("以后把玄关灯叫小夜灯"),
            ExplicitMemoryWriteDetection::DeviceAlias {
                target_alias: "玄关灯".to_owned(),
                new_alias: "小夜灯".to_owned(),
            }
        );

        assert_eq!(
            ExplicitMemoryWriteDetector::detect("记住把走廊灯叫做夜灯。"),
            ExplicitMemoryWriteDetection::DeviceAlias {
                target_alias: "走廊灯".to_owned(),
                new_alias: "夜灯".to_owned(),
            }
        );
    }

    #[test]
    fn explicit_memory_detector_ignores_one_shot_control() {
        assert_eq!(
            ExplicitMemoryWriteDetector::detect("打开卧室灯"),
            ExplicitMemoryWriteDetection::None
        );
    }

    #[test]
    fn explicit_memory_detector_rejects_safety_weakening() {
        assert!(matches!(
            ExplicitMemoryWriteDetector::detect("以后门锁都自动打开"),
            ExplicitMemoryWriteDetection::Rejected { .. }
        ));
    }

    #[test]
    fn safety_memory_rejects_weakening_safety_rule() {
        let item = NewMemoryItem::new(
            MemoryScope::Safety,
            MemoryKind::SafetyRule,
            "door_lock",
            json!({ "rule": "以后打开门锁不需要确认" }),
            evidence_id(),
        )
        .with_safety_effect(SafetyEffect::StrengthenOnly);

        assert!(matches!(
            SafetyMemory::validate_new_item(&item).expect_err("weakening"),
            MemoryError::SafetyWeakening
        ));
    }

    #[test]
    fn safety_memory_accepts_strengthening_safety_rule() {
        let item = NewMemoryItem::new(
            MemoryScope::Safety,
            MemoryKind::SafetyRule,
            "door_lock",
            json!({ "rule": "night unlock requires second confirmation" }),
            evidence_id(),
        )
        .with_safety_effect(SafetyEffect::StrengthenOnly);

        SafetyMemory::validate_new_item(&item).expect("strengthening accepted");
    }

    #[test]
    fn context_assembler_respects_budget_and_uses_refs() {
        let mut short = ShortSessionMemory::new(3);
        short.append(
            "把客厅灯调到70%",
            command(Some("living_room_main_light"), Action::SetBrightness),
            Some(TraceId("tr_prev".to_owned())),
        );
        let item = MemoryItem {
            id: "mem_1".to_owned(),
            scope: MemoryScope::Device,
            kind: MemoryKind::DeviceAlias,
            key: "屋里大灯".to_owned(),
            value: json!({ "device_id": "living_room_main_light" }),
            source_evidence_id: evidence_id(),
            confidence_milli: 1000,
            created_at: OffsetDateTime::now_utc(),
            expires_at: None,
            safety_effect: SafetyEffect::Neutral,
        };
        let assembler = ContextAssembler::new(ContextAssemblerConfig {
            memory_enabled: true,
            max_short_memory_turns: 3,
            max_long_term_items: 3,
            max_context_chars: 500,
            include_long_term: true,
        });

        let context = assembler.assemble(&short, &[item]);

        assert!(context.text.chars().count() <= 500);
        assert!(context.text.contains("last_target"));
        assert!(context.text.contains("ref=ev_test"));
        assert_eq!(context.evidence_refs, vec![evidence_id()]);
    }

    #[test]
    fn context_assembler_can_disable_memory_for_low_resource_fallback() {
        let assembler = ContextAssembler::new(ContextAssemblerConfig {
            memory_enabled: false,
            max_short_memory_turns: 0,
            max_long_term_items: 0,
            max_context_chars: 1,
            include_long_term: false,
        });

        let context = assembler.assemble(&ShortSessionMemory::default(), &[]);

        assert!(context.text.is_empty());
        assert_eq!(context.short_turns_used, 0);
        assert_eq!(context.long_items_used, 0);
        assert_eq!(context.budget_chars, 1);
    }

    #[test]
    fn context_assembler_limits_long_term_items_inside_compiler() {
        let items = (0..5)
            .map(|index| MemoryItem {
                id: format!("mem_{index}"),
                scope: MemoryScope::Device,
                kind: MemoryKind::DeviceAlias,
                key: format!("alias_{index}"),
                value: json!({ "device_id": "living_room_main_light" }),
                source_evidence_id: EvidenceId(format!("ev_{index}")),
                confidence_milli: 1000,
                created_at: OffsetDateTime::now_utc(),
                expires_at: None,
                safety_effect: SafetyEffect::Neutral,
            })
            .collect::<Vec<_>>();
        let assembler = ContextAssembler::new(ContextAssemblerConfig {
            memory_enabled: true,
            max_short_memory_turns: 3,
            max_long_term_items: 3,
            max_context_chars: 500,
            include_long_term: true,
        });

        let context = assembler.assemble(&ShortSessionMemory::default(), &items);

        assert_eq!(context.long_items_used, 3);
        assert_eq!(
            context.evidence_refs,
            vec![
                EvidenceId("ev_0".to_owned()),
                EvidenceId("ev_1".to_owned()),
                EvidenceId("ev_2".to_owned())
            ]
        );
        assert!(!context.text.contains("alias_4"));
    }

    #[test]
    fn context_assembler_can_disable_only_long_term_injection() {
        let mut short = ShortSessionMemory::new(3);
        short.append(
            "把客厅灯调到70%",
            command(Some("living_room_main_light"), Action::SetBrightness),
            Some(TraceId("tr_prev".to_owned())),
        );
        let item = MemoryItem {
            id: "mem_1".to_owned(),
            scope: MemoryScope::Device,
            kind: MemoryKind::DeviceAlias,
            key: "小夜灯".to_owned(),
            value: json!({ "device_id": "hallway_light" }),
            source_evidence_id: evidence_id(),
            confidence_milli: 1000,
            created_at: OffsetDateTime::now_utc(),
            expires_at: None,
            safety_effect: SafetyEffect::Neutral,
        };
        let assembler = ContextAssembler::new(ContextAssemblerConfig {
            memory_enabled: true,
            max_short_memory_turns: 1,
            max_long_term_items: 3,
            max_context_chars: 500,
            include_long_term: false,
        });

        let context = assembler.assemble(&short, &[item]);

        assert!(context.text.contains("last_target"));
        assert_eq!(context.short_turns_used, 1);
        assert_eq!(context.long_items_used, 0);
        assert!(context.evidence_refs.is_empty());
    }
}
