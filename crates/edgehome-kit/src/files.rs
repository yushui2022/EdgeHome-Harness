//! Read-only reference tools. Explicit root, bounded traversal and UTF-8 reads.
//! This is not an OS sandbox against a hostile process racing path replacement.
use anyhow::Result;
use edgehome_runtime::{
    InputField, Preflight, Tool, ToolContext, ToolDefinition, ToolError, ToolRegistry, TypedTool,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 16384;
const MAX_DEPTH: usize = 4;

#[derive(Clone)]
pub struct FileWorkspace {
    root: PathBuf,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

fn link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn valid_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'));
            !part.is_empty() && !reserved && !part.starts_with('.') && !part.ends_with(['.', ' '])
        })
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

impl FileWorkspace {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        anyhow::ensure!(
            root.is_dir(),
            "file workspace must be an existing directory"
        );
        Ok(Self { root })
    }

    fn checked_path(&self, path: &str) -> Result<PathBuf, ToolError> {
        if !valid_relative(path) {
            return Err(ToolError::Denied);
        }
        let mut full = self.root.clone();
        for part in path.split('/') {
            full.push(part);
            match fs::symlink_metadata(&full) {
                Ok(meta) if link(&meta) => return Err(ToolError::Denied),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ToolError::Unavailable);
                }
                Err(_) => return Err(ToolError::Denied),
            }
        }
        let canonical = full.canonicalize().map_err(|_| ToolError::Denied)?;
        if !canonical.starts_with(&self.root) {
            return Err(ToolError::Denied);
        }
        Ok(canonical)
    }

    fn catalog(&self, started: Instant, timeout: Duration) -> Result<Vec<String>, ToolError> {
        if link(&fs::symlink_metadata(&self.root).map_err(|_| ToolError::Denied)?) {
            return Err(ToolError::Denied);
        }
        let mut pending = vec![(self.root.clone(), 0)];
        let mut files = Vec::new();
        let mut entries = 0;
        while let Some((dir, depth)) = pending.pop() {
            for entry in fs::read_dir(dir).map_err(|_| ToolError::Denied)? {
                entries += 1;
                if entries > MAX_ENTRIES || started.elapsed() >= timeout {
                    return Err(ToolError::Denied);
                }
                let entry = entry.map_err(|_| ToolError::Denied)?;
                let path = entry.path();
                let relative = path
                    .strip_prefix(&self.root)
                    .map_err(|_| ToolError::Denied)?;
                let Some(relative) = relative.to_str() else {
                    continue;
                };
                let relative = relative.replace('\\', "/");
                if !valid_relative(&relative) {
                    continue;
                }
                let meta = fs::symlink_metadata(&path).map_err(|_| ToolError::Denied)?;
                if link(&meta) {
                    continue;
                }
                if meta.is_dir() {
                    if depth >= MAX_DEPTH {
                        return Err(ToolError::Denied);
                    }
                    pending.push((path, depth + 1));
                } else if meta.is_file() {
                    files.push(relative);
                }
            }
        }
        files.sort();
        Ok(files)
    }

    fn text(&self, path: &str) -> Result<String, ToolError> {
        let path = self.checked_path(path)?;
        let meta = fs::symlink_metadata(&path).map_err(|_| ToolError::Denied)?;
        if !meta.is_file() || link(&meta) || meta.len() > MAX_BYTES as u64 {
            return Err(ToolError::Denied);
        }
        let file = fs::File::open(path).map_err(|_| ToolError::Unavailable)?;
        if !file
            .metadata()
            .map_err(|_| ToolError::Unavailable)?
            .is_file()
        {
            return Err(ToolError::Denied);
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ToolError::Unavailable)?;
        if bytes.len() > MAX_BYTES || bytes.contains(&0) {
            return Err(ToolError::Denied);
        }
        String::from_utf8(bytes).map_err(|_| ToolError::Denied)
    }

    fn check(&self, path: &str, guided: bool) -> Result<Preflight, ToolError> {
        match self.checked_path(path) {
            Ok(_) => {
                self.text(path)?;
                Ok(Preflight::Allow)
            }
            Err(ToolError::Unavailable) if guided => {
                let files = self.catalog(Instant::now(), Duration::from_secs(2))?;
                let matching: Vec<_> = files
                    .iter()
                    .filter(|name| name.rsplit('/').next() == Some(path))
                    .cloned()
                    .collect();
                // Candidates come from real state, never a benchmark answer key.
                let candidates = if matching.is_empty() { files } else { matching };
                let started = Instant::now();
                let mut choices = Vec::new();
                for candidate in candidates {
                    if started.elapsed() >= Duration::from_secs(2) {
                        return Err(ToolError::Denied);
                    }
                    // Existing does not imply readable by this tool.
                    if self.text(&candidate).is_ok() {
                        choices.push(Value::String(candidate));
                        if choices.len() == 32 {
                            break;
                        }
                    }
                }
                if choices.is_empty() {
                    return Ok(Preflight::Deny);
                }
                Ok(Preflight::Clarify {
                    message: "The requested path is unavailable or incomplete. Which existing relative file path should be used?".into(),
                    fields: vec![InputField { name: "path".into(), prompt: "Choose an existing relative file path.".into(), choices }],
                })
            }
            Err(_) => Ok(Preflight::Deny),
        }
    }

    /// `guided=false` is a measurement ablation: same tools, prompt and safety
    /// bounds, but application preflight cannot offer structured recovery.
    pub fn registry(&self, guided: bool) -> Result<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        let workspace = self.clone();
        registry.register(TypedTool::<EmptyArgs, _, _>::new(
            "file.list", "List existing relative file paths in the configured workspace. No arguments. Does not read content.",
            |_| Ok(()), move |_, ctx| {
                if ctx.cancellation.is_cancelled() { return Err(ToolError::Unavailable); }
                Ok(json!({"files":workspace.catalog(Instant::now(), ctx.timeout)?}))
            },
        ))?;
        for stats in [false, true] {
            registry.register(FileTool {
                workspace: self.clone(),
                guided,
                stats,
            })?;
        }
        Ok(registry)
    }
}

