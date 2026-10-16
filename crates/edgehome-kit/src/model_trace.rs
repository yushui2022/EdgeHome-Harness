use edgehome_runtime::{CancellationToken, ModelError, ModelLimits, ModelProvider, ModelRequest};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    time::Instant,
};

/// Explicit content capture for reviewable experiments; never enabled by default.
/// Refuses overwrite, bounds total bytes, and fails closed if evidence is lost.
pub(crate) struct ModelTrace {
    inner: Box<dyn ModelProvider>,
    file: File,
    bytes: usize,
}
impl ModelTrace {
    pub fn wrap(
        inner: Box<dyn ModelProvider>,
        path: Option<&Path>,
    ) -> anyhow::Result<Box<dyn ModelProvider>> {
        match path {
            None => Ok(inner),
            Some(path) => Ok(Box::new(Self {
                inner,
                file: OpenOptions::new().create_new(true).write(true).open(path)?,
                bytes: 0,
            })),
        }
    }
}
impl ModelProvider for ModelTrace {
    fn complete(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
    ) -> Result<String, ModelError> {
        self.complete_cancellable(request, limits, &CancellationToken::default())
    }
    fn complete_cancellable(
        &mut self,
        request: &ModelRequest,
        limits: ModelLimits,
        cancel: &CancellationToken,
    ) -> Result<String, ModelError> {
        let started = Instant::now();
        let result = self.inner.complete_cancellable(request, limits, cancel);
        let response = result
            .as_ref()
            .ok()
            .map(|s| s.chars().take(limits.max_output_bytes).collect::<String>());
        let mut bytes=serde_json::to_vec(&json!({"request":request,"response":response,"error":result.as_ref().err(),"elapsed_ms":started.elapsed().as_millis()})).map_err(|_|ModelError::Unavailable)?;
        bytes.push(b'\n');
        if self.bytes.saturating_add(bytes.len()) > 2 * 1024 * 1024 {
            return Err(ModelError::ResponseTooLarge);
        }
        self.file
            .write_all(&bytes)
            .and_then(|_| self.file.sync_data())
            .map_err(|_| ModelError::Unavailable)?;
        self.bytes += bytes.len();
        result
    }
}
