use anyhow::{Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn create(destination: &Path, sdk: Option<&Path>, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 64
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
    {
        bail!(
            "project name must start with a lowercase letter and contain only a-z, digits, hyphens or underscores"
        );
    }
    let default = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sdk = sdk
        .unwrap_or(&default)
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("SDK checkout unavailable; provide --sdk-path"))?;
    for component in ["edgehome-kit", "edgehome-runtime"] {
        if !sdk
            .join("crates")
            .join(component)
            .join("Cargo.toml")
            .is_file()
        {
            bail!("--sdk-path must name the EdgeHome repository root");
        }
    }
    if destination.exists() {
        bail!("destination already exists; init never overwrites an existing project");
    }
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(destination)?;
    fs::create_dir(destination.join("src"))?;
    let dependency = |component: &str| {
        serde_json::to_string(
            &sdk.join("crates")
                .join(component)
                .to_string_lossy()
                .trim_start_matches("\\\\?\\")
                .replace('\\', "/"),
        )
        .expect("string serialization")
    };
    let manifest = format!(
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.95\"\nlicense = \"MIT\"\n\n[workspace]\n\n[dependencies]\nedgehome-kit = {{ path = {} }}\nedgehome-runtime = {{ path = {} }}\nanyhow = \"1\"\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nschemars = {{ version = \"0.8\", features = [\"derive\"] }}\n",
        dependency("edgehome-kit"),
        dependency("edgehome-runtime")
    );
    fs::write(destination.join("Cargo.toml"), manifest)?;
    for (name, content) in [
        ("src/main.rs", include_str!("../templates/main.rs")),
        ("src/tools.rs", include_str!("../templates/tools.rs")),
        ("agent.yaml", include_str!("../templates/agent.yaml")),
        ("README.md", include_str!("../templates/README.md")),
        (
            "client.py",
            include_str!("../../../examples/python/agent_client.py"),
        ),
        (
            "cases.json",
            include_str!("../../../cases/sdk-starter.json"),
        ),
        ("LICENSE", include_str!("../../../LICENSE")),
        (".gitignore", "/target/\n*.jsonl\n.env\n.env.*\n"),
    ] {
        fs::write(destination.join(name), content)?;
    }
    Ok(destination.canonicalize()?)
}
