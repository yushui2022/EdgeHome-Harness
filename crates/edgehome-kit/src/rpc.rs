use anyhow::Result;
use edgehome_runtime::{Agent, AgentError, Journal, ModelProvider, ToolDefinition};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};

pub const MAX_FRAME_BYTES: usize = 8192;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Prompt {
        input: String,
        tools: Option<Vec<String>>,
    },
    Execute {
        plan_id: String,
        #[serde(default)]
        approved: bool,
    },
    Reply {
        clarification_id: String,
        input: String,
    },
    Resolve {
        clarification_id: String,
        values: Value,
    },
    Continue,
    Cancel,
    Status,
    Tools,
}

pub fn error_code(error: &AgentError) -> &'static str {
    match error {
        AgentError::Configuration => "configuration",
        AgentError::Cancelled => "cancelled",
        AgentError::Input => "input_limit",
        AgentError::Budget => "budget_exhausted",
        AgentError::Context => "context_limit",
        AgentError::Model(_) => "model_error",
        AgentError::Decision => "invalid_decision",
        AgentError::ObservationRequired => "observation_required",
        AgentError::Tool(_) => "tool_rejected",
        AgentError::ExecuteDisabled => "execution_disabled",
        AgentError::UnknownPlan => "unknown_plan",
        AgentError::ConfirmationRequired => "confirmation_required",
        AgentError::Expired => "expired_plan",
        AgentError::NoContinuation => "no_continuation",
        AgentError::NoClarification => "no_clarification",
        AgentError::InvalidAnswer => "invalid_answer",
        AgentError::ClarificationRequired => "clarification_required",
        AgentError::Journal(_) => "journal_error",
    }
}

pub fn write_json(writer: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Local, sequential NDJSON protocol, not JSON-RPC 2.0 or a network server.
/// EOF exits without executing a pending proposal. Cancel takes effect when
/// read; use the SDK token from another thread for in-flight cooperative stops.
pub fn serve<P: ModelProvider, J: Journal>(
    agent: &mut Agent<P, J>,
    tools: &[ToolDefinition],
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<()> {
    write_json(
        output,
        &json!({"type":"ready","protocol":1,"max_request_bytes":MAX_FRAME_BYTES,"capabilities":["clarification_reply","clarification_resolve"]}),
    )?;
    loop {
        let mut line = Vec::new();
        let count = (&mut *input)
            .take(MAX_FRAME_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        if count > MAX_FRAME_BYTES {
            write_json(
                output,
                &json!({"type":"error","id":null,"code":"frame_too_large"}),
            )?;
            anyhow::bail!("RPC request exceeded byte limit; connection closed");
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let request = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => request,
            Err(_) => {
                write_json(
                    output,
                    &json!({"type":"error","id":null,"code":"invalid_request"}),
                )?;
                continue;
            }
        };
        let result: std::result::Result<Value, AgentError> = match request.command {
            Command::Prompt {
                input,
                tools: Some(selected),
            } => agent
                .prompt_with_tools(
                    &input,
                    &selected.iter().map(String::as_str).collect::<Vec<_>>(),
                )
                .map(|v| json!(v)),
            Command::Prompt { input, tools: None } => agent.prompt(&input).map(|v| json!(v)),
            Command::Execute { plan_id, approved } => {
                agent.execute(&plan_id, approved).map(|v| json!(v))
            }
            Command::Reply {
                clarification_id,
                input,
            } => agent.reply(&clarification_id, &input).map(|v| json!(v)),
            Command::Resolve {
                clarification_id,
                values,
            } => agent.resolve(&clarification_id, values).map(|v| json!(v)),
            Command::Continue => agent.continue_run().map(|v| json!(v)),
            Command::Cancel => {
                agent.cancellation_token().cancel();
                agent.cancel_pending().map(|_| json!(agent.usage()))
            }
            Command::Status => Ok(json!(agent.usage())),
            Command::Tools => Ok(json!(tools)),
        };
        match result {
            Ok(mut value) => {
                if value["status"] == "clarification" {
                    value["clarification"] = json!(agent.clarification());
                }
                write_json(
                    output,
                    &json!({"type":"result","id":request.id,"result":value}),
                )?;
            }
            Err(error) => write_json(
                output,
                &json!({"type":"error","id":request.id,"code":error_code(&error),"message":error.to_string(),"clarification":agent.clarification()}),
            )?,
        }
    }
}
