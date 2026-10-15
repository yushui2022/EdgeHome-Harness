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

#[derive(Debug, Clone, Copy)]
pub struct ToolContext {
    pub timeout: Duration,
    pub max_output_bytes: usize,
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
    fn execute(&mut self, arguments: Value, context: ToolContext) -> Result<Value, ToolError>;
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
