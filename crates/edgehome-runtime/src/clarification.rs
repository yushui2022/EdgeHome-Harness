use crate::*;
use std::collections::BTreeSet;

/// A task-local request. Draft arguments came from the model, not a user approval.
/// Only `resolve`'s values are host-supplied; execution still needs plan approval.
#[derive(Debug, Clone, Serialize)]
pub struct Clarification {
    pub clarification_id: String,
    pub message: String,
    pub tool: Option<String>,
    pub draft_arguments: Value,
    pub fields: Vec<InputField>,
}

impl<P: ModelProvider, J: Journal> Agent<P, J> {
    pub fn clarification(&self) -> Option<&Clarification> {
        self.clarification.as_ref()
    }

    /// Continue the same task with a free-text user reply. The model interprets
    /// it; this is not confirmation of any generated argument or plan.
    pub fn reply(&mut self, id: &str, input: &str) -> Result<Step, AgentError> {
        self.check_cancelled()?;
        self.match_clarification(id)?;
        if input.trim().is_empty() || input.len() > self.config.max_input_bytes {
            return Err(AgentError::Input);
        }
        self.push_message(Message::new("user", input))?;
        self.clarification = None;
        self.record(EventKind::ClarificationAnswered, None)?;
        self.next_step()
    }

    /// Resolve structured fields using values supplied by the host/user. This
    /// does not call the model or replace unrelated draft arguments. All requested
    /// fields must be supplied, no others; policy and types are checked again.
    pub fn resolve(&mut self, id: &str, values: Value) -> Result<Step, AgentError> {
        self.check_cancelled()?;
        let question = self.match_clarification(id)?.clone();
        let tool = question.tool.ok_or(AgentError::InvalidAnswer)?;
        let values = values.as_object().ok_or(AgentError::InvalidAnswer)?;
        if question.fields.is_empty()
            || values.len() != question.fields.len()
            || serde_json::to_vec(values)
                .map_err(|_| AgentError::InvalidAnswer)?
                .len()
                > self.config.max_input_bytes
            || question.fields.iter().any(|field| {
                values.get(&field.name).is_none_or(|v| {
                    v.is_null() || (!field.choices.is_empty() && !field.choices.contains(v))
                })
            })
        {
            return Err(AgentError::InvalidAnswer);
        }
        let mut arguments = question.draft_arguments;
        let object = arguments.as_object_mut().ok_or(AgentError::InvalidAnswer)?;
        for (key, value) in values {
            object.insert(key.clone(), value.clone());
        }
        // Invalid typed answers do not consume the question or call the model.
        self.registry
            .tools
            .get(&tool)
            .ok_or(AgentError::Decision)?
            .validate(&arguments)?;
        self.push_message(Message::new(
            "user",
            json!({"clarification_answer":values}).to_string(),
        ))?;
        self.clarification = None;
        self.record(EventKind::ClarificationAnswered, None)?;
        self.propose(tool, arguments)
    }

    fn match_clarification(&self, id: &str) -> Result<&Clarification, AgentError> {
        self.clarification
            .as_ref()
            .filter(|q| q.clarification_id == id)
            .ok_or(AgentError::NoClarification)
    }

    pub(crate) fn ask(
        &mut self,
        message: String,
        tool: Option<String>,
        mut arguments: Value,
        fields: Vec<InputField>,
    ) -> Result<Step, AgentError> {
        if self.clarification_turns >= self.config.max_clarification_turns {
            return Err(AgentError::Budget);
        }
        let mut names = BTreeSet::new();
        if message.trim().is_empty()
            || message.len() > self.config.max_model_output_bytes
            || fields.len() > 32
            || fields.iter().any(|field| {
                field.name.is_empty()
                    || !names.insert(&field.name)
                    || field.prompt.trim().is_empty()
                    || field.prompt.len() > 1024
                    || field.choices.len() > 32
                    || tool.as_ref().is_none_or(|name| {
                        self.registry.definitions[name].parameters["properties"]
                            .get(&field.name)
                            .is_none()
                    })
            })
        {
            return Err(AgentError::Configuration);
        }
        for field in &fields {
            arguments
                .as_object_mut()
                .ok_or(AgentError::Configuration)?
                .remove(&field.name);
        }
        let question = Clarification {
            clarification_id: uuid::Uuid::new_v4().to_string(),
            message: message.clone(),
            tool,
            draft_arguments: arguments,
            fields,
        };
        // Record the question and retained draft in bounded conversation context.
        // A failed journal/context check never publishes a resumable question.
        let mut messages = self.messages.clone();
        messages.push(Message::new(
            "assistant",
            json!({"clarification":question}).to_string(),
        ));
        self.check_context(&messages)?;
        self.record(EventKind::Clarification, None)?;
        self.check_cancelled()?;
        self.messages = messages;
        self.clarification_turns += 1;
        self.can_continue = false;
        self.clarification = Some(question);
        Ok(Step::Clarification { message })
    }

    pub(crate) fn propose(&mut self, tool: String, arguments: Value) -> Result<Step, AgentError> {
        self.check_cancelled()?;
        if !arguments.is_object() || !self.active_tools.contains(&tool) {
            return Err(AgentError::Decision);
        }
        let result = self
            .registry
            .tools
            .get(&tool)
            .ok_or(AgentError::Decision)?
            .preflight(&arguments);
        match result {
            Ok(Preflight::Allow) => {}
            Ok(Preflight::Clarify { message, fields }) => {
                return self.ask(message, Some(tool), arguments, fields);
            }
            other => {
                self.record(EventKind::Rejected, None)?;
                return Err(AgentError::Tool(other.err().unwrap_or(ToolError::Denied)));
            }
        }
        self.check_cancelled()?;
        if self.tool_calls >= self.config.max_tool_calls {
            return Err(AgentError::Budget);
        }
        let call = match self.config.decision_protocol {
            DecisionProtocol::Tagged => json!({"kind":"call", "tool":tool, "arguments":arguments}),
            DecisionProtocol::Action => json!({"action":tool, "arguments":arguments}),
        };
        self.push_message(Message::new("assistant", call.to_string()))?;
        let preview = PlanPreview {
            plan_id: uuid::Uuid::new_v4().to_string(),
            tool,
            arguments,
        };
        self.record(EventKind::Proposed, Some(&preview))?;
        self.check_cancelled()?;
        self.pending = Some(Pending {
            preview: preview.clone(),
            created_at: Instant::now(),
        });
        Ok(Step::Proposal { plan: preview })
    }
}
