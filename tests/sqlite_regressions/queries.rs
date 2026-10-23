//! Query-builder reads.

#[path = "queries/aggregates.rs"]
mod aggregates;

#[path = "queries/chunk.rs"]
mod chunk;

#[path = "queries/distinct.rs"]
mod distinct;

#[path = "queries/filters.rs"]
mod filters;

#[path = "queries/projections.rs"]
mod projections;

#[path = "queries/schema_columns.rs"]
mod schema_columns;
