//! Prelude module for TideORM
//!
//! This module re-exports the types most applications use frequently.
//! Import this when you want the common model, query, migration, and relation
//! types in scope without pulling each module in separately.

// Core types
pub use crate::database::{Database, DatabaseBuilder, Transaction};
// Global database access functions
pub use crate::database::{db, has_global_db, require_db, try_db};
pub use crate::error::Error;
// Note: We don't export Result here to avoid shadowing std::result::Result
// Use `tideorm::Result` explicitly when needed
pub use crate::callbacks::{CallbackRunner, Callbacks};
#[cfg(feature = "attachments")]
pub use crate::config::FileUrlGenerator;
pub use crate::config::{
    Config, DatabaseType, PoolConfig, RegisterMigrations, RegisterSeeds, TideConfig,
};
pub use crate::model::{
    BatchUpdateBuilder, CreateBuilder, IndexDefinition, Model, ModelMeta, NestedSave,
    NestedSaveBuilder, OnConflictBuilder, UpdateBuilder, UpdateValue,
};
pub use crate::query::{
    // CTE types
    CTE,
    FrameBound,
    FrameType,
    // Join result consolidation
    JoinResultConsolidator,
    JoinType,
    LogicalOp,
    OrBranch,
    OrBranchBuilder,
    // OR clause types
    OrGroup,
    Order,
    QueryBuilder,
    // Query fragment for consolidate()
    QueryFragment,
    UnionClause,
    // UNION types
    UnionType,
    // Window function types
    WindowFunction,
    WindowFunctionType,
};
pub use crate::schema::{
    ColumnSchema, SchemaGenerator, SchemaWriter, TableSchema, TableSchemaBuilder,
};
pub use crate::soft_delete::SoftDelete;
pub use crate::sync::{RegisterModels, SyncModel};

// Migrations
pub use crate::migration::{
    ColumnType,
    CompositePrimaryKey,
    DefaultValue,
    Migration,
    MigrationInfo,
    MigrationResult,
    MigrationStatus,
    Migrator,
    Schema,
    // Multi-column constraint types
    UniqueConstraint,
    async_trait,
};

// Relations
pub use crate::relations::{
    // Basic relations
    BelongsTo,
    EagerLoadExt,
    EagerQueryBuilder,
    HasMany,
    // Many-to-many relations
    HasManyThrough,
    HasOne,
    MorphMany,
    MorphOne,
    MorphResult,
    MorphResult3,
    MorphResult4,
    // Polymorphic relations
    MorphTo,
    // Constraints
    RelationConstraints,
    // Extension traits
    RelationExt,
    // Metadata
    RelationInfo,
    RelationLoader,
    RelationPath,
    RelationTree,
    RelationType,
    // Self-referencing relations
    SelfRef,
    SelfRefMany,
    WithPivot,
    // Eager loading
    WithRelations,
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
    CacheConfig, CacheKeyBuilder, CacheOptions, CacheStats, CacheStrategy, CacheWarmer,
    CachedStatementInfo, PreparedStatementCache, PreparedStatementConfig, PreparedStatementStats,
    QueryCache,
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
    Column, ColumnCondition, ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOperator,
    ColumnOrd, IntoColumnName,
};

// JPA-like entity manager / persistence context
#[cfg(feature = "entity-manager")]
pub use crate::entity_manager::{
    EntityManager, EntityManagerLoad, EntityState, Managed, TideEntityManagerMeta,
    save_with_entity_manager,
};

// Derive macro
pub use tideorm_macros::Model;

// Attribute macro
pub use tideorm_macros::model;

// Common external types users will need
pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value as JsonValue, json};

// Date/time types
pub use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

// Other common types
pub use rust_decimal::Decimal;
pub use uuid::Uuid;

// Type aliases and casting
pub use crate::types::{
    Accessor,
    AttributeCaster,
    BigIntArray,
    BoolArray,
    CastType,
    CastValue,
    Castable,
    Collection,
    CommaSeparated,
    DbEnum,
    // Casting types
    Encrypted,
    FloatArray,
    Hashed,
    // Array types
    IntArray,
    Json,
    JsonArray,
    Jsonb,
    Mutator,
    Text,
    TextArray,
    // Unix timestamp types
    UnixTimestamp,
    UnixTimestampMillis,
    WithDefault,
};
