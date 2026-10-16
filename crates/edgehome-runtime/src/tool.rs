use std::{collections::BTreeMap, time::Duration};

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub cancellation: crate::CancellationToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ToolError {
    #[error("tool arguments are invalid")]
    Arguments,
    #[error("tool policy or current precondition rejected the call")]
    Denied,
    #[error("tool outcome cannot be established")]
    Unavailable,
}

/// Registered tools are trusted code, not sandboxed plugins. `validate` must be
/// side-effect-free and enforce semantic policy as well as argument types. It is
/// called at proposal and again before dispatch. Handlers own transport limits.
pub trait Tool {
    fn definition(&self) -> ToolDefinition;
    fn validate(&self, arguments: &Value) -> Result<(), ToolError>;
    /// Side-effect-free business check, repeated immediately before dispatch.
    /// Implementations must retain `validate`'s type and permission checks.
    fn preflight(&self, arguments: &Value) -> Result<Preflight, ToolError> {
        self.validate(arguments)?;
        Ok(Preflight::Allow)
    }
    fn execute(&mut self, arguments: Value, context: ToolContext) -> Result<Value, ToolError>;
}

#[derive(Debug, Clone, Serialize)]
pub struct InputField {
    /// A top-level argument name in this tool's schema.
    pub name: String,
    pub prompt: String,
    /// Empty means free input; otherwise resolution must select an exact value.
    pub choices: Vec<Value>,
}

#[derive(Debug, Clone)]
pub enum Preflight {
    Allow,
    Clarify {
        message: String,
        fields: Vec<InputField>,
    },
    Deny,
}

/// Add an application policy without replacing a typed tool's existing checks.
/// The callback is trusted, synchronous and must bound its own I/O.
pub struct GuardedTool<T, G> {
    inner: T,
    guard: G,
}

impl<T, G> GuardedTool<T, G> {
    pub fn new(inner: T, guard: G) -> Self {
        Self { inner, guard }
    }
}

impl<T: Tool, G: Fn(&Value) -> Result<Preflight, ToolError>> Tool for GuardedTool<T, G> {
    fn definition(&self) -> ToolDefinition {
        self.inner.definition()
    }
    fn validate(&self, arguments: &Value) -> Result<(), ToolError> {
        self.inner.validate(arguments)
    }
    fn preflight(&self, arguments: &Value) -> Result<Preflight, ToolError> {
        // Do not bypass an inner guard when wrappers are nested.
        match self.inner.preflight(arguments)? {
            Preflight::Allow => (self.guard)(arguments),
            other => Ok(other),
        }
    }
    fn execute(&mut self, arguments: Value, context: ToolContext) -> Result<Value, ToolError> {
        self.inner.execute(arguments, context)
    }
}

/// Generate the prompt schema and deserialize arguments from the same Rust type.
/// Use `#[serde(deny_unknown_fields)]` on argument structs; the validation closure
/// supplies application ranges, permissions, and current-state preconditions.
pub struct TypedTool<A, V, F> {
    definition: ToolDefinition,
    validate: V,
    execute: F,
    marker: std::marker::PhantomData<A>,
}

impl<A, V, F> TypedTool<A, V, F>
where
    A: DeserializeOwned + JsonSchema,
    V: Fn(&A) -> Result<(), ToolError>,
    F: FnMut(A, ToolContext) -> Result<Value, ToolError>,
{
    pub fn new(name: &str, description: &str, validate: V, execute: F) -> Self {
        Self {
            definition: ToolDefinition {
                name: name.into(),
                description: description.into(),
                parameters: serde_json::to_value(
                    schemars::r#gen::SchemaSettings::draft07()
                        .with(|settings| settings.inline_subschemas = true)
                        .into_generator()
                        .into_root_schema_for::<A>(),
                )
                .expect("serializable schema"),
            },
            validate,
            execute,
            marker: std::marker::PhantomData,
        }
    }
}

impl<A, V, F> Tool for TypedTool<A, V, F>
where
    A: DeserializeOwned + JsonSchema,
    V: Fn(&A) -> Result<(), ToolError>,
    F: FnMut(A, ToolContext) -> Result<Value, ToolError>,
{
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn validate(&self, arguments: &Value) -> Result<(), ToolError> {
        let args = serde_json::from_value(arguments.clone()).map_err(|_| ToolError::Arguments)?;
        (self.validate)(&args)
    }

    fn execute(&mut self, arguments: Value, context: ToolContext) -> Result<Value, ToolError> {
        let args = serde_json::from_value(arguments).map_err(|_| ToolError::Arguments)?;
        (self.execute)(args, context)
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    pub(crate) tools: BTreeMap<String, Box<dyn Tool>>,
    pub(crate) definitions: BTreeMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn definitions(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.definitions.values()
    }

    pub fn register(&mut self, tool: impl Tool + 'static) -> Result<(), ToolError> {
        let definition = tool.definition();
        let name = &definition.name;
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            || definition.description.len() > 1024
            || !definition.parameters.is_object()
            || serde_json::to_vec(&definition.parameters)
                .map_err(|_| ToolError::Arguments)?
                .len()
                > 8192
            || self.tools.contains_key(name)
        {
            return Err(ToolError::Arguments);
        }
        self.tools.insert(name.clone(), Box::new(tool));
        self.definitions.insert(definition.name.clone(), definition);
        Ok(())
    }
}
