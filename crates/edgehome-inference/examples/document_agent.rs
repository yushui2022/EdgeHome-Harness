//! Hardware-free document tool demo; optionally connect any configured engine.
//! --execute-demo permits only this example's in-memory, read-only tool.
use edgehome_inference::{HttpProvider, InferenceConfig};
use edgehome_runtime::{
    Agent, AgentConfig, ModelError, ModelLimits, ModelProvider, ModelRequest, Step, ToolRegistry,
    TypedTool,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum DocumentId {
    Sample,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DocumentArgs {
    document: DocumentId,
}

struct DemoProvider;
struct InspectableProvider {
    inner: Box<dyn ModelProvider>,
    trace: bool,
}
impl ModelProvider for InspectableProvider {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError> {
        if self.trace {
            eprintln!(
                "model request: {}",
                serde_json::to_string(request).map_err(|_| ModelError::Configuration)?
            );
        }
        let output = self.inner.complete(request, limits)?;
        if self.trace {
            eprintln!("model candidate: {output}");
        }
        Ok(output)
    }
}
impl ModelProvider for DemoProvider {
    fn complete(&mut self, request: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
        if let Some(result) = request
            .messages
            .last()
            .and_then(|m| serde_json::from_str::<Value>(&m.content).ok())
            .and_then(|v| v.get("tool_result").cloned())
        {
            Ok(json!({"kind":"final","message":format!("Document statistics: {}",result["output"])}).to_string())
        } else {
            Ok(
                json!({"kind":"call","tool":"document.stats","arguments":{"document":"sample"}})
                    .to_string(),
            )
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    let mut execute = false;
    let mut result_only = false;
    let mut trace = false;
    let mut prompt = "Use document.stats for document sample, then report the character and line counts from its result.".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config_path = Some(args.next().ok_or("--config needs a YAML path")?),
            "--prompt" => prompt = args.next().ok_or("--prompt needs text")?,
            "--execute-demo" => execute = true,
            "--result-only" => result_only = true,
            "--trace-model" => trace = true,
            "--help" => {
                println!(
                    "document_agent [--config FILE.yaml] [--prompt TEXT] [--execute-demo] [--result-only] [--trace-model]\nDefault: deterministic provider, preview only. The only tool reads an in-memory sample. --result-only stops after the tool receipt; no model-written final answer. --trace-model prints requests and raw candidates (including prompt and tool data) to stderr."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let mut runtime_config = AgentConfig {
        execute_enabled: execute,
        require_tool_observation: true,
        max_tool_calls: 1,
        ..AgentConfig::default()
    };
    let provider: Box<dyn ModelProvider> = if let Some(path) = config_path {
        let config: InferenceConfig = serde_yaml::from_str(&std::fs::read_to_string(path)?)?;
        runtime_config.model_timeout = std::time::Duration::from_millis(config.request_timeout_ms);
        Box::new(HttpProvider::new(config)?)
    } else {
        Box::new(DemoProvider)
    };

    let mut tools = ToolRegistry::new();
    tools.register(TypedTool::<DocumentArgs, _, _>::new(
        "document.stats", "Read-only character and line counts. Call with arguments {\"document\":\"sample\"}; use the returned counts in your answer.",
        |_| Ok(()),
        |args, _| {
            let document = match args.document {
                DocumentId::Sample => "EdgeHome is a local agent development toolkit.\nTools belong to the host application.",
            };
            Ok(json!({"characters":document.chars().count(),"lines":document.lines().count()}))
        },
    ))?;
    let mut agent = Agent::new(
        InspectableProvider {
            inner: provider,
            trace,
        },
        tools,
        runtime_config,
    )?;
    let mut step = agent.prompt(&prompt)?;
    loop {
        println!("{}", serde_json::to_string_pretty(&step)?);
        match step {
            Step::Final { .. } | Step::Clarification { .. } => break,
            Step::Proposal { plan } if execute => {
                // Explicit command-line opt-in authorizes ONLY this immutable
                // document fixture. Real hosts must bind approval to their UI.
                let receipt = agent.execute(&plan.plan_id, true)?;
                println!("{}", serde_json::to_string_pretty(&receipt)?);
                if result_only || !receipt.continuation_available {
                    break;
                }
                step = agent.continue_run()?;
            }
            Step::Proposal { .. } => break,
        }
    }
    Ok(())
}
