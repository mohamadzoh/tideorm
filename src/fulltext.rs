//! Full-Text Search Support for TideORM
//!
//! This module builds backend-specific full-text search SQL for PostgreSQL,
//! MySQL or MariaDB, and SQLite.
//!
//! The generated query shape depends on the active backend, so if search works
//! on one database and fails on another, start by checking backend selection,
//! index setup, and the exact search mode being requested.
//!
//! Use plain search first, then add ranking only after the base query shape and
//! index coverage are behaving correctly on the active backend.

use std::fmt;
use std::marker::PhantomData;

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::sql_builder::SqlBuilder;
use crate::internal::sql_safety::{
    escape_fts5_query_literal_terms as escape_fts5_query, escape_sql_literal_for_db,
    format_identifier_reference, fts5_boolean_query, fts5_near_query, fts5_phrase_query,
    fts5_prefix_query, is_safe_identifier_segment, quote_ident, sanitize_mysql_fulltext_query,
    sanitize_postgres_boolean_tsquery,
    sanitize_postgres_proximity_tsquery_literals as sanitize_postgres_proximity_tsquery,
    sanitize_postgres_tsquery_literals as sanitize_postgres_tsquery,
};
use crate::internal::{ConnectionTrait, Value, build_statement_with_values, push_param};
use crate::model::Model;

mod core;
mod index_helpers;
mod search_builder;

pub use core::*;
pub use index_helpers::*;
pub use search_builder::*;

/// Render `columns` as a quoted, comma-separated list, each name preceded by
/// `prefix` (`new.` and `old.` inside SQLite triggers).
fn column_list(db_type: DatabaseType, columns: &[String], prefix: &str) -> String {
    let mut params = Vec::new();
    let mut builder = SqlBuilder::new(db_type, &mut params);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            builder = builder.raw(", ");
        }
        builder = builder.raw(prefix).ident(column);
    }
    builder.into_sql()
}

/// Render the PostgreSQL text a `tsvector` is built from: every column, with
/// `NULL` read as empty, joined by spaces.
fn pg_search_document(columns: &[String]) -> String {
    let mut params = Vec::new();
    let mut builder = SqlBuilder::new(DatabaseType::Postgres, &mut params);
    for (index, column) in columns.iter().enumerate() {
        if index > 0 {
            builder = builder.raw(" || ' ' || ");
        }
        builder = builder.raw("COALESCE(").ident(column).raw(", '')");
    }
    builder.into_sql()
}

#[cfg(test)]
#[path = "../tests/unit/fulltext_tests.rs"]
mod tests;
