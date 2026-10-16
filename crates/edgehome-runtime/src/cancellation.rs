use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A one-way, cooperative stop signal. Create a new agent after cancellation.
/// Signalling cannot forcibly interrupt blocking code or undo a dispatched tool.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
