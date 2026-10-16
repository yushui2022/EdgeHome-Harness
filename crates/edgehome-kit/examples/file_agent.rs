//! Set EDGEHOME_FILE_ROOT to an explicitly selected directory before starting.
use anyhow::{Context, Result};
use edgehome_kit::{files::FileWorkspace, run_cli};
use edgehome_runtime::{ModelError, ModelLimits, ModelProvider, ModelRequest};

struct NoMock;
impl ModelProvider for NoMock {
    fn complete(&mut self, _: &ModelRequest, _: ModelLimits) -> Result<String, ModelError> {
        Err(ModelError::Configuration)
    }
}

fn main() -> Result<()> {
    let root = std::env::var_os("EDGEHOME_FILE_ROOT")
        .context("set EDGEHOME_FILE_ROOT to an existing directory")?;
    let workspace = FileWorkspace::new(root)?;
    let guided = match std::env::var("EDGEHOME_FILE_POLICY").as_deref() {
        Ok("basic") => false,
        Ok("guided") | Err(_) => true,
        _ => anyhow::bail!("EDGEHOME_FILE_POLICY must be basic or guided"),
    };
    run_cli(move || workspace.registry(guided), || Box::new(NoMock))
}
