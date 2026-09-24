//! Schema generation module
//!
//! This module writes `CREATE TABLE` / `CREATE INDEX` SQL for export:
//! [`SchemaWriter`] reads the connected database's
//! catalog, and [`SchemaGenerator`] renders
//! tables described with [`TableSchemaBuilder`].
//!
//! It is for exporting or inspecting schema SQL, not for applying live
//! migrations.
//!
//! You can wire schema generation through `TideConfig::schema_file(...)` or use
//! `SchemaWriter::write_schema(...)` directly.

mod generator;
mod types;
mod writer;

pub use generator::SchemaGenerator;
pub use types::{ColumnSchema, TableSchema, TableSchemaBuilder, rust_type_to_column_type};
pub use writer::SchemaWriter;

#[cfg(test)]
#[path = "../../tests/unit/schema_tests.rs"]
mod tests;
