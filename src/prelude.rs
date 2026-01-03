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
pub use crate::error::{Error, ValidationErrors};
// Note: We don't export Result here to avoid shadowing std::result::Result
// Use `tideorm::Result` explicitly when needed
pub use crate::model::{Model, ModelMeta, CreateBuilder, UpdateBuilder, IndexDefinition, OnConflictBuilder, BatchUpdateBuilder};
pub use crate::query::{QueryBuilder, Order, JoinType};
pub use crate::soft_delete::SoftDelete;
pub use crate::callbacks::{Callbacks, CallbackRunner};
pub use crate::config::{TideConfig, Config, PoolConfig, DatabaseType, RegisterMigrations};
pub use crate::schema::{SchemaGenerator, TableSchema, ColumnSchema, TableSchemaBuilder, SchemaWriter};
pub use crate::sync::{SyncModel, RegisterModels};

// Migrations
pub use crate::migration::{
    Migration, Migrator, Schema, ColumnType, DefaultValue,
    MigrationResult, MigrationInfo, MigrationStatus,
    async_trait,
};

// Relations
pub use crate::relations::{BelongsTo, HasOne, HasMany, RelationExt, EagerLoadExt, WithRelations};

// File Attachments
pub use crate::attachments::{HasAttachments, FileAttachment, FilesData, AttachmentError};

// Translations
pub use crate::translations::{HasTranslations, TranslationsData, FieldTranslations, TranslationInput, TranslationError, ApplyTranslations};

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

// Type aliases for convenience
pub use crate::types::{Json, Jsonb, Text, DbEnum, Castable};
