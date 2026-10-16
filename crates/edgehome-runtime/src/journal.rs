use std::collections::VecDeque;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    TaskStarted,
    ModelStarted,
    ModelCompleted,
    ValidationRepair,
    Proposed,
    Cancelled,
    Rejected,
    DispatchStarted,
    Completed,
    OutcomeUnknown,
    Final,
    Clarification,
    ClarificationAnswered,
}

/// Observe lifecycle metadata after the underlying journal accepts it. The
/// listener runs synchronously and must remain fast; it is trusted host code.
pub struct ObservedJournal<J, F> {
    inner: J,
    listener: F,
}

impl<J, F> ObservedJournal<J, F> {
    pub fn new(inner: J, listener: F) -> Self {
        Self { inner, listener }
    }
    pub fn inner(&self) -> &J {
        &self.inner
    }
}

impl<J: Journal, F: FnMut(&Event)> Journal for ObservedJournal<J, F> {
    fn record(&mut self, event: Event) -> Result<(), JournalError> {
        self.inner.record(event.clone())?;
        (self.listener)(&event);
        Ok(())
    }
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
