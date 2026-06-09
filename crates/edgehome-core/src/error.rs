use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HarnessError {
    #[error("input cannot be empty")]
    EmptyInput,

    #[error("model output is invalid: {0}")]
    InvalidModelOutput(String),

    #[error("schema validation failed: {0}")]
    SchemaValidation(String),

    #[error("normalization failed: {0}")]
    Normalization(String),

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("policy denied command: {0}")]
    PolicyDenied(String),
}
