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

/// Hidden TideORM-owned ORM facade for macro-generated code.
#[doc(hidden)]
pub mod orm;

// ============================================================================
// PUBLIC MODULES
// ============================================================================

/// Error types for TideORM
pub mod error;

/// Database connection and pool management
pub mod database;

/// Model trait and utilities
pub mod model;

/// Fluent query builder
pub mod query;

/// Attribute types and casting
pub mod types;

/// Soft delete support
pub mod soft_delete;

/// Callbacks and hooks for model lifecycle events
pub mod callbacks;

/// Database schema synchronization (DB_SYNC=true)
pub mod sync;

/// Schema generation (SQL file export)
pub mod schema;

/// Model relations (belongs_to, has_one, has_many)
pub mod relations;

/// Opt-in JPA-like entity manager and persistence-context support
#[cfg(feature = "entity-manager")]
pub mod entity_manager;

extern crate self as tideorm;

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../tests/support/postgres_test_config.rs"]
pub(crate) mod postgres_test_config;

/// File attachments system (attach, detach, sync)
#[cfg(feature = "attachments")]
pub mod attachments;

/// Translations system for multi-language support
#[cfg(feature = "translations")]
pub mod translations;

/// Global configuration
pub mod config;

/// Database migrations
pub mod migration;

/// Query logging and debugging
pub mod logging;

/// Performance profiling
pub mod profiling;

/// Query caching and prepared statement caching
pub mod cache;

/// Database seeding system
pub mod seeding;

/// Model validation system
pub mod validation;

/// Full-text search support
#[cfg(feature = "fulltext")]
pub mod fulltext;

/// Strongly-typed columns for compile-time type safety
pub mod columns;

/// Record tokenization for secure ID encoding
pub mod tokenization;

/// Re-exports for convenience
pub mod prelude;

// ============================================================================
// PUBLIC RE-EXPORTS
// ============================================================================
//
// Crate root vs. prelude: the prelude is glob-imported by every generated model,
// so it carries the names a user writes in ordinary code and nothing whose name
// is common enough to shadow a user's own type. The crate root carries those
// same names plus anything a user only ever spells out in full. A name exported
// from the root but missing from the prelude therefore needs a reason; where
// that reason exists it is written down next to the export.

pub use database::Database;
// Global database access functions
#[cfg(feature = "attachments")]
pub use attachments::{AttachmentError, FileAttachment, FilesData, HasAttachments};
pub use callbacks::{CallbackRunner, Callbacks};
pub use config::{Config, TideConfig};
pub use database::{db, has_global_db, require_db, try_db};
pub use error::{Error, Result};
// The bound parameter type of the raw-SQL `*_with_params` entry points. It is
// re-exported here and from the prelude so calling a documented API never
// requires naming the `#[doc(hidden)]` `internal` module.
pub use internal::DbValue;
pub use migration::{ColumnType, Migration, Migrator, Schema};
pub use model::{Model, ModelMeta};
pub use query::{AggregateFunction, JoinClause, JoinType, Order, QueryBuilder};
// Note: `relations::EagerLoadModel` is deliberately NOT re-exported here or in
// the prelude. It is `#[doc(hidden)]` machinery whose only method is
// `__eager_load`, and macro-generated code names it through the fully qualified
// `::tideorm::relations::EagerLoadModel` path.
pub use relations::{BelongsTo, EagerLoadExt, HasMany, HasOne, RelationExt, WithRelations};
pub use schema::SchemaWriter;
pub use soft_delete::SoftDelete;
#[cfg(feature = "translations")]
pub use translations::{
    ApplyTranslations, FieldTranslations, HasTranslations, TranslationError, TranslationInput,
    TranslationsData,
};

// Query logging and debugging
pub use logging::{
    LogLevel, QueryDebugInfo, QueryLogEntry, QueryLogger, QueryOperation, QueryStats, QueryTimer,
};

// Performance profiling
pub use profiling::{
    GlobalProfiler, GlobalStats, ProfileReport, ProfiledQuery, Profiler, QueryAnalyzer,
    QueryComplexity, QuerySuggestion, SuggestionLevel,
};

// Query and statement caching
pub use cache::{
    CacheConfig, CacheKeyBuilder, CacheOptions, CacheStats, CacheStrategy, CachedStatementInfo,
    PreparedStatementCache, PreparedStatementConfig, PreparedStatementStats, QueryCache,
};

// Validation
pub use validation::{
    ValidatableValue, Validate, ValidationBuilder, ValidationErrors, ValidationRule, Validator,
};

// Tokenization
pub use tokenization::{TokenConfig, TokenDecoder, TokenEncoder, Tokenizable};

// Re-export the derive macro
pub use tideorm_macros::Model;

// Re-export the attribute macro
pub use tideorm_macros::model;
pub use tideorm_macros::scopes;

// Re-export async_trait for macro use
pub use async_trait;

// Re-export inventory for macro use
#[doc(hidden)]
pub use inventory;

// Re-export chrono for macro use (timestamps)
pub use chrono;

// Re-export common types that users need
pub use chrono::{DateTime, NaiveDateTime, Utc};
pub use rust_decimal::Decimal;
pub use serde::{Deserialize, Serialize};
pub use uuid::Uuid;
