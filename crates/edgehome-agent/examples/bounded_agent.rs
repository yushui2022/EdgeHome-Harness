//! Run with `cargo run -p edgehome-agent --example bounded_agent`.
//! Add `-- --execute-mock` to explicitly dispatch to the simulated executor.
use edgehome_agent::{AgentConfig, AgentRuntime, CandidateProvider, ModelBudget, ProviderError};
use edgehome_core::UserInput;
use edgehome_executor::MockExecutor;
use edgehome_parser::RulePreParser;
use edgehome_registry::{DeviceRegistry, StateFreshness};
use edgehome_trace::TraceStore;

/// A deterministic stand-in, not an AI model. Replace this implementation with
/// your local model adapter; enforce the supplied transport budget there.
struct RuleCandidateProvider;

impl CandidateProvider for RuleCandidateProvider {
    fn propose(&mut self, input: &UserInput, budget: ModelBudget) -> Result<String, ProviderError> {
        let candidate = RulePreParser.pre_parse(input).ok_or(ProviderError)?;
        let output = serde_json::to_string(&candidate).map_err(|_| ProviderError)?;
        if output.len() > budget.max_output_bytes {
            return Err(ProviderError);
        }
        Ok(output)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let execute_mock = std::env::args().skip(1).any(|arg| arg == "--execute-mock");
    let registry = DeviceRegistry::from_yaml_str(include_str!("../../../configs/devices.yaml"))?;
    let mut agent = AgentRuntime::new(
        RuleCandidateProvider,
        MockExecutor::new(execute_mock),
        registry,
        TraceStore::open(":memory:")?,
        AgentConfig {
            execute_enabled: execute_mock,
            ..AgentConfig::default()
        },
    )?;

    // Freshness is synthetic ONLY because this is a fully simulated example.
    // Real hosts must read device state and authenticate operator confirmation.
    let preview = agent.propose("把客厅灯打开", StateFreshness::Fresh)?;
    println!("{}", serde_json::to_string_pretty(&preview)?);
    if execute_mock {
        let receipt = agent.execute(preview.trace_id(), true, StateFreshness::Fresh)?;
        println!("{}", serde_json::to_string_pretty(&receipt)?);
        println!(
            "Replay rejected: {}",
            agent
                .execute(preview.trace_id(), true, StateFreshness::Fresh)
                .unwrap_err()
        );
    }

    for input in ["关闭燃气报警器", "晚上十点后把走廊灯调到30%"] {
        println!(
            "{}",
            serde_json::to_string_pretty(&agent.propose(input, StateFreshness::Fresh)?)?
        );
    }
    Ok(())
}
