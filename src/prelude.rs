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
// Bound parameter values for `Database::raw_with_params` and friends. Exported
// here so raw SQL never sends callers into the hidden `internal` module.
pub use crate::internal::DbValue;
pub use crate::model::{
    BatchUpdateBuilder, CreateBuilder, IndexDefinition, Model, ModelMeta, NestedSave,
    NestedSaveBuilder, OnConflictBuilder, UpdateBuilder, UpdateValue,
};
pub use crate::query::{
    // Aggregate terminals
    AggregateFunction,
    // CTE types
    CTE,
    FrameBound,
    FrameType,
    // Joins
    JoinClause,
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
pub use tideorm_macros::scopes;

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
    BigIntArray,
    BoolArray,
    CastType,
    CastValue,
    Castable,
    FloatArray,
    Hashed,
    // Array types
    IntArray,
    Json,
    JsonArray,
    Jsonb,
    Text,
    TextArray,
    // Unix timestamp types
    UnixTimestamp,
    UnixTimestampMillis,
};

#[cfg(test)]
mod prelude_surface_tests {
    //! Regression guard for crate-root/prelude drift.
    //!
    //! `AggregateFunction` and `JoinClause` were re-exported from the crate
    //! root only. Naming each type through both paths fails to compile if one
    //! surface loses an export.

    #[test]
    fn query_helpers_are_exported_from_both_surfaces() {
        fn assert_exported<T>() {}

        assert_exported::<crate::AggregateFunction>();
        assert_exported::<crate::prelude::AggregateFunction>();
        assert_exported::<crate::JoinClause>();
        assert_exported::<crate::prelude::JoinClause>();
        assert_exported::<crate::JoinType>();
        assert_exported::<crate::prelude::JoinType>();
        assert_exported::<crate::Order>();
        assert_exported::<crate::prelude::Order>();
    }

    /// The raw-SQL `*_with_params` entry points are only callable if their
    /// parameter type has a name outside the hidden `internal` module.
    #[test]
    fn raw_sql_parameters_are_nameable_without_touching_internal() {
        let params: Vec<crate::DbValue> = vec![
            crate::prelude::DbValue::BigInt(Some(7)),
            true.into(),
            "alice".into(),
        ];

        assert_eq!(params.len(), 3);
    }
}
