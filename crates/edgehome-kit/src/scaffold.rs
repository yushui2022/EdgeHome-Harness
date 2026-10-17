use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const REPOSITORY: &str = "https://github.com/yushui2022/EdgeHome-Harness.git";
// Release-controlled compatibility baseline, independent of build location and
// Cargo's artifact cache. Update only after publishing and testing that SDK.
pub const SDK_REV: &str = include_str!("../sdk-revision.txt");

pub fn create(
    destination: &Path,
    sdk: Option<&Path>,
    revision: Option<&str>,
    name: &str,
) -> Result<PathBuf> {
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
    if sdk.is_some() && revision.is_some() {
        bail!("--sdk-path and --sdk-rev are mutually exclusive");
    }
    let sdk = sdk
        .map(Path::canonicalize)
        .transpose()
        .context("SDK checkout unavailable; check --sdk-path")?;
    let revision = revision.unwrap_or(SDK_REV.trim());
    if let Some(sdk) = &sdk {
        for component in ["edgehome-kit", "edgehome-runtime", "edgehome-inference"] {
            if !sdk
                .join("crates")
                .join(component)
                .join("Cargo.toml")
                .is_file()
            {
                bail!("--sdk-path must name the EdgeHome repository root");
            }
        }
    } else if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!(
            "a full 40-character SDK commit is required; use --sdk-rev with a published commit, or --sdk-path for local development"
        );
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
        if let Some(sdk) = &sdk {
            let path = serde_json::to_string(
                &sdk.join("crates")
                    .join(component)
                    .to_string_lossy()
                    .trim_start_matches("\\\\?\\")
                    .replace('\\', "/"),
            )
            .expect("string serialization");
            format!("{{ path = {path} }}")
        } else {
            format!(
                "{{ git = {REPOSITORY:?}, rev = {:?} }}",
                revision.to_ascii_lowercase()
            )
        }
    };
    let manifest = format!(
        "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\nrust-version = \"1.95\"\nlicense = \"MIT\"\n\n[workspace]\n\n[dependencies]\nedgehome-kit = {}\nedgehome-runtime = {}\nanyhow = \"1\"\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\nschemars = {{ version = \"0.8\", features = [\"derive\"] }}\n",
        dependency("edgehome-kit"),
        dependency("edgehome-runtime")
    );
    fs::write(destination.join("Cargo.toml"), manifest)?;
    for (name, content) in [
        ("src/main.rs", include_str!("../templates/main.rs")),
        ("src/tools.rs", include_str!("../templates/tools.rs")),
        ("agent.yaml", include_str!("../templates/agent.yaml")),
        ("README.md", include_str!("../templates/README.md")),
        ("UPGRADING.md", include_str!("../templates/UPGRADING.md")),
        (
            "rust-toolchain.toml",
            include_str!("../../../rust-toolchain.toml"),
        ),
        // Seed with the SDK's reviewed versions. `update --workspace` reconciles
        // the new application without a blanket transitive dependency upgrade.
        ("Cargo.lock", include_str!("../../../Cargo.lock")),
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

pub fn lock_dependencies(project: &Path, offline: bool) -> Result<()> {
    let mut command = Command::new("cargo");
    command.current_dir(project).args(["update", "--workspace"]);
    if offline {
        command.arg("--offline");
    }
    let recovery = || {
        format!(
            "project kept at {}; dependency locking did not complete. Check Git/network access and the SDK revision, then run `cargo update --workspace` there before `cargo build --locked`",
            project.display()
        )
    };
    let status = command.status().with_context(recovery)?;
    if !status.success() {
        bail!("{}", recovery());
    }
    Ok(())
}
