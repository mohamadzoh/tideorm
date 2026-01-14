//! # TideORM
//!
//! A developer-friendly ORM for Rust with clean, expressive syntax.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! #[derive(Model)]
//! #[tide(table = "users")]
//! pub struct User {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub email: String,
//!     pub name: String,
//! }
//!
//! #[tokio::main]
//! async fn main() -> tideorm::Result<()> {
//!     // Initialize TideORM with database and configuration
//!     TideConfig::init()
//!         .database("postgres://localhost/myapp")
//!         .max_connections(20)
//!         .min_connections(5)
//!         .connect()
//!         .await?;
//!     
//!     // Create a record
//!     let mut user = User {
//!         id: 0,
//!         email: "john@example.com".to_string(),
//!         name: "John Doe".to_string(),
//!     };
//!     user = user.save().await?;
//!     
//!     // Find by ID
//!     let user = User::find(1).await?;
//!     
//!     // Query with conditions
//!     let users = User::query()
//!         .where_eq("name", "John")
//!         .order_by("created_at", Order::Desc)
//!         .limit(10)
//!         .get()
//!         .await?;
//!     
//!     // Update
//!     user.name = "Jane Doe".to_string();
//!     let user = user.update().await?;
//!     
//!     // Delete
//!     user.delete().await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Features
//!
//! - **Clean Model Definitions** - Use `#[derive(Model)]` to define your models
//! - **Unified Configuration** - Database, pool settings, and app config in one place
//! - **Global Database Connection** - Initialize once, use anywhere
//! - **Production-Ready Pool Settings** - Configure min/max connections, timeouts
//! - **Fluent Query Builder** - Chain methods for readable queries
//! - **Type Safe** - Full Rust type safety without verbose syntax
//! - **Async First** - Built for async/await from the ground up
//! - **Database Agnostic** - PostgreSQL, MySQL, SQLite support
//! - **Zero SeaORM Exposure** - SeaORM is an internal implementation detail
//! - **Query Logging & Debugging** - Built-in query logging with timing and slow query detection
//! - **Performance Profiling** - Query analysis and optimization suggestions
//! - **Helpful Error Messages** - Clear errors with suggestions for fixes
//!
//! ## Query Logging
//!
//! Enable query logging for debugging:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Enable via environment variable
//! // TIDE_LOG_QUERIES=true cargo run
//!
//! // Or enable programmatically
//! QueryLogger::global()
//!     .set_level(LogLevel::Debug)
//!     .set_slow_query_threshold_ms(100)
//!     .enable();
//!
//! // Debug a specific query
//! let debug_info = User::query()
//!     .where_eq("active", true)
//!     .where_gt("age", 18)
//!     .debug();
//! println!("{}", debug_info);
//! ```
//!
//! ## Performance Profiling
//!
//! Profile query performance:
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//!
//! // Start profiling
//! let profiler = Profiler::start();
//!
//! // Execute queries
//! let users = User::all().await?;
//!
//! // Get report
//! let report = profiler.stop();
//! println!("{}", report);
//! ```
//!
//! ## Model Definition
//!
//! Models are defined using the `#[derive(Model)]` macro with field-level attributes:
//!
//! ```rust,ignore
//! #[derive(Model)]
//! #[tide(table = "posts", soft_delete)]
//! #[index("user_id")]
//! #[unique_index("slug")]
//! pub struct Post {
//!     #[tide(primary_key, auto_increment)]
//!     pub id: i64,
//!     
//!     pub user_id: i64,
//!     pub slug: String,
//!     pub title: String,
//!     pub content: String,
//!     pub published: bool,
//!     
//!     // Auto-managed timestamps
//!     pub created_at: DateTime<Utc>,
//!     pub updated_at: DateTime<Utc>,
//!     
//!     // Soft delete support
//!     pub deleted_at: Option<DateTime<Utc>>,
//!
//!     // JSON and array fields (PostgreSQL)
//!     pub metadata: Json,                    // JSONB column
//!     pub tags: TextArray,                   // TEXT[] column
//!     pub scores: IntArray,                  // INTEGER[] column
//! }
//! ```
//!
//! ### JSON and Array Field Types
//!
//! TideORM supports PostgreSQL's native JSON and array types:
//!
//! | Rust Type | PostgreSQL Type | Description |
//! |-----------|-----------------|-------------|
//! | `Json` | `JSONB` | JSON data storage |
//! | `Jsonb` | `JSONB` | Alias for Json |
//! | `IntArray` | `INTEGER[]` | Array of integers |
//! | `BigIntArray` | `BIGINT[]` | Array of big integers |
//! | `TextArray` | `TEXT[]` | Array of strings |
//! | `BoolArray` | `BOOLEAN[]` | Array of booleans |
//! | `FloatArray` | `DOUBLE PRECISION[]` | Array of floats |
//! | `JsonArray` | `JSONB[]` | Array of JSON objects |
//!
//! ### Field Attributes
//!
//! | Attribute | Description |
//! |-----------|-------------|
//! | `#[tide(primary_key)]` | Marks the primary key field |
//! | `#[tide(auto_increment)]` | Auto-incrementing field (usually with primary_key) |
//! | `#[tide(column = "name")]` | Override the column name |
//! | `#[tide(skip)]` | Skip this field in database operations |
//!
//! ### Table Attributes
//!
//! | Attribute | Description |
//! |-----------|-------------|
//! | `#[tide(table = "name")]` | Override the table name |
//! | `#[tide(soft_delete)]` | Enable soft delete support |
//! | `#[index("col1,col2")]` | Create a composite index |
//! | `#[unique_index("col")]` | Create a unique index |
//!
//! ## Query Builder
//!
//! The fluent query builder supports all common operations:
//!
//! ```rust,ignore
//! // WHERE conditions
//! User::query()
//!     .where_eq("status", "active")      // WHERE status = 'active'
//!     .where_not("role", "banned")       // AND role != 'banned'
//!     .where_in("tier", vec!["gold", "platinum"])  // AND tier IN (...)
//!     .where_like("email", "%@company.com")       // AND email LIKE ...
//!     .where_null("deleted_at")          // AND deleted_at IS NULL
//!     .where_between("age", 18, 65)      // AND age BETWEEN 18 AND 65
//!     .get()
//!     .await?;
//!
//! // Ordering, pagination
//! User::query()
//!     .order_by("created_at", Order::Desc)
//!     .limit(10)
//!     .offset(20)
//!     .get()
//!     .await?;
//!
//! // Pagination helper
//! let page = User::query()
//!     .where_eq("active", true)
//!     .page(2, 25)  // Page 2, 25 per page
//!     .get()
//!     .await?;
//!
//! // Counting
//! let count = User::query()
//!     .where_eq("active", true)
//!     .count()
//!     .await?;
//!
//! // Bulk delete
//! let deleted = User::query()
//!     .where_eq("status", "inactive")
//!     .delete()
//!     .await?;  // Returns number of deleted rows
//! ```
//!
//! ### JSON Operations (PostgreSQL)
//!
//! Query JSON and JSONB columns with native PostgreSQL operators:
//!
//! ```rust,ignore
//! // JSON containment: column @> value
//! User::query()
//!     .where_json_contains("metadata", serde_json::json!({"role": "admin"}))
//!     .get()
//!     .await?;
//!
//! // JSON key existence: column ? key
//! User::query()
//!     .where_json_key_exists("settings", "theme")
//!     .get()
//!     .await?;
//!
//! // JSON path queries: column @? path
//! User::query()
//!     .where_json_path_exists("preferences", "$.notifications.email")
//!     .get()
//!     .await?;
//! ```
//!
//! ### Array Operations (PostgreSQL)
//!
//! Query array columns with native PostgreSQL operators:
//!
//! ```rust,ignore
//! // Array containment: column @> ARRAY[values]
//! User::query()
//!     .where_array_contains("tags", vec!["rust", "postgres"])
//!     .get()
//!     .await?;
//!
//! // Array overlap (contains any): column && ARRAY[values]
//! User::query()
//!     .where_array_overlaps("skills", vec!["javascript", "react"])
//!     .get()
//!     .await?;
//!
//! // Array contains all: ARRAY[values] <@ column
//! User::query()
//!     .where_array_contains_all("permissions", vec!["read", "write"])
//!     .get()
//!     .await?;
//! ```
//!
//! ### Conditional Queries (Scopes)
//!
//! ```rust,ignore
//! // Build queries conditionally
//! User::query()
//!     .when(filter.is_some(), |q| q.where_eq("status", filter.unwrap()))
//!     .when_some(search_term, |q, term| q.where_like("name", format!("%{}%", term)))
//!     .get()
//!     .await?;
//! ```
//!
//! ## Soft Delete
//!
//! Models with `#[tide(soft_delete)]` support soft deletion:
//!
//! ```rust,ignore
//! // Regular queries exclude soft-deleted records
//! let users = User::all().await?;
//!
//! // Include soft-deleted records
//! let all_users = User::query().with_trashed().get().await?;
//!
//! // Only soft-deleted records
//! let deleted = User::query().only_trashed().get().await?;
//!
//! // Soft delete
//! user.soft_delete().await?;
//!
//! // Restore
//! user.restore().await?;
//!
//! // Force delete (permanent)
//! user.force_delete().await?;
//! ```
//!
//! ## Callbacks
//!
//! Implement lifecycle hooks for your models:
//!
//! ```rust,ignore
//! impl Callbacks for User {
//!     fn before_save(&mut self) -> tideorm::Result<()> {
//!         // Normalize email before saving
//!         self.email = self.email.to_lowercase();
//!         Ok(())
//!     }
//!     
//!     fn after_create(&self) -> tideorm::Result<()> {
//!         // Send welcome email
//!         println!("User {} created!", self.email);
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Available callbacks:
//! - `before_validation`, `after_validation`
//! - `before_save`, `after_save`
//! - `before_create`, `after_create`
//! - `before_update`, `after_update`
//! - `before_delete`, `after_delete`
//!
//! ## Model Relations
//!
//! Define relationships between models:
//!
//! ```rust,ignore
//! // BelongsTo: Post belongs to User
//! impl BelongsTo<User> for Post {
//!     fn foreign_key() -> &'static str { "user_id" }
//! }
//!
//! // HasMany: User has many Posts
//! impl HasMany<Post> for User {
//!     fn foreign_key() -> &'static str { "user_id" }
//! }
//!
//! // Load relations
//! let post = Post::find(1).await?;
//! let author: User = post.load_belongs_to().await?;
//!
//! let user = User::find(1).await?;
//! let posts: Vec<Post> = user.load_has_many().await?;
//! ```
//!
//! ## Batch Operations
//!
//! ```rust,ignore
//! // Insert many records at once
//! let users = vec![user1, user2, user3];
//! let inserted = User::insert_all(users).await?;
//!
//! // Update many records with conditions
//! User::update_all()
//!     .set("status", "inactive")
//!     .where_eq("last_login_at", None::<DateTime<Utc>>)
//!     .execute()
//!     .await?;
//! ```
//!
//! ## Raw SQL
//!
//! When you need raw SQL access:
//!
//! ```rust,ignore
//! // Execute raw SQL
//! Database::execute("TRUNCATE TABLE temp_data").await?;
//!
//! // Query with parameters
//! let users: Vec<User> = Database::raw(
//!     "SELECT * FROM users WHERE status = $1 AND age > $2"
//! ).await?;
//! ```
//!
//! ## Schema Generation
//!
//! Auto-generate schema files from database introspection:
//!
//! ```rust,ignore
//! // Configure to auto-generate schema on connect
//! TideConfig::init()
//!     .database("postgres://localhost/myapp")
//!     .schema_file("schema.sql")  // Auto-generates on connect
//!     .connect()
//!     .await?;
//!
//! // Or generate manually
//! SchemaWriter::write_schema("schema.sql").await?;
//! ```
//!
//! ## Multi-Database Support
//!
//! TideORM supports PostgreSQL, MySQL, and SQLite:
//!
//! ```rust,ignore
//! // PostgreSQL
//! TideConfig::init()
//!     .database_type(DatabaseType::Postgres)
//!     .database("postgres://user:pass@localhost/db")
//!     .connect()
//!     .await?;
//!
//! // MySQL
//! TideConfig::init()
//!     .database_type(DatabaseType::MySQL)
//!     .database("mysql://user:pass@localhost/db")
//!     .connect()
//!     .await?;
//!
//! // SQLite
//! TideConfig::init()
//!     .database_type(DatabaseType::SQLite)
//!     .database("sqlite:./data.db")
//!     .connect()
//!     .await?;
//! ```
//!
//! ## Connection Pool Configuration
//!
//! ```rust,ignore
//! TideConfig::init()
//!     .database("postgres://localhost/myapp")
//!     .max_connections(50)           // Maximum connections
//!     .min_connections(5)            // Minimum idle connections
//!     .connect_timeout(Duration::from_secs(30))
//!     .idle_timeout(Duration::from_secs(600))
//!     .max_lifetime(Duration::from_secs(1800))
//!     .connect()
//!     .await?;
//! ```
//!
//! ## Query Logging
//!
//! Enable query logging for debugging:
//!
//! ```bash
//! TIDE_LOG_QUERIES=true cargo run
//! ```
//!
//! ## Error Handling
//!
//! TideORM provides descriptive error types:
//!
//! ```rust,ignore
//! match User::find(999).await {
//!     Ok(user) => println!("Found: {}", user.name),
//!     Err(Error::NotFound { message, .. }) => println!("User not found: {}", message),
//!     Err(Error::Connection { message }) => println!("Database error: {}", message),
//!     Err(e) => println!("Other error: {}", e),
//! }
//! ```
//!
//! ## Design Philosophy
//!
//! TideORM is designed with these principles:
//!
//! 1. **Convention over Configuration** - Smart defaults, minimal boilerplate
//! 2. **Developer Happiness** - APIs that feel natural and are hard to misuse
//! 4. **Type Safety** - Catch errors at compile time when possible
//! 5. **Performance** - Zero-cost abstractions where possible

