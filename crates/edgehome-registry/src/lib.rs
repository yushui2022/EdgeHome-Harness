//! Device registry, capability model, and state cache for EdgeHome Harness.
//!
//! The model must not know backend tokens, entity ids, or backend routing rules.
//! This crate resolves user-facing aliases into controlled device records and
//! validates whether a normalized command is even possible for that device.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use edgehome_core::{
    Action, CommandParams, DeviceId, DeviceType, NormalizedCommand, RiskLevel, Room,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::{Duration, OffsetDateTime};

pub type RegistryResult<T> = Result<T, RegistryError>;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("failed to read registry config `{path}`: {source}")]
    ReadConfig {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse registry config `{path}`: {source}")]
    ParseConfig {
        path: PathBuf,
        source: serde_yaml::Error,
    },

    #[error("unknown device alias: {0}")]
    UnknownAlias(String),

    #[error("unknown device id: {0}")]
    UnknownDevice(String),

    #[error("unsupported device type in capability config: {0}")]
    UnsupportedDeviceType(String),

    #[error("device `{device_id}` has type `{actual}`, command expects `{expected}`")]
    DeviceTypeMismatch {
        device_id: String,
        actual: String,
        expected: String,
    },

    #[error("unsupported capability: {device_type}.{action}")]
    UnsupportedCapability { device_type: String, action: String },

    #[error("command has no resolved device_id")]
    MissingDeviceId,

    #[error("brightness is outside capability range: {value}")]
    BrightnessOutOfRange { value: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Mock,
    HomeAssistant,
    MiioLocal,
    Mqtt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: DeviceId,
    pub aliases: Vec<String>,
    pub room: Room,
    pub device_type: DeviceType,
    pub backend: BackendKind,
    pub backend_entity_id: String,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityRule {
    pub action: Action,
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub devices: Vec<DeviceRecord>,
    pub capabilities: BTreeMap<String, Vec<CapabilityRule>>,
}

#[derive(Debug, Clone)]
pub struct DeviceRegistry {
    devices: Vec<DeviceRecord>,
    alias_index: HashMap<String, usize>,
    device_id_index: HashMap<DeviceId, usize>,
    capabilities: BTreeMap<String, Vec<CapabilityRule>>,
}

impl DeviceRegistry {
    pub fn load_from_path(path: impl AsRef<Path>) -> RegistryResult<Self> {
        let path = path.as_ref();
        let content =
            std::fs::read_to_string(path).map_err(|source| RegistryError::ReadConfig {
                path: path.to_path_buf(),
                source,
            })?;
        Self::from_yaml_str_with_path(&content, path)
    }

    pub fn from_yaml_str(content: &str) -> RegistryResult<Self> {
        Self::from_yaml_str_with_path(content, Path::new("<inline>"))
    }

    pub fn alias_resolver(&self) -> DeviceAliasResolver<'_> {
        DeviceAliasResolver { registry: self }
    }

    pub fn capability_resolver(&self) -> CapabilityResolver<'_> {
        CapabilityResolver { registry: self }
    }

    pub fn resolve_alias(&self, alias: &str) -> RegistryResult<&DeviceRecord> {
        let normalized_alias = normalize_alias(alias);
        self.alias_index
            .get(&normalized_alias)
            .and_then(|index| self.devices.get(*index))
            .ok_or_else(|| RegistryError::UnknownAlias(alias.to_owned()))
    }

    pub fn get_device(&self, device_id: &DeviceId) -> RegistryResult<&DeviceRecord> {
        self.device_id_index
            .get(device_id)
            .and_then(|index| self.devices.get(*index))
            .ok_or_else(|| RegistryError::UnknownDevice(device_id.0.clone()))
    }

    pub fn validate_capability(
        &self,
        command: &NormalizedCommand,
    ) -> RegistryResult<CapabilityCheck> {
        let device_id = command
            .device_id
            .as_ref()
            .ok_or(RegistryError::MissingDeviceId)?;
        let device = self.get_device(device_id)?;

        if device.device_type != command.device_type {
            return Err(RegistryError::DeviceTypeMismatch {
                device_id: device.device_id.0.clone(),
                actual: device_type_key(&device.device_type).to_owned(),
                expected: device_type_key(&command.device_type).to_owned(),
            });
        }

        let device_type = device_type_key(&command.device_type);
        let action = action_key(&command.action);
        let rule = self
            .capabilities
            .get(device_type)
            .and_then(|rules| rules.iter().find(|rule| rule.action == command.action))
            .ok_or_else(|| RegistryError::UnsupportedCapability {
                device_type: device_type.to_owned(),
                action: action.to_owned(),
            })?;

        validate_params(rule, &command.params)?;

        Ok(CapabilityCheck {
            device_id: device.device_id.clone(),
            device_type: command.device_type.clone(),
            action: command.action.clone(),
            accepted: true,
        })
    }

    fn from_yaml_str_with_path(content: &str, path: &Path) -> RegistryResult<Self> {
        let config: RegistryConfig =
            serde_yaml::from_str(content).map_err(|source| RegistryError::ParseConfig {
                path: path.to_path_buf(),
                source,
            })?;

        for key in config.capabilities.keys() {
            parse_device_type_key(key)?;
        }

        let mut alias_index = HashMap::new();
        let mut device_id_index = HashMap::new();
        for (index, device) in config.devices.iter().enumerate() {
            device_id_index.insert(device.device_id.clone(), index);
            for alias in &device.aliases {
                alias_index.insert(normalize_alias(alias), index);
            }
        }

        Ok(Self {
            devices: config.devices,
            alias_index,
            device_id_index,
            capabilities: config.capabilities,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityCheck {
    pub device_id: DeviceId,
    pub device_type: DeviceType,
    pub action: Action,
    pub accepted: bool,
}

pub struct DeviceAliasResolver<'a> {
    registry: &'a DeviceRegistry,
}

impl<'a> DeviceAliasResolver<'a> {
    pub fn resolve(&self, alias: &str) -> RegistryResult<&'a DeviceRecord> {
        self.registry.resolve_alias(alias)
    }
}

pub struct CapabilityResolver<'a> {
    registry: &'a DeviceRegistry,
}

impl CapabilityResolver<'_> {
    pub fn validate(&self, command: &NormalizedCommand) -> RegistryResult<CapabilityCheck> {
        self.registry.validate_capability(command)
    }
}

