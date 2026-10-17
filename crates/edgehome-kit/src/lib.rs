//! Small host entry points shared by the developer CLI and generated apps.
//! Business tools stay in the application; this crate never loads arbitrary code.
mod config;
mod eval;
pub mod files;
mod journal;
mod model_trace;
mod probe;
pub mod rpc;
mod scaffold;
#[path = "../templates/tools.rs"]
pub mod starter;

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
pub use config::{AppConfig, RuntimeSettings};
use edgehome_inference::HttpProvider;
use edgehome_runtime::{Agent, ExecutionStatus, ModelLimits, ModelProvider, Step, ToolRegistry};
pub use journal::SessionJournal;
#[cfg(test)]
use serde_json::Value;
use serde_json::json;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Parser)]
#[command(
    about = "Build and host a focused local agent",
    version,
    long_version = concat!(env!("CARGO_PKG_VERSION"), "\nDefault SDK commit: ", include_str!("../sdk-revision.txt"))
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct ModelOptions {
    #[arg(long, help = "Application config; defaults to agent.yaml if present")]
    config: Option<PathBuf>,
    #[arg(
        long,
        help = "Explicitly use the application's deterministic fixture provider"
    )]
    mock: bool,
}
#[derive(Args)]
struct LogOptions {
    #[arg(
        long,
        help = "Explicit content trace to a NEW file (2 MiB); includes prompts and responses"
    )]
    model_trace: Option<PathBuf>,
    #[arg(long, help = "Write lifecycle metadata to stderr as NDJSON")]
    events: bool,
    #[arg(
        long,
        help = "Create a new metadata journal (512 KiB maximum; no execution restore)"
    )]
    journal: Option<PathBuf>,
}
#[derive(Subcommand)]
enum Command {
    /// Generate an independent Cargo app with two replaceable tools.
    Init {
        directory: PathBuf,
        #[arg(
            long,
            conflicts_with = "sdk_rev",
            help = "Local SDK checkout (development only; not portable)"
        )]
        sdk_path: Option<PathBuf>,
        #[arg(
            long,
            help = "Published 40-character SDK commit; defaults to the bundled compatibility baseline"
        )]
        sdk_rev: Option<String>,
        #[arg(
            long,
            help = "Resolve the lockfile using only cached Git sources and dependencies"
        )]
        offline: bool,
        #[arg(long, default_value = "my-edge-agent")]
        name: String,
    },
    /// Validate local configuration; --probe checks two system-only instructions.
    Doctor {
        #[command(flatten)]
        model: ModelOptions,
        #[arg(long)]
        probe: bool,
    },
    /// List the application's registered tools and schemas.
    Tools,
    /// Propose a task. Only --allow tools can execute; default is one tool result.
    Run {
        #[command(flatten)]
        model: ModelOptions,
        #[command(flatten)]
        log: LogOptions,
        #[arg(long)]
        prompt: String,
        #[arg(
            long = "tool",
            help = "Expose only this tool; repeat to select multiple"
        )]
        tools: Vec<String>,
        #[arg(
            long,
            help = "Preauthorize this tool for this invocation; repeat as needed"
        )]
        allow: Vec<String>,
        #[arg(
            long = "loop",
            help = "Continue model turns after successful tool observations"
        )]
        continue_loop: bool,
    },
    /// Serve bounded, sequential NDJSON on stdin/stdout (protocol v1).
    Rpc {
        #[command(flatten)]
        model: ModelOptions,
        #[command(flatten)]
        log: LogOptions,
        #[arg(long)]
        enable_execution: bool,
    },
    /// Measure tool selection, typed arguments and results against fixed cases.
    Eval {
        #[command(flatten)]
        model: ModelOptions,
        #[arg(long)]
        cases: PathBuf,
        #[arg(long)]
        allow: Vec<String>,
        #[arg(long, default_value_t = 1)]
        repeat: u32,
        #[arg(long, default_value_t = 42)]
        order_seed: u64,
        #[arg(
            long,
            help = "Include raw model content and receipts; use only with reviewable test data"
        )]
        include_responses: bool,
    },
}

pub(crate) fn provider(
    config: &AppConfig,
    is_mock: bool,
    mock: &impl Fn() -> Box<dyn ModelProvider>,
) -> Result<Box<dyn ModelProvider>> {
    if is_mock {
        return Ok(mock());
    }
    let inference = config.inference.clone().ok_or_else(|| {
        anyhow::anyhow!("configure inference in agent.yaml or explicitly select --mock")
    })?;
    Ok(Box::new(HttpProvider::new(inference)?))
}

fn checked_allow(registry: &ToolRegistry, allowed: &[String]) -> Result<BTreeSet<String>> {
    let names: BTreeSet<_> = registry.definitions().map(|d| d.name.clone()).collect();
    if allowed.iter().any(|n| !names.contains(n)) {
        bail!("--allow names an unregistered tool");
    }
    Ok(allowed.iter().cloned().collect())
}

