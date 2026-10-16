use anyhow::Result;
use edgehome_runtime::{Event, Journal, JournalError, MemoryJournal};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

/// Optional metadata-only session log. This is not a restorable transcript.
/// A fresh path is required; log size is capped and write failures fail closed.
pub struct SessionJournal {
    memory: MemoryJournal,
    file: Option<File>,
    bytes: usize,
    max_bytes: usize,
    events: bool,
}

impl SessionJournal {
    pub fn new(path: Option<&Path>, events: bool) -> Result<Self> {
        let file = path
            .map(|p| OpenOptions::new().write(true).create_new(true).open(p))
            .transpose()
            .map_err(|_| {
                anyhow::anyhow!(
                    "cannot create session journal; choose a new file in an existing directory"
                )
            })?;
        Ok(Self {
            memory: MemoryJournal::default(),
            file,
            bytes: 0,
            max_bytes: 512 * 1024,
            events,
        })
    }
}

impl Journal for SessionJournal {
    fn record(&mut self, event: Event) -> Result<(), JournalError> {
        let mut line =
            serde_json::to_vec(&json!({"type":"event","event":event})).map_err(|_| JournalError)?;
        line.push(b'\n');
        if let Some(file) = &mut self.file {
            if self.bytes.saturating_add(line.len()) > self.max_bytes {
                return Err(JournalError);
            }
            file.write_all(&line).map_err(|_| JournalError)?;
            file.sync_data().map_err(|_| JournalError)?;
            self.bytes += line.len();
        }
        self.memory.record(event)?;
        if self.events {
            std::io::stderr()
                .write_all(&line)
                .map_err(|_| JournalError)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_budget_failure_happens_before_more_file_data_is_written() {
        let temp = crate::tests::Temp::new();
        let path = temp.path().join("bounded.jsonl");
        let mut journal = SessionJournal::new(Some(&path), false).unwrap();
        journal.max_bytes = 1;
        assert!(
            journal
                .record(Event {
                    kind: edgehome_runtime::EventKind::DispatchStarted,
                    plan_id: Some("plan".into()),
                    tool: Some("tool".into())
                })
                .is_err()
        );
        drop(journal);
        assert_eq!(std::fs::metadata(path).unwrap().len(), 0);
    }
}
