//! Prelude module for TideORM
//!
//! This module re-exports the types most applications use frequently.
//! Import this when you want the common model, query, migration, and relation
//! types in scope without pulling each module in separately.

pub use crate::callbacks::Callbacks;
#[cfg(feature = "attachments")]
pub use crate::config::FileUrlGenerator;
pub use crate::config::{Config, DatabaseType, PoolConfig, TideConfig};
pub use crate::database::{AccessMode, IsolationLevel, SqliteTransactionMode, TransactionOptions};
pub use crate::database::{Database, DatabaseBuilder, Transaction};
pub use crate::database::{db, has_global_db, require_db};
// The drivers' connection options, for `DatabaseBuilder` settings a URL does not name.
#[cfg(feature = "mysql")]
pub use crate::internal::driver::mysql::{MySqlConnectOptions, MySqlSslMode};
#[cfg(feature = "postgres")]
pub use crate::internal::driver::postgres::{PgConnectOptions, PgSslMode};
#[cfg(feature = "sqlite")]
pub use crate::internal::driver::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqliteLockingMode, SqliteSynchronous,
};
// Bound parameter values for `Database::raw_with_params` and friends. Exported
// here so raw SQL never sends callers into the hidden `internal` module.
pub use crate::internal::DbValue;
pub use crate::model::{
    BatchUpdateBuilder, IndexDefinition, Model, ModelMeta, NestedSave, NestedSaveBuilder,
    OnConflictBuilder, UpdateValue,
};
pub use crate::query::{
    Aggregate, AggregateCondition, CTE, Chunks, CursorPage, FrameBound, FrameType, HavingCondition,
    JoinResultConsolidator, JoinType, LogicalOp, OrBranchBuilder, OrGroup, Order, Paginated,
    QueryBuilder, QueryFragment, SortOrder, WindowFunction, WindowFunctionType,
};
pub use crate::schema::{
    ColumnSchema, SchemaGenerator, SchemaWriter, TableSchema, TableSchemaBuilder,
};
pub use crate::soft_delete::SoftDelete;
pub use crate::sync::{RegisterModels, SyncMode, SyncModel};

// `Result` is deliberately left out: it would shadow `std::result::Result` in
// every module that glob-imports the prelude. Spell it `tideorm::Result`.
pub use crate::error::{DbFailureKind, Error};

// Migrations
pub use crate::migration::{
    AppliedMigration, BatchFailure, ColumnType, DefaultValue, ForeignKeyAction, Migration,
    MigrationFailure, MigrationInfo, MigrationResult, MigrationStatus, Migrator, Schema, StringLen,
    async_trait,
};

// Relations. `relations::EagerLoadModel` is left out on purpose: it is
// `#[doc(hidden)]` machinery that generated code names by its full path.
pub use crate::relations::{
    BelongsTo, EagerQueryBuilder, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne, MorphTo,
    RelationPath, RelationTree, SelfRef, SelfRefMany,
};

// File Attachments
#[cfg(feature = "attachments")]
pub use crate::attachments::{AttachmentError, FileAttachment, FilesData, HasAttachments};

// Translations
#[cfg(feature = "translations")]
pub use crate::translations::{
    ApplyTranslations, FieldTranslations, HasTranslations, TranslationError, TranslationInput,
    TranslationsData,
};

// Query logging and debugging
pub use crate::logging::{
    LogLevel, QueryDebugInfo, QueryLogEntry, QueryLogger, QueryOperation, QueryStats, QueryTimer,
};

// Performance profiling
pub use crate::profiling::{
    GlobalProfiler, GlobalStats, ProfileReport, ProfiledQuery, Profiler, QueryAnalyzer,
    QueryComplexity, QuerySuggestion, SuggestionLevel,
};

// Query caching
pub use crate::cache::{
    CacheConfig, CacheInvalidation, CacheKeyBuilder, CacheOptions, CacheStats, CacheStrategy,
    QueryCache,
};

// Database seeding
pub use crate::seeding::{Seed, SeedFailure, SeedInfo, SeedResult, SeedStatus, Seeder};

// Validation
pub use crate::validation::{
    ValidatableValue, Validate, ValidationBuilder, ValidationErrors, ValidationRule, Validator,
};

// Tokenization
pub use crate::tokenization::{TokenConfig, TokenDecoder, TokenEncoder, Tokenizable};

// Full-text search
#[cfg(feature = "fulltext")]
pub use crate::fulltext::{
    FullTextConfig, FullTextIndex, FullTextIndexConfig, FullTextSearch, FullTextSearchBuilder,
    HighlightConfig, HighlightedField, PgFullTextIndexType, SearchMode, SearchResult,
    SearchWeights, generate_snippet, highlight_text, pg_headline_sql,
};

// Strongly-typed columns
pub use crate::columns::{
    Column, ColumnCondition, ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOrd, ColumnRead,
    ColumnValue, IntoColumnName, IntoColumnNames,
};

// JPA-like entity manager / persistence context
#[cfg(feature = "entity-manager")]
pub use crate::entity_manager::{
    EntityManager, EntityManagerLoad, EntityState, Managed, ManagedModel,
};

// Derive macros
pub use tideorm_macros::{FromQueryResult, Model};

// Attribute macro
pub use tideorm_macros::model;
pub use tideorm_macros::scopes;

// Common external types users will need
pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value as JsonValue, json};

// Date/time types
pub use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

// Other common types
pub use rust_decimal::Decimal;
pub use uuid::Uuid;

// Type aliases and field value types
pub use crate::types::{
    BigIntArray, BoolArray, FloatArray, Hashed, IntArray, Json, JsonArray, Jsonb, Text, TextArray,
    UnixTimestamp, UnixTimestampMillis,
};
