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
use crate::internal::Value;
use crate::internal::sql_safety::{
    column_list, double_quoted, escape_fts5_query_literal_terms as escape_fts5_query,
    fts5_boolean_query, fts5_near_query, fts5_phrase_query, fts5_prefix_query, has_searchable_char,
    is_safe_identifier_segment, quote_ident, quote_qualified, sanitize_mysql_fulltext_query,
    sanitize_postgres_boolean_tsquery,
    sanitize_postgres_proximity_tsquery_literals as sanitize_postgres_proximity_tsquery,
    sanitize_postgres_tsquery_literals as sanitize_postgres_tsquery, search_words,
};
use crate::model::Model;
use crate::orm::sql_builder::BoundSql;

mod core;
mod index_helpers;
mod search_builder;

pub use core::*;
pub use index_helpers::*;
pub use search_builder::*;

/// A PostgreSQL text search configuration, written into the SQL as an escaped
/// constant.
///
/// An index [`FullTextIndex`] builds names its configuration as a constant,
/// and only an expression naming the same constant can use it: a bound
/// configuration is cast when the statement runs, which the planner cannot
/// match to the index, so every search read the whole table.
fn pg_language(language: Option<&str>) -> String {
    DatabaseType::Postgres
        .dialect()
        .string_literal(language.unwrap_or("english"))
}

/// `to_tsvector(<language>, <columns>)` as the index [`FullTextIndex`] builds
/// and every search reads it: a search uses the index only when it spells the
/// same expression.
fn pg_tsvector(language: Option<&str>, columns: &[String]) -> String {
    format!(
        "to_tsvector({}, {})",
        pg_language(language),
        pg_search_document(columns)
    )
}

/// Render the PostgreSQL text a `tsvector` is built from: every column, with
/// `NULL` read as empty, joined by spaces.
fn pg_search_document(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| {
            format!(
                "COALESCE({}, '')",
                quote_ident(DatabaseType::Postgres, column)
            )
        })
        .collect::<Vec<_>>()
        .join(" || ' ' || ")
}

#[cfg(test)]
#[path = "../tests/unit/fulltext_tests.rs"]
mod tests;
