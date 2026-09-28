//! Prelude module for TideORM
//!
//! This module re-exports the types most applications use frequently.
//! Import this when you want the common model, query, migration, and relation
//! types in scope without pulling each module in separately.

pub use crate::callbacks::Callbacks;
#[cfg(feature = "attachments")]
pub use crate::config::FileUrlGenerator;
pub use crate::config::{Config, DatabaseType, PoolConfig, TideConfig};
pub use crate::database::{Database, DatabaseBuilder, Transaction};
pub use crate::database::{db, has_global_db, require_db};
// Bound parameter values for `Database::raw_with_params` and friends. Exported
// here so raw SQL never sends callers into the hidden `internal` module.
pub use crate::internal::DbValue;
pub use crate::model::{
    BatchUpdateBuilder, IndexDefinition, Model, ModelMeta, NestedSave, NestedSaveBuilder,
    OnConflictBuilder, UpdateValue,
};
pub use crate::query::{
    Aggregate, AggregateCondition, CTE, FrameBound, FrameType, HavingCondition, JoinClause,
    JoinResultConsolidator, JoinType, LogicalOp, OrBranchBuilder, OrGroup, Order, Paginated,
    QueryBuilder, QueryFragment, SortOrder, UnionClause, UnionType, WindowFunction,
    WindowFunctionType,
};
pub use crate::schema::{
    ColumnSchema, SchemaGenerator, SchemaWriter, TableSchema, TableSchemaBuilder,
};
pub use crate::soft_delete::SoftDelete;
pub use crate::sync::{RegisterModels, SyncModel};

// `Result` is deliberately left out: it would shadow `std::result::Result` in
// every module that glob-imports the prelude. Spell it `tideorm::Result`.
pub use crate::error::Error;

// Migrations
pub use crate::migration::{
    ColumnType, DefaultValue, Migration, MigrationInfo, MigrationResult, MigrationStatus, Migrator,
    Schema, async_trait,
};

// Relations. `relations::EagerLoadModel` is left out on purpose: it is
// `#[doc(hidden)]` machinery that generated code names by its full path.
pub use crate::relations::{
    BelongsTo, EagerQueryBuilder, HasMany, HasManyThrough, HasOne, MorphMany, MorphOne, MorphTo,
    RelationPath, RelationTree, SelfRef, SelfRefMany, WithRelations,
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

// Query and statement caching
pub use crate::cache::{
    CacheConfig, CacheKeyBuilder, CacheOptions, CacheStats, CacheStrategy, CachedStatementInfo,
    PreparedStatementCache, PreparedStatementConfig, PreparedStatementStats, QueryCache,
};

// Database seeding
pub use crate::seeding::{Seed, SeedInfo, SeedResult, SeedStatus, Seeder};

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
    Column, ColumnCondition, ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOrd,
    IntoColumnName,
};

// JPA-like entity manager / persistence context
#[cfg(feature = "entity-manager")]
pub use crate::entity_manager::{
    EntityManager, EntityManagerLoad, EntityState, Managed, TideEntityManagerMeta,
};

// Derive macro
pub use tideorm_macros::Model;

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
