//! Schema generation module
//!
//! This module writes SQL schema output from TideORM model definitions.
//!
//! It is mainly for exporting or inspecting schema SQL, not for applying live
//! migrations. If generated SQL looks wrong, check the model metadata and index
//! declarations first.
//!
//! You can wire schema generation through `TideConfig::schema_file(...)` or use
//! `SchemaWriter::write_schema(...)` directly.

use parking_lot::RwLock;

mod generator;
mod types;
mod writer;

#[cfg(test)]
use crate::config::DatabaseType;
#[cfg(test)]
use crate::model::IndexDefinition;

pub use generator::SchemaGenerator;
pub use types::{
    ColumnSchema, TableSchema, TableSchemaBuilder, rust_type_to_column_type, rust_type_to_sql,
};
pub use writer::SchemaWriter;

pub(super) static SCHEMA_REGISTRY: RwLock<Vec<TableSchema>> = RwLock::new(Vec::new());

#[cfg(test)]
#[path = "../../tests/unit/schema_tests.rs"]
mod tests;
