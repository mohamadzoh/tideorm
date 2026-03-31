//! Database connection and pool management
//!
//! This module exposes the `Database` handle used by query builders, model
//! helpers, and transactions.
//!
//! If database calls fail early, the most common causes are:
//! - the global database was never initialized with `Database::init()` or
//!   `Database::set_global()`
//! - the connection URL is wrong for the selected backend
//! - the current code is running outside the transaction or database scope you
//!   expected
//!
//! Use `Database::connect()` for straightforward setup. Use
//! `Database::builder()` when you need to adjust pool limits or timeouts.
//!
//! Practical split:
//! - use `Database::connect()` when you already know the URL and defaults are fine
//! - use `Database::builder()` when pool sizing, idle counts, or timeouts need tuning
//! - use the global handle only after startup initialization is finished, otherwise model helpers fail before they ever reach the backend
//!
//! If a query unexpectedly uses the wrong connection, check whether code is
//! still inside the intended transaction scope or whether it fell back to the
//! global handle.

mod builder;
mod core;
mod raw;
mod state;
mod transaction;

use std::future::Future;

pub use builder::DatabaseBuilder;
pub use core::Database;
pub use state::{
    __current_backend, __current_connection, __current_db, db, has_global_db, require_db, try_db,
};
pub use transaction::{Connection, Transaction};

#[doc(hidden)]
pub use transaction::ConnectionRef;

pub(crate) use state::DatabaseHandle;

pub(crate) async fn __in_db_scope<F, T>(db: &Database, future: F) -> crate::error::Result<T>
where
    F: Future<Output = crate::error::Result<T>>,
{
    let handle = db.current_handle()?;
    state::with_connection_override(handle, future).await
}

#[cfg(test)]
#[path = "../../tests/unit/database_tests.rs"]
mod tests;