struct FileTool {
    workspace: FileWorkspace,
    guided: bool,
    stats: bool,
}
impl Tool for FileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: if self.stats { "file.stats" } else { "file.read" }.into(),
            description: if self.stats {
                "Count UTF-8 bytes, Unicode scalar characters and lines in one existing text file. Requires an explicit relative path; never guess missing or ambiguous paths."
            } else {
                "Read one existing UTF-8 text file. Requires an explicit relative path; never guess missing or ambiguous paths."
            }.into(),
            parameters: serde_json::to_value(schemars::schema_for!(PathArgs)).expect("schema"),
        }
    }
    fn validate(&self, arguments: &Value) -> Result<(), ToolError> {
        let args: PathArgs =
            serde_json::from_value(arguments.clone()).map_err(|_| ToolError::Arguments)?;
        if !valid_relative(&args.path) {
            return Err(ToolError::Denied);
        }
        Ok(())
    }
    fn preflight(&self, arguments: &Value) -> Result<Preflight, ToolError> {
        self.validate(arguments)?;
        self.workspace.check(
            arguments["path"].as_str().ok_or(ToolError::Arguments)?,
            self.guided,
        )
    }
    fn execute(&mut self, arguments: Value, context: ToolContext) -> Result<Value, ToolError> {
        let started = Instant::now();
        self.validate(&arguments)?;
        if context.cancellation.is_cancelled() {
            return Err(ToolError::Unavailable);
        }
        let path = arguments["path"].as_str().ok_or(ToolError::Arguments)?;
        let text = self.workspace.text(path)?;
        let value = if self.stats {
            json!({"path":path,"bytes":text.len(),"characters":text.chars().count(),"lines":text.lines().count()})
        } else {
            json!({"path":path,"text":text})
        };
        if started.elapsed() >= context.timeout
            || context.cancellation.is_cancelled()
            || serde_json::to_vec(&value)
                .map_err(|_| ToolError::Unavailable)?
                .len()
                > context.max_output_bytes
        {
            return Err(ToolError::Unavailable);
        }
        Ok(value)
    }
}
