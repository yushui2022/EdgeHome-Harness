use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HarnessError;

pub const MODEL_OUTPUT_SCHEMA_VERSION: &str = "model_output.v1";
pub const COMMAND_SCHEMA_VERSION: &str = "command.v1";
pub const DEVICE_REGISTRY_SCHEMA_VERSION: &str = "device_registry.v1";
pub const POLICY_SCHEMA_VERSION: &str = "policy.v1";
pub const MEMORY_SCHEMA_VERSION: &str = "memory.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct UserInput {
    pub text: String,
}

impl UserInput {
    pub fn new(text: impl Into<String>) -> Result<Self, HarnessError> {
        let text = text.into().trim().to_owned();
        if text.is_empty() {
            return Err(HarnessError::EmptyInput);
        }

        Ok(Self { text })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ModelOutputSchemaVersion(pub String);

impl Default for ModelOutputSchemaVersion {
    fn default() -> Self {
        Self(MODEL_OUTPUT_SCHEMA_VERSION.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct CommandSchemaVersion(pub String);

impl Default for CommandSchemaVersion {
    fn default() -> Self {
        Self(COMMAND_SCHEMA_VERSION.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn new(value: impl Into<String>) -> Result<Self, HarnessError> {
        let value = value.into().trim().to_owned();
        if value.is_empty() {
            return Err(HarnessError::Validation(
                "device_id cannot be empty".to_owned(),
            ));
        }

        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    ControlDevice,
    QueryStatus,
    CreateRule,
    UpdateMemory,
    Unknown,
}

impl Default for Intent {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Intent {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Room {
    LivingRoom,
    Bedroom,
    Hallway,
    Kitchen,
    Bathroom,
    Entrance,
    Unknown,
}

impl Default for Room {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Room {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Light,
    AirConditioner,
    Curtain,
    Switch,
    Camera,
    Lock,
    Sensor,
    GasDevice,
    Unknown,
}

impl Default for DeviceType {
    fn default() -> Self {
        Self::Unknown
    }
}

impl DeviceType {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    TurnOn,
    TurnOff,
    SetBrightness,
    IncreaseBrightness,
    DecreaseBrightness,
    SetTemperature,
    SetMode,
    Open,
    Close,
    Lock,
    Unlock,
    Unknown,
}

impl Default for Action {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Action {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Read,
    Low,
    Medium,
    High,
    Blocked,
    Unknown,
}

impl Default for RiskLevel {
    fn default() -> Self {
        Self::Unknown
    }
}

impl RiskLevel {
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    RequireConfirmation,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct CommandParams {
    pub brightness: Option<u8>,
    pub temperature: Option<i16>,
    pub mode: Option<String>,
    pub time_after: Option<String>,
    pub raw_value: Option<String>,
}

impl CommandParams {
    pub fn has_out_of_range_brightness(&self) -> bool {
        self.brightness.is_some_and(|brightness| brightness > 100)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ModelCandidate {
    pub schema_version: ModelOutputSchemaVersion,
    pub intent: Intent,
    pub room: Option<Room>,
    pub device_alias: Option<String>,
    pub device_type: DeviceType,
    pub action: Action,
    pub params: CommandParams,
}

impl Default for ModelCandidate {
    fn default() -> Self {
        Self {
            schema_version: ModelOutputSchemaVersion::default(),
            intent: Intent::Unknown,
            room: None,
            device_alias: None,
            device_type: DeviceType::Unknown,
            action: Action::Unknown,
            params: CommandParams::default(),
        }
    }
}

impl ModelCandidate {
    pub fn contains_unknowns(&self) -> bool {
        self.intent.is_unknown() || self.device_type.is_unknown() || self.action.is_unknown()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct NormalizedCommand {
    pub schema_version: CommandSchemaVersion,
    pub intent: Intent,
    pub room: Room,
    pub device_id: Option<DeviceId>,
    pub device_type: DeviceType,
    pub action: Action,
    pub params: CommandParams,
    pub risk: RiskLevel,
}

impl Default for NormalizedCommand {
    fn default() -> Self {
        Self {
            schema_version: CommandSchemaVersion::default(),
            intent: Intent::Unknown,
            room: Room::Unknown,
            device_id: None,
            device_type: DeviceType::Unknown,
            action: Action::Unknown,
            params: CommandParams::default(),
            risk: RiskLevel::Unknown,
        }
    }
}

impl NormalizedCommand {
    pub fn contains_unknowns(&self) -> bool {
        self.intent.is_unknown()
            || self.room.is_unknown()
            || self.device_type.is_unknown()
            || self.action.is_unknown()
            || self.risk.is_unknown()
    }

    pub fn can_enter_policy_gate(&self) -> bool {
        !self.contains_unknowns()
            && self.device_id.is_some()
            && !self.params.has_out_of_range_brightness()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionPlan {
    pub trace_id: Option<String>,
    pub dry_run: bool,
    pub target: DeviceId,
    pub action: Action,
    pub params: CommandParams,
    pub policy: PolicyDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DryRunPlan {
    pub plan: ExecutionPlan,
    pub backend: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionResult {
    pub success: bool,
    pub message: String,
    pub raw_backend_response: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_input_rejects_empty_text() {
        assert_eq!(UserInput::new("   "), Err(HarnessError::EmptyInput));
    }

    #[test]
    fn user_input_trims_text() {
        let input = UserInput::new("  把客厅灯关掉  ").expect("valid input");
        assert_eq!(input.text, "把客厅灯关掉");
    }

    #[test]
    fn model_candidate_defaults_to_unknown_and_fail_closed() {
        let candidate = ModelCandidate::default();
        assert!(candidate.contains_unknowns());
    }

    #[test]
    fn normalized_command_with_unknowns_cannot_enter_policy_gate() {
        let command = NormalizedCommand::default();
        assert!(!command.can_enter_policy_gate());
    }

    #[test]
    fn normalized_command_with_valid_fields_can_enter_policy_gate() {
        let command = NormalizedCommand {
            intent: Intent::ControlDevice,
            room: Room::LivingRoom,
            device_id: Some(DeviceId::new("living_room_main_light").expect("device id")),
            device_type: DeviceType::Light,
            action: Action::TurnOff,
            risk: RiskLevel::Low,
            ..NormalizedCommand::default()
        };

        assert!(command.can_enter_policy_gate());
    }

    #[test]
    fn brightness_above_one_hundred_is_out_of_range() {
        let params = CommandParams {
            brightness: Some(101),
            ..CommandParams::default()
        };

        assert!(params.has_out_of_range_brightness());
    }

    #[test]
    fn action_serializes_as_snake_case() {
        let serialized = serde_json::to_string(&Action::SetBrightness).expect("serialize action");
        assert_eq!(serialized, "\"set_brightness\"");
    }

    #[test]
    fn normalized_command_round_trips_through_json() {
        let command = NormalizedCommand {
            intent: Intent::ControlDevice,
            room: Room::Hallway,
            device_id: Some(DeviceId::new("hallway_light").expect("device id")),
            device_type: DeviceType::Light,
            action: Action::SetBrightness,
            params: CommandParams {
                brightness: Some(30),
                ..CommandParams::default()
            },
            risk: RiskLevel::Low,
            ..NormalizedCommand::default()
        };

        let json = serde_json::to_string(&command).expect("serialize command");
        let decoded: NormalizedCommand = serde_json::from_str(&json).expect("deserialize command");

        assert_eq!(decoded, command);
    }
}