pub trait DeviceStateProvider {
    fn state(&self, device_id: &DeviceId) -> Option<DeviceStateSnapshot>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceStateSnapshot {
    pub device_id: DeviceId,
    pub state_json: Value,
    #[serde(with = "time::serde::rfc3339")]
    pub observed_at: OffsetDateTime,
    pub stale_after_sec: i64,
    pub expired_after_sec: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateFreshness {
    Fresh,
    Stale,
    Expired,
    Unknown,
}

#[derive(Debug, Default, Clone)]
pub struct StateCache {
    states: HashMap<DeviceId, DeviceStateSnapshot>,
}

impl StateCache {
    pub fn insert(&mut self, snapshot: DeviceStateSnapshot) {
        self.states.insert(snapshot.device_id.clone(), snapshot);
    }

    pub fn freshness_at(&self, device_id: &DeviceId, now: OffsetDateTime) -> StateFreshness {
        let Some(snapshot) = self.state(device_id) else {
            return StateFreshness::Unknown;
        };
        let age = now - snapshot.observed_at;
        if age <= Duration::seconds(snapshot.stale_after_sec) {
            StateFreshness::Fresh
        } else if age <= Duration::seconds(snapshot.expired_after_sec) {
            StateFreshness::Stale
        } else {
            StateFreshness::Expired
        }
    }
}

impl DeviceStateProvider for StateCache {
    fn state(&self, device_id: &DeviceId) -> Option<DeviceStateSnapshot> {
        self.states.get(device_id).cloned()
    }
}

pub type MockStateProvider = StateCache;

fn validate_params(rule: &CapabilityRule, params: &CommandParams) -> RegistryResult<()> {
    if rule.action == Action::SetBrightness {
        if let Some(brightness) = params.brightness {
            let value = i64::from(brightness);
            if rule.min.is_some_and(|min| value < min) || rule.max.is_some_and(|max| value > max) {
                return Err(RegistryError::BrightnessOutOfRange { value: brightness });
            }
        }
    }
    Ok(())
}

fn normalize_alias(alias: &str) -> String {
    alias.split_whitespace().collect::<String>().to_lowercase()
}

fn parse_device_type_key(value: &str) -> RegistryResult<DeviceType> {
    match value {
        "light" => Ok(DeviceType::Light),
        "air_conditioner" => Ok(DeviceType::AirConditioner),
        "curtain" => Ok(DeviceType::Curtain),
        "switch" => Ok(DeviceType::Switch),
        "camera" => Ok(DeviceType::Camera),
        "lock" => Ok(DeviceType::Lock),
        "sensor" => Ok(DeviceType::Sensor),
        "gas_device" => Ok(DeviceType::GasDevice),
        "unknown" => Ok(DeviceType::Unknown),
        other => Err(RegistryError::UnsupportedDeviceType(other.to_owned())),
    }
}

fn device_type_key(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::Light => "light",
        DeviceType::AirConditioner => "air_conditioner",
        DeviceType::Curtain => "curtain",
        DeviceType::Switch => "switch",
        DeviceType::Camera => "camera",
        DeviceType::Lock => "lock",
        DeviceType::Sensor => "sensor",
        DeviceType::GasDevice => "gas_device",
        DeviceType::Unknown => "unknown",
    }
}

fn action_key(action: &Action) -> &'static str {
    match action {
        Action::TurnOn => "turn_on",
        Action::TurnOff => "turn_off",
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

#[cfg(test)]
mod tests {
    use super::*;
    use edgehome_core::{CommandSchemaVersion, Intent};
    use serde_json::json;

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

    fn command(
        device_id: &str,
        device_type: DeviceType,
        action: Action,
        params: CommandParams,
    ) -> NormalizedCommand {
        NormalizedCommand {
            schema_version: CommandSchemaVersion::default(),
            intent: Intent::ControlDevice,
            room: Room::LivingRoom,
            device_id: Some(DeviceId::new(device_id).expect("device id")),
            device_type,
            action,
            params,
            risk: RiskLevel::Low,
        }
    }

    #[test]
    fn living_room_light_alias_resolves_to_device_id() {
        let registry = load_registry();
        let device = registry.alias_resolver().resolve("客厅灯").expect("device");

        assert_eq!(device.device_id.0, "living_room_main_light");
    }

    #[test]
    fn unknown_device_alias_is_rejected() {
        let registry = load_registry();
        let error = registry
            .alias_resolver()
            .resolve("不存在设备")
            .expect_err("unknown alias");

        assert!(matches!(error, RegistryError::UnknownAlias(_)));
    }

    #[test]
    fn light_cannot_set_temperature() {
        let registry = load_registry();
        let command = command(
            "living_room_main_light",
            DeviceType::Light,
            Action::SetTemperature,
            CommandParams {
                temperature: Some(26),
                ..CommandParams::default()
            },
        );

        let error = registry
            .capability_resolver()
            .validate(&command)
            .expect_err("unsupported capability");

        assert!(matches!(error, RegistryError::UnsupportedCapability { .. }));
    }

    #[test]
    fn brightness_above_capability_range_is_rejected() {
        let registry = load_registry();
        let command = command(
            "hallway_light",
            DeviceType::Light,
            Action::SetBrightness,
            CommandParams {
                brightness: Some(150),
                ..CommandParams::default()
            },
        );

        let error = registry
            .capability_resolver()
            .validate(&command)
            .expect_err("brightness out of range");

        assert!(matches!(
            error,
            RegistryError::BrightnessOutOfRange { value: 150 }
        ));
    }

    #[test]
    fn supported_light_brightness_is_accepted() {
        let registry = load_registry();
        let command = command(
            "hallway_light",
            DeviceType::Light,
            Action::SetBrightness,
            CommandParams {
                brightness: Some(30),
                ..CommandParams::default()
            },
        );

        let check = registry
            .capability_resolver()
            .validate(&command)
            .expect("capability accepted");

        assert!(check.accepted);
    }

    #[test]
    fn state_freshness_covers_fresh_stale_expired_unknown() {
        let device_id = DeviceId::new("living_room_main_light").expect("device id");
        let observed_at = OffsetDateTime::now_utc();
        let mut cache = StateCache::default();
        cache.insert(DeviceStateSnapshot {
            device_id: device_id.clone(),
            state_json: json!({ "power": "on" }),
            observed_at,
            stale_after_sec: 30,
            expired_after_sec: 60,
        });

        assert_eq!(
            cache.freshness_at(&device_id, observed_at + Duration::seconds(10)),
            StateFreshness::Fresh
        );
        assert_eq!(
            cache.freshness_at(&device_id, observed_at + Duration::seconds(40)),
            StateFreshness::Stale
        );
        assert_eq!(
            cache.freshness_at(&device_id, observed_at + Duration::seconds(70)),
            StateFreshness::Expired
        );
        assert_eq!(
            cache.freshness_at(
                &DeviceId::new("unknown_device").expect("device id"),
                observed_at
            ),
            StateFreshness::Unknown
        );
    }
}