#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)]

#[doc(hidden)]
pub mod internal;

/// Re-export sea_orm for internal macro use only
#[doc(hidden)]
pub use sea_orm;

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

/// File attachments system (attach, detach, sync)
pub mod attachments;

/// Translations system for multi-language support
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
pub mod fulltext;

/// Strongly-typed columns for compile-time type safety
pub mod columns;

/// Re-exports for convenience
pub mod prelude;

// ============================================================================
// PUBLIC RE-EXPORTS
// ============================================================================

pub use database::Database;
// Global database access functions
pub use database::{db, try_db, has_global_db};
pub use error::{Error, Result};
pub use model::{Model, ModelMeta};
pub use query::{QueryBuilder, Order, JoinType, JoinClause, AggregateFunction};
pub use soft_delete::SoftDelete;
pub use config::{TideConfig, Config};
pub use callbacks::{Callbacks, CallbackRunner};
pub use relations::{BelongsTo, HasOne, HasMany, RelationExt, EagerLoadExt, WithRelations};
pub use attachments::{HasAttachments, FileAttachment, FilesData, AttachmentError};
pub use translations::{HasTranslations, TranslationsData, FieldTranslations, TranslationInput, TranslationError, ApplyTranslations};
pub use schema::SchemaWriter;
pub use migration::{Migration, Migrator, Schema, ColumnType};

