use std::collections::VecDeque;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Proposed,
    Cancelled,
    Rejected,
    DispatchStarted,
    Completed,
    OutcomeUnknown,
    Final,
    Clarification,
}

/// Metadata only: arguments, raw model text, and tool output are deliberately
/// excluded. Applications can implement their own persistent, redacted sink.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub kind: EventKind,
    pub plan_id: Option<String>,
    pub tool: Option<String>,
}

#[derive(Debug, Error)]
#[error("journal write failed")]
pub struct JournalError;

pub trait Journal {
    fn record(&mut self, event: Event) -> Result<(), JournalError>;
}

pub struct MemoryJournal {
    capacity: usize,
    events: VecDeque<Event>,
}

impl Default for MemoryJournal {
    fn default() -> Self {
        Self::new(128)
    }
}

impl MemoryJournal {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            events: VecDeque::new(),
        }
    }
    pub fn events(&self) -> &VecDeque<Event> {
        &self.events
    }
}

impl Journal for MemoryJournal {
    fn record(&mut self, event: Event) -> Result<(), JournalError> {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
        Ok(())
    }
}
