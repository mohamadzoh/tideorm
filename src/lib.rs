//! TideORM is a Rust ORM with macro-generated models, a query builder, and
//! field-declared relation helpers.
//!
//! When you are trying to debug behavior, start with:
//! - `query::QueryBuilder::debug()` to inspect generated SQL before execution
//! - `logging::QueryLogger` to see executed queries and slow-query timing
//! - `database::Database::init()` or `Database::set_global()` errors when models
//!   report that no global database has been configured
//!
//! The module docs below cover the public surface area in more detail.

#![recursion_limit = "256"]
// CI runs `clippy -- -D warnings`, so these `warn` levels are effectively deny
// there. `missing_docs` is still opted out of by a number of `allow` sites in
// individual modules; those are the gaps, not this attribute.
#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)]

#[doc(hidden)]
pub mod internal;

#[doc(hidden)]
pub mod orm;

pub mod cache;
pub mod callbacks;
pub mod columns;
pub mod config;
pub mod database;
pub mod error;
pub mod logging;
pub mod migration;
pub mod model;
pub mod prelude;
pub mod profiling;
pub mod query;
pub mod relations;
pub mod schema;
pub mod seeding;
pub mod soft_delete;
pub mod sync;
pub mod tokenization;
pub mod types;
pub mod validation;

#[cfg(feature = "attachments")]
pub mod attachments;
#[cfg(feature = "entity-manager")]
pub mod entity_manager;

/// Emit the given items only when TideORM itself is built with `entity-manager`.
///
/// Generated models wrap their entity-manager items in this rather than in
/// `#[cfg(feature = "entity-manager")]`, which rustc evaluates against the
/// crate the model is defined in: every crate without a feature of that name
/// warned `unexpected cfg condition value`, and one that enabled
/// `tideorm/entity-manager` without declaring it silently lost the items.
#[cfg(feature = "entity-manager")]
#[doc(hidden)]
#[macro_export]
macro_rules! __if_entity_manager {
    ($($item:tt)*) => { $($item)* };
}

/// See the `entity-manager` build of this macro.
#[cfg(not(feature = "entity-manager"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __if_entity_manager {
    ($($item:tt)*) => {};
}
#[cfg(feature = "fulltext")]
pub mod fulltext;
#[cfg(feature = "translations")]
pub mod translations;

extern crate self as tideorm;

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../tests/support/postgres_test_config.rs"]
pub(crate) mod postgres_test_config;

// The crate root re-exports the whole prelude, so `tideorm::X` and
// `tideorm::prelude::X` never drift apart. Only the names the prelude leaves out
// on purpose are added here: `Result`, which would shadow `std::result::Result`
// in every glob-importing module, and the crates generated code reaches through
// `::tideorm::`.
pub use prelude::*;

pub use error::Result;

pub use async_trait;
pub use chrono;
#[doc(hidden)]
pub use inventory;
// A model's serde impls and JSON helpers name these, so the crate defining the
// model needs neither as a dependency of its own.
#[doc(hidden)]
pub use serde;
#[doc(hidden)]
pub use serde_json;

#[cfg(test)]
#[path = "../tests/unit/support.rs"]
mod test_support;
