//! Schema generation module
//!
//! [`Catalog`] (`Database::catalog`) reads the connected database's tables,
//! columns, indexes and foreign keys.
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

pub(crate) mod catalog;
mod generator;
mod types;
mod writer;

pub use catalog::{Catalog, ColumnInfo, ForeignKeyInfo, IndexInfo};
pub use generator::SchemaGenerator;
pub(crate) use types::column_declaration_methods;
pub use types::{ColumnSchema, TableSchema, TableSchemaBuilder};
pub use writer::SchemaWriter;

#[cfg(test)]
#[path = "../../tests/unit/schema_tests.rs"]
mod tests;
