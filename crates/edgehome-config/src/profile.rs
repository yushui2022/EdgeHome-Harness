use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileName {
    StrictMode,
    NormalMode,
    #[serde(alias = "low_memory_mode")]
    LowMemory,
    EvalMode,
    DemoMode,
}

impl ProfileName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrictMode => "strict_mode",
            Self::NormalMode => "normal_mode",
            Self::LowMemory => "low_memory",
            Self::EvalMode => "eval_mode",
            Self::DemoMode => "demo_mode",
        }
    }

    pub fn file_name(self) -> String {
        format!("{}.yaml", self.as_str())
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProfileName {
    type Err = ConfigError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "strict" | "strict_mode" => Ok(Self::StrictMode),
            "normal" | "normal_mode" => Ok(Self::NormalMode),
            "low_memory" | "low_memory_mode" => Ok(Self::LowMemory),
            "eval" | "eval_mode" => Ok(Self::EvalMode),
            "demo" | "demo_mode" => Ok(Self::DemoMode),
            other => Err(ConfigError::UnknownProfile(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorBackend {
    Mock,
    HomeAssistant,
    MiioLocal,
    Mqtt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DangerousActionPolicy {
    Deny,
    RequireConfirmation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeProfile {
    pub name: ProfileName,
    pub model_name: String,
    pub ollama_base_url: String,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub num_ctx: u32,
    pub num_predict: u32,
    pub timeout_ms: u64,
    pub retry_count: u8,
    pub memory_enabled: bool,
    pub max_short_memory_turns: u8,
    pub max_context_chars: usize,
    pub executor_backend: ExecutorBackend,
    pub dangerous_action_policy: DangerousActionPolicy,
    pub audit_enabled: bool,
    pub trace_enabled: bool,
}

impl RuntimeProfile {
    pub fn low_memory() -> Self {
        Self {
            name: ProfileName::LowMemory,
            model_name: "openbmb/minicpm5:1b".to_owned(),
            ollama_base_url: "http://127.0.0.1:11434".to_owned(),
            temperature: 0.1,
            top_p: 0.8,
            top_k: 20,
            repeat_penalty: 1.25,
            num_ctx: 1024,
            num_predict: 128,
            timeout_ms: 8_000,
            retry_count: 1,
            memory_enabled: true,
            max_short_memory_turns: 3,
            max_context_chars: 500,
            executor_backend: ExecutorBackend::Mock,
            dangerous_action_policy: DangerousActionPolicy::Deny,
            audit_enabled: true,
            trace_enabled: true,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.model_name.trim().is_empty() {
            return self.invalid("model_name cannot be empty");
        }
        if self.ollama_base_url.trim().is_empty() {
            return self.invalid("ollama_base_url cannot be empty");
        }
        if !(0.0..=1.0).contains(&self.temperature) {
            return self.invalid("temperature must be between 0.0 and 1.0");
        }
        if !(0.0..=1.0).contains(&self.top_p) {
            return self.invalid("top_p must be between 0.0 and 1.0");
        }
        if self.top_k == 0 {
            return self.invalid("top_k must be greater than 0");
        }
        if self.repeat_penalty < 1.0 {
            return self.invalid("repeat_penalty must be at least 1.0");
        }
        if self.num_ctx == 0 {
            return self.invalid("num_ctx must be greater than 0");
        }
        if self.num_predict == 0 {
            return self.invalid("num_predict must be greater than 0");
        }
        if self.timeout_ms == 0 {
            return self.invalid("timeout_ms must be greater than 0");
        }
        if self.max_context_chars == 0 {
            return self.invalid("max_context_chars must be greater than 0");
        }

        if self.name == ProfileName::LowMemory {
            if self.num_ctx > 1024 {
                return self.invalid("low_memory num_ctx must be <= 1024");
            }
            if self.num_predict > 128 {
                return self.invalid("low_memory num_predict must be <= 128");
            }
            if self.max_short_memory_turns > 3 {
                return self.invalid("low_memory max_short_memory_turns must be <= 3");
            }
            if self.max_context_chars > 500 {
                return self.invalid("low_memory max_context_chars must be <= 500");
            }
            if self.retry_count > 1 {
                return self.invalid("low_memory retry_count must be <= 1");
            }
            if self.executor_backend != ExecutorBackend::Mock {
                return self.invalid("low_memory executor_backend must default to mock");
            }
        }

        Ok(())
    }

    fn invalid<T>(&self, message: impl Into<String>) -> Result<T, ConfigError> {
        Err(ConfigError::Validation {
            profile: self.name.to_string(),
            message: message.into(),
        })
    }
}

pub fn load_profile(
    config_dir: impl AsRef<Path>,
    profile: impl AsRef<str>,
) -> Result<RuntimeProfile, ConfigError> {
    let profile = ProfileName::from_str(profile.as_ref())?;
    load_profile_from_path(config_dir.as_ref().join(profile.file_name()))
}

pub fn load_profile_from_path(path: impl AsRef<Path>) -> Result<RuntimeProfile, ConfigError> {
    let path = path.as_ref();
    let content = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let profile: RuntimeProfile =
        serde_yaml::from_str(&content).map_err(|source| ConfigError::Parse {
            path: PathBuf::from(path),
            source,
        })?;

    profile.validate()?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_config_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("configs")
    }

    fn workspace_config(path: &str) -> PathBuf {
        workspace_config_dir().join(path)
    }

    #[test]
    fn parses_low_memory_aliases() {
        assert_eq!(
            ProfileName::from_str("low_memory").expect("profile"),
            ProfileName::LowMemory
        );
        assert_eq!(
            ProfileName::from_str("low_memory_mode").expect("profile"),
            ProfileName::LowMemory
        );
    }

    #[test]
    fn low_memory_default_is_conservative() {
        let profile = RuntimeProfile::low_memory();
        assert_eq!(profile.executor_backend, ExecutorBackend::Mock);
        assert_eq!(profile.dangerous_action_policy, DangerousActionPolicy::Deny);
        assert!(profile.num_ctx <= 1024);
        assert!(profile.num_predict <= 128);
        assert!(profile.max_short_memory_turns <= 3);
        assert!(profile.max_context_chars <= 500);
        assert!(profile.retry_count <= 1);
        profile.validate().expect("valid low memory profile");
    }

    #[test]
    fn loads_low_memory_profile_from_configs_dir() {
        let profile =
            load_profile_from_path(workspace_config("low_memory.yaml")).expect("load profile");
        assert_eq!(profile.name, ProfileName::LowMemory);
        assert_eq!(profile.executor_backend, ExecutorBackend::Mock);
        assert_eq!(profile.dangerous_action_policy, DangerousActionPolicy::Deny);
        assert!(profile.trace_enabled);
        assert!(profile.audit_enabled);
    }

    #[test]
    fn load_profile_uses_profile_file_name() {
        let profile = load_profile(workspace_config_dir(), "low_memory").expect("load low_memory");
        assert_eq!(profile.name, ProfileName::LowMemory);
    }

    #[test]
    fn missing_config_has_clear_error() {
        let error = load_profile_from_path(workspace_config("missing.yaml")).expect_err("missing");
        let message = error.to_string();
        assert!(message.contains("failed to read config"));
        assert!(message.contains("missing.yaml"));
    }

    #[test]
    fn invalid_low_memory_profile_is_rejected() {
        let mut profile = RuntimeProfile::low_memory();
        profile.num_ctx = 4096;
        let error = profile.validate().expect_err("invalid profile");
        assert!(
            error
                .to_string()
                .contains("low_memory num_ctx must be <= 1024")
        );
    }
}