// Query logging and debugging
pub use logging::{QueryLogger, LogLevel, QueryLogEntry, QueryTimer, QueryStats, QueryDebugInfo, QueryOperation};

// Performance profiling
pub use profiling::{Profiler, ProfileReport, ProfiledQuery, GlobalProfiler, GlobalStats, QueryAnalyzer, QuerySuggestion, QueryComplexity, SuggestionLevel};

// Query and statement caching
pub use cache::{QueryCache, PreparedStatementCache, CacheConfig, CacheStrategy, CacheStats, PreparedStatementStats, PreparedStatementConfig, CacheKeyBuilder, CacheOptions, CachedStatementInfo, CacheWarmer};

// Validation
pub use validation::{Validate, ValidationErrors, ValidationRule, ValidationBuilder, Validator, ValidatableValue};

// Re-export the derive macro
pub use tideorm_macros::Model;

// Re-export the attribute macro
pub use tideorm_macros::model;

// Re-export relation attribute macros
pub use tideorm_macros::{belongs_to, has_one, has_many};

// Re-export async_trait for macro use
pub use async_trait;

// Re-export chrono for macro use (timestamps)
pub use chrono;

// Re-export common types that users need
pub use serde::{Deserialize, Serialize};
pub use chrono::{DateTime, NaiveDateTime, Utc};
pub use uuid::Uuid;
pub use rust_decimal::Decimal;

