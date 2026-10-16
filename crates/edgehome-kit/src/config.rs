use anyhow::{Result, bail};
use edgehome_inference::InferenceConfig;
use edgehome_runtime::AgentConfig;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path, time::Duration};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub inference: Option<InferenceConfig>,
    pub runtime: RuntimeSettings,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeSettings {
    pub decision_protocol: edgehome_runtime::DecisionProtocol,
    pub max_decision_repairs: u32,
    pub max_clarification_turns: u32,
    pub max_model_calls: u32,
    pub max_tool_calls: u32,
    pub max_context_bytes: usize,
    pub max_tool_output_bytes: usize,
    pub require_tool_observation: bool,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            decision_protocol: edgehome_runtime::DecisionProtocol::Tagged,
            max_decision_repairs: 0,
            max_clarification_turns: 4,
            max_model_calls: 8,
            max_tool_calls: 4,
            max_context_bytes: 16384,
            max_tool_output_bytes: 4096,
            require_tool_observation: true,
        }
    }
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let default_path = Path::new("agent.yaml");
        let path = path.or_else(|| default_path.exists().then_some(default_path));
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let bytes = read_bounded(path, 65536)?;
        // YAML diagnostics can include source text. Do not echo configuration
        // contents, URLs, or accidentally pasted credentials into errors.
        serde_yaml::from_slice(&bytes).map_err(|_| {
            anyhow::anyhow!("invalid agent configuration; check field names and YAML types")
        })
    }

    pub fn agent_config(&self, execute_enabled: bool) -> AgentConfig {
        AgentConfig {
            decision_protocol: self.runtime.decision_protocol,
            max_decision_repairs: self.runtime.max_decision_repairs,
            max_clarification_turns: self.runtime.max_clarification_turns,
            max_model_calls: self.runtime.max_model_calls,
            max_tool_calls: self.runtime.max_tool_calls,
            max_context_bytes: self.runtime.max_context_bytes,
            max_tool_output_bytes: self.runtime.max_tool_output_bytes,
            require_tool_observation: self.runtime.require_tool_observation,
            model_timeout: self
                .inference
                .as_ref()
                .map(|c| Duration::from_millis(c.request_timeout_ms))
                .unwrap_or(Duration::from_secs(15)),
            execute_enabled,
            ..AgentConfig::default()
        }
    }
}

pub(crate) fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|_| anyhow::anyhow!("cannot open requested input file"))?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        bail!("input file exceeds byte limit");
    }
    Ok(bytes)
}
