//! Prelude module for TideORM
//!
//! This module re-exports the most commonly used types and traits.
//! Import everything with:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! ```

// Core types
pub use crate::database::{Database, DatabaseBuilder, Transaction};
// Global database access functions
pub use crate::database::{db, try_db, has_global_db};
pub use crate::error::Error;
// Note: We don't export Result here to avoid shadowing std::result::Result
// Use `tideorm::Result` explicitly when needed
pub use crate::model::{Model, ModelMeta, CreateBuilder, UpdateBuilder, IndexDefinition, OnConflictBuilder, BatchUpdateBuilder, UpdateValue};
pub use crate::query::{
    QueryBuilder, Order, JoinType,
    // UNION types
    UnionType, UnionClause,
    // Window function types
    WindowFunction, WindowFunctionType, FrameBound, FrameType,
    // CTE types
    CTE,
};
pub use crate::soft_delete::SoftDelete;
pub use crate::callbacks::{Callbacks, CallbackRunner};
pub use crate::config::{TideConfig, Config, PoolConfig, DatabaseType, RegisterMigrations, RegisterSeeds};
pub use crate::schema::{SchemaGenerator, TableSchema, ColumnSchema, TableSchemaBuilder, SchemaWriter};
pub use crate::sync::{SyncModel, RegisterModels};

// Migrations
pub use crate::migration::{
    Migration, Migrator, Schema, ColumnType, DefaultValue,
    MigrationResult, MigrationInfo, MigrationStatus,
    async_trait,
};

// Relations
pub use crate::relations::{
    // Basic relations
    BelongsTo, HasOne, HasMany, 
    // Many-to-many relations
    HasManyThrough, WithPivot,
    // Polymorphic relations
    MorphTo, MorphOne, MorphMany, MorphResult, MorphResult3, MorphResult4,
    // Extension traits
    RelationExt, EagerLoadExt, 
    // Eager loading
    WithRelations, EagerQueryBuilder, RelationTree, RelationPath,
    // Constraints
    RelationConstraints,
    // Metadata
    RelationInfo, RelationType,
    RelationLoader,
};

// File Attachments
pub use crate::attachments::{HasAttachments, FileAttachment, FilesData, AttachmentError};

// Translations
pub use crate::translations::{HasTranslations, TranslationsData, FieldTranslations, TranslationInput, TranslationError, ApplyTranslations};

// Query logging and debugging
pub use crate::logging::{QueryLogger, LogLevel, QueryLogEntry, QueryTimer, QueryStats, QueryDebugInfo, QueryOperation};

// Performance profiling
pub use crate::profiling::{Profiler, ProfileReport, ProfiledQuery, GlobalProfiler, GlobalStats, QueryAnalyzer, QuerySuggestion, QueryComplexity, SuggestionLevel};

// Query and statement caching
pub use crate::cache::{
    QueryCache, PreparedStatementCache, 
    CacheConfig, CacheStrategy, CacheStats,
    PreparedStatementStats, PreparedStatementConfig,
    CacheKeyBuilder, CacheOptions, CachedStatementInfo, CacheWarmer,
};

// Database seeding
pub use crate::seeding::{
    Seed, Seeder, SeedResult, SeedInfo, SeedStatus,
};

// Validation
pub use crate::validation::{Validate, ValidationErrors, ValidationRule, ValidationBuilder, Validator, ValidatableValue};

// Full-text search
pub use crate::fulltext::{
    FullTextSearch, FullTextSearchBuilder, FullTextConfig, FullTextIndex,
    SearchMode, SearchResult, HighlightedField, SearchWeights,
    FullTextIndexConfig, PgFullTextIndexType, HighlightConfig,
    highlight_text, generate_snippet, pg_headline_sql,
};

// Derive macro
pub use tideorm_macros::Model;

// Relation attribute macros
pub use tideorm_macros::{belongs_to, has_one, has_many};

// Common external types users will need
pub use serde::{Deserialize, Serialize};
pub use serde_json::{json, Value as JsonValue};

// Date/time types
pub use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

// Other common types
pub use uuid::Uuid;
pub use rust_decimal::Decimal;

// Type aliases and casting
pub use crate::types::{
    Json, Jsonb, Text, DbEnum, Castable,
    // Casting types
    Encrypted, Hashed, CommaSeparated, Collection, CastType, CastValue,
    WithDefault, AttributeCaster, Accessor, Mutator,
    // Array types
    IntArray, BigIntArray, TextArray, BoolArray, FloatArray, JsonArray,
};
