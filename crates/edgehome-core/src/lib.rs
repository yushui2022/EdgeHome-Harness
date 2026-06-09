//! Core contracts for EdgeHome Harness.
//!
//! This crate must stay free of runtime adapters, SQLite implementations, and executor code.
//! It defines shared domain types used by parser, gate, memory, eval, and executor crates.

mod error;
mod schema;
mod types;

pub use error::HarnessError;
pub use schema::{
    model_candidate_schema_json, normalized_command_schema_json, schema_as_pretty_json,
};
pub use types::*;
