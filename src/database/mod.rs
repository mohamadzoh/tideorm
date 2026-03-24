//! Database connection and pool management
//!
//! This module provides the main `Database` struct for connecting to and
//! interacting with databases. It completely hides the underlying connection
//! pool and ORM implementation.
//!
//! ## Example
//!
//! ```rust,no_run
//! # tideorm::__doctest_prelude!();
//! # async fn demo() -> tideorm::Result<()> {
//!
//! // Simple connection
//! let db = Database::connect("postgres://localhost/myapp").await?;
//!
//! // With options
//! let db = Database::builder()
//!     .url("postgres://localhost/myapp")
//!     .max_connections(10)
//!     .min_connections(2)
//!     .connect_timeout(Duration::from_secs(5))
//!     .build()
//!     .await?;
//!
//! // Transactions
//! db.transaction(|tx| Box::pin(async move {
//!     // tx.connection() gives you the transaction connection
//!     Ok(())
//! })).await?;
//! # let _ = db;
//! # Ok::<(), tideorm::Error>(())
//! # }
//! ```
//!
//! ## Global Database Connection
//!
//! TideORM supports a global database connection, allowing models to access
//! the database without explicitly passing a connection reference:
//!
//! ```rust,no_run
//! # tideorm::__doctest_prelude!();
//! # async fn demo() -> tideorm::Result<()> {
//! // Initialize global connection (call once at startup)
//! Database::init("postgres://localhost/myapp").await?;
//!
//! // Now models can use the global connection automatically
//! let user = User {
//!     id: 0,
//!     email: "john@example.com".to_string(),
//!     name: "John".to_string(),
//!     ..Default::default()
//! };
//!
//! // No need to pass &db - uses global connection automatically
//! let user = user.save().await?;
//! # let _ = user;
//! # Ok::<(), tideorm::Error>(())
//! # }
//! ```

mod builder;
mod core;
mod raw;
mod state;
mod transaction;

pub use builder::DatabaseBuilder;
pub use core::Database;
pub use state::{
    __current_backend, __current_connection, __current_db, db, has_global_db, require_db, try_db,
};
pub use transaction::{Connection, Transaction};

#[doc(hidden)]
pub use transaction::ConnectionRef;

pub(crate) use state::{DatabaseHandle, poll_with_thread_override};

#[cfg(test)]
#[path = "../testing/database_tests.rs"]
mod tests;