pub fn run_cli(
    registry: impl Fn() -> Result<ToolRegistry>,
    mock: impl Fn() -> Box<dyn ModelProvider>,
) -> Result<()> {
    match Cli::parse().command {
        Command::Init {
            directory,
            sdk_path,
            sdk_rev,
            offline,
            name,
        } => {
            let path =
                scaffold::create(&directory, sdk_path.as_deref(), sdk_rev.as_deref(), &name)?;
            scaffold::lock_dependencies(&path, offline)?;
            println!(
                "Created {} with Cargo.lock.\nNext: cd into the project and run `cargo run --locked -- doctor --mock`.\nCommit Cargo.toml, Cargo.lock and rust-toolchain.toml with your application.",
                path.display()
            );
        }
        Command::Tools => rpc::write_json(
            &mut std::io::stdout(),
            &json!({"tools":registry()?.definitions().collect::<Vec<_>>()}),
        )?,
        Command::Doctor { model, probe } => {
            let config = AppConfig::load(model.config.as_deref())?;
            let tools = registry()?;
            let names: Vec<_> = tools.definitions().map(|d| d.name.clone()).collect();
            let model_provider = provider(&config, model.mock, &mock)?;
            Agent::new(model_provider, tools, config.agent_config(false))?;
            let mut probe_status = "not_requested";
            if probe && !model.mock {
                probe::check(
                    provider(&config, false, &mock)?.as_mut(),
                    ModelLimits {
                        timeout: config.agent_config(false).model_timeout,
                        max_output_bytes: 4096,
                        max_request_bytes: 16384,
                    },
                )?;
                probe_status = "passed";
            } else if probe {
                probe_status = "skipped_mock";
            }
            rpc::write_json(
                &mut std::io::stdout(),
                &json!({"status":"ready","mode":if model.mock {"mock"} else {"model"},"tools":names,"probe":probe_status,"note":"Configuration readiness does not establish task accuracy or hardware performance."}),
            )?;
        }
        Command::Run {
            model,
            log,
            prompt,
            tools,
            allow,
            continue_loop,
        } => {
            let config = AppConfig::load(model.config.as_deref())?;
            let registry = registry()?;
            let allowed = checked_allow(&registry, &allow)?;
            let mut agent = Agent::with_journal(
                model_trace::ModelTrace::wrap(
                    provider(&config, model.mock, &mock)?,
                    log.model_trace.as_deref(),
                )?,
                registry,
                config.agent_config(!allowed.is_empty()),
                SessionJournal::new(log.journal.as_deref(), log.events)?,
            )?;
            let mut step = if tools.is_empty() {
                agent.prompt(&prompt)?
            } else {
                agent.prompt_with_tools(
                    &prompt,
                    &tools.iter().map(String::as_str).collect::<Vec<_>>(),
                )?
            };
            let mut output = std::io::stdout();
            loop {
                rpc::write_json(&mut output, &json!({"type":"step","step":step}))?;
                match step {
                    Step::Proposal { plan } if allowed.contains(&plan.tool) => {
                        let receipt = agent.execute(&plan.plan_id, true)?;
                        rpc::write_json(&mut output, &json!({"type":"receipt","receipt":receipt}))?;
                        if receipt.status != ExecutionStatus::Succeeded
                            || !receipt.completion_recorded
                        {
                            bail!(
                                "tool result or completion record is uncertain; reconcile before another action"
                            );
                        }
                        if !continue_loop || !receipt.continuation_available {
                            break;
                        }
                        step = agent.continue_run()?;
                    }
                    _ => break,
                }
            }
        }
        Command::Rpc {
            model,
            log,
            enable_execution,
        } => {
            let config = AppConfig::load(model.config.as_deref())?;
            let tools = registry()?;
            let definitions = tools.definitions().cloned().collect::<Vec<_>>();
            let mut agent = Agent::with_journal(
                model_trace::ModelTrace::wrap(
                    provider(&config, model.mock, &mock)?,
                    log.model_trace.as_deref(),
                )?,
                tools,
                config.agent_config(enable_execution),
                SessionJournal::new(log.journal.as_deref(), log.events)?,
            )?;
            rpc::serve(
                &mut agent,
                &definitions,
                &mut std::io::stdin().lock(),
                &mut std::io::stdout(),
            )?;
        }
        Command::Eval {
            model,
            cases,
            allow,
            repeat,
            order_seed,
            include_responses,
        } => {
            let config = AppConfig::load(model.config.as_deref())?;
            let allowed = checked_allow(&registry()?, &allow)?;
            let report = eval::evaluate(
                &cases,
                &config,
                model.mock,
                &allowed,
                &registry,
                &mock,
                eval::EvalOptions {
                    repeat,
                    order_seed,
                    include_responses,
                },
            )?;
            rpc::write_json(&mut std::io::stdout(), &json!(report))?;
            if report.passed != report.total {
                bail!("task evaluation failed; inspect per-case outcomes");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod file_tests;
#[cfg(test)]
mod reliability_tests;
#[cfg(test)]
mod tests;
