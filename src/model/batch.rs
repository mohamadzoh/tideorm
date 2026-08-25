//! Bulk `UPDATE` in a single statement.
//!
//! [`BatchUpdateBuilder`] writes columns, not models: no rows are loaded and no
//! callbacks or validations run. Use it for maintenance writes that would be
//! wasteful as a load-modify-save loop; use `Model::save()` when the model's own
//! lifecycle matters.

use crate::columns::IntoColumnName;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident;
use crate::query::{LogicalOp, OrGroup, QueryBuilder};

use super::Model;

mod builder_setup_filters;
mod sql_execution;
mod validation_helpers;

/// Builder for batch update operations.
///
/// # Soft-delete scope
///
/// Unlike [`crate::query::QueryBuilder`], a batch update **includes
/// soft-deleted rows by default** — it behaves as if `with_trashed()` had been
/// called. That default exists so bulk maintenance writes (restoring a batch of
/// trashed rows, backfilling a column, scrubbing personal data) still reach the
/// rows they are aimed at. Call
/// [`BatchUpdateBuilder::without_trashed`] to restrict the update to rows that
/// are not soft-deleted.
pub struct BatchUpdateBuilder<M: Model> {
    _marker: std::marker::PhantomData<M>,
    updates: std::collections::HashMap<String, UpdateValue>,
    conditions: Vec<crate::query::WhereCondition>,
    returning: bool,
    limit_value: Option<u64>,
    /// Whether soft-deleted rows are in scope. Defaults to `true`.
    include_trashed: bool,
}

/// What a batch update writes into one column.
///
/// Most variants describe an assignment computed *by the database* from the
/// column's current value, so the update stays a single statement and does not
/// have to read rows first. That also means these never run model callbacks or
/// validations — a batch update writes columns, not models.
///
/// Build these through the [`BatchUpdateBuilder`] setters
/// ([`set`](BatchUpdateBuilder::set), [`increment`](BatchUpdateBuilder::increment),
/// and friends) rather than by hand.
#[derive(Debug, Clone)]
pub enum UpdateValue {
    /// Assign a literal value, bound as a parameter.
    Value(serde_json::Value),
    /// Assign a raw SQL expression, spliced into the statement verbatim.
    ///
    /// Nothing escapes or validates the string, so it must never contain
    /// user-controlled input. Written by
    /// [`set_trusted_raw`](BatchUpdateBuilder::set_trusted_raw).
    UnsafeRaw(String),
    /// Add to the column's current value (`col = col + n`).
    Increment(i64),
    /// Subtract from the column's current value (`col = col - n`).
    Decrement(i64),
    /// Multiply the column's current value (`col = col * n`).
    Multiply(f64),
    /// Divide the column's current value (`col = col / n`).
    ///
    /// A divisor of zero is left to the backend, which typically errors.
    Divide(f64),
    /// Append a value to an array or JSON array column.
    ///
    /// Rendered with the backend's own function: `array_append` on PostgreSQL,
    /// `JSON_ARRAY_APPEND` on MySQL/MariaDB, `json_insert` on SQLite.
    ArrayAppend(serde_json::Value),
    /// Remove matching entries from an array or JSON array column.
    ///
    /// PostgreSQL removes every match; the JSON-based backends remove one.
    ArrayRemove(serde_json::Value),
    /// Set one path inside a JSON column, leaving the rest of the document alone.
    ///
    /// The path is restricted to `$.field` / `$.field.subfield` form with plain
    /// identifier segments; anything else is rejected when the SQL is built.
    JsonSet(String, serde_json::Value),
    /// Replace the column only where it is currently `NULL` (`col = COALESCE(col, default)`).
    ///
    /// This is the backfill variant: rows that already hold a value keep it.
    Coalesce(serde_json::Value),
}

impl<M: Model> Default for BatchUpdateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/model_batch_tests.rs"]
mod tests;
