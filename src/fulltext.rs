//! Full-Text Search Support for TideORM
//!
//! This module builds backend-specific full-text search SQL for PostgreSQL,
//! MySQL or MariaDB, and SQLite.
//!
//! The generated query shape depends on the active backend, so if search works
//! on one database and fails on another, start by checking backend selection,
//! index setup, and the exact search mode being requested.
//!
//! Use plain search first, then add ranking or highlighting only after the base
//! query shape and index coverage are behaving correctly on the active backend.

use std::fmt;
use std::marker::PhantomData;

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::sql_safety::{
    escape_fts5_query_literal_terms as escape_fts5_query, escape_sql_literal_for_db,
    format_identifier_reference, quote_ident,
    sanitize_postgres_proximity_tsquery_literals as sanitize_postgres_proximity_tsquery,
    sanitize_postgres_tsquery_literals as sanitize_postgres_tsquery,
};
use crate::internal::{ConnectionTrait, FromQueryResult, Value, build_statement_with_values};
use crate::model::Model;

// =============================================================================

mod core;
mod index_helpers;
mod search_builder;

pub use core::*;
pub use index_helpers::*;
pub use search_builder::*;
