//! Global TideORM configuration
//!
//! This module provides global configuration for TideORM, including
//! database connection, pool settings, translation settings, and other defaults.
//!
//! ## Example
//!
//! ```rust,no_run
//! # tideorm::__doctest_prelude!();
//! # async fn demo() -> tideorm::Result<()> {
//!
//! // Configure and connect in one unified call
//! TideConfig::init()
//!     .database_type(DatabaseType::Postgres)
//!     .database("postgres://localhost/mydb")
//!     .max_connections(20)
//!     .min_connections(5)
//!     .languages(&["en", "fr", "ar", "es"])
//!     .fallback_language("en")
//!     .connect()
//!     .await?;
//!
//! // Now use models - database is automatically available
//! let users = User::all().await?;
//! # let _ = users;
//! # Ok::<(), tideorm::Error>(())
//! # }
//! ```

#[allow(missing_docs)]
mod builder;
#[allow(missing_docs)]
mod database;
#[allow(missing_docs)]
mod registration;
#[allow(missing_docs)]
mod settings;
mod state;

pub use builder::TideConfig;
pub use database::DatabaseType;
pub use registration::{RegisterMigrations, RegisterSeeds};
pub use settings::{Config, PoolConfig};

#[cfg(feature = "attachments")]
pub use settings::FileUrlGenerator;

#[cfg(test)]
pub(crate) use database::rewrite_driver_url;
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
#[path = "../testing/config_tests.rs"]
mod tests;
