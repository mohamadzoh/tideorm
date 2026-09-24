//! Global TideORM configuration
//!
//! This module defines the global TideORM configuration surface, including
//! database connection, pool settings, translation settings, and other defaults.
//!
//! Use this as the startup entry point when you need one place to initialize
//! the global database handle, pool limits, languages, file URL generation, and
//! related defaults before models start issuing queries.

mod builder;
mod database;
mod settings;
mod state;

pub use builder::TideConfig;
pub use database::DatabaseType;
pub use settings::{Config, PoolConfig};

#[cfg(feature = "attachments")]
pub use settings::FileUrlGenerator;

#[cfg(test)]
#[path = "../../tests/unit/config_tests.rs"]
mod tests;
