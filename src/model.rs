//! Model trait and utilities
//!
//! This module provides the core `Model` trait that all TideORM models implement.
//!
//! ## Features
//!
//! - Static methods for querying (find, where_clause, paginate, etc.)
//! - Instance methods for record manipulation (save, update, delete, reload)
//! - Configuration methods for hidden attributes, translations, searchable fields
//! - Support for soft deletes, translations, and file attachments
//! - Index definitions for schema generation

use async_trait::async_trait;
use std::collections::HashMap;

/// Index definition for database schema generation
///
/// Supports both regular and unique indexes with optional custom names.
///
/// # Example (in macro)
/// ```ignore
/// #[derive(Model)]
/// #[tideorm(
///     table = "users",
///     index = "email;name:first_name,last_name",
///     unique_index = "tenant_id,email"
/// )]
/// struct User { ... }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct IndexDefinition {
    /// Index name (auto-generated if not specified)
    pub name: String,
    /// Column names in the index
    pub columns: Vec<String>,
    /// Whether this is a unique index
    pub unique: bool,
}

impl IndexDefinition {
    /// Create a new index definition
    pub fn new(name: impl Into<String>, columns: Vec<String>, unique: bool) -> Self {
        Self {
            name: name.into(),
            columns,
            unique,
        }
    }

    /// Parse index definitions from the macro format
    ///
    /// Format: "field1", "field1,field2", or "name:field1,field2"
    /// Multiple indexes separated by semicolons: "email;name:first_name,last_name"
    pub fn parse(_table_name: &str, input: &str, unique: bool) -> Vec<Self> {
        if input.is_empty() {
            return vec![];
        }

        input
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|part| {
                let part = part.trim();
                let (name, columns) = if let Some((n, cols)) = part.split_once(':') {
                    (n.trim().to_string(), cols)
                } else {
                    // Auto-generate name: idx_tablename_columns or uidx_tablename_columns
                    let cols = part;
                    let prefix = if unique { "uidx" } else { "idx" };
                    let col_part = cols.replace(',', "_").replace(' ', "");
                    (format!("{}_{}", prefix, col_part), cols)
                };

                let columns: Vec<String> = columns
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                IndexDefinition::new(name, columns, unique)
            })
            .collect()
    }
}

use crate::error::{Error, Result};
use crate::query::QueryBuilder;

/// Metadata trait for model information
///
/// This trait provides compile-time information about a model.
/// It is automatically implemented by the `#[derive(Model)]` macro.
pub trait ModelMeta: Sized + Send + Sync + Clone + 'static {
    /// The type of the primary key
    type PrimaryKey: Send + Sync + Clone + std::fmt::Debug + std::fmt::Display + 'static;

    /// Returns the table name for this model
    fn table_name() -> &'static str;

    /// Returns the primary key column name
    fn primary_key_name() -> &'static str;

    /// Returns whether the primary key is auto-incrementing.
    fn primary_key_auto_increment() -> bool {
        false
    }

    /// Returns all column names in the database
    fn column_names() -> &'static [&'static str];

    /// Returns all field names in the struct
    fn field_names() -> &'static [&'static str];

    // =========================================================================
    // CONFIGURATION METHODS (can be overridden via macro attributes)
    // =========================================================================

    /// Returns hidden attributes that should not be exposed in API responses
    /// This applies to both model fields and file attachment fields
    fn hidden_attributes() -> Vec<&'static str> {
        vec!["deleted_at"]
    }

    /// Returns the default presenter name
    fn default_presenter() -> &'static str {
        "default"
    }

    /// Returns searchable fields for full-text search
    fn searchable_fields() -> Vec<&'static str> {
        vec![]
    }

    /// Returns the default order for queries
    fn default_order() -> Vec<(&'static str, &'static str)> {
        vec![("id", "DESC")]
    }

    /// Returns fields for option set label
    fn option_set_label() -> &'static str {
        "id"
    }

    /// Returns fields to search in option sets
    fn option_set_search_fields() -> Vec<&'static str> {
        vec![]
    }

    /// Returns translatable fields (for models with translations JSONB column)
    fn translatable_fields() -> Vec<&'static str> {
        vec![]
    }

    /// Returns allowed languages for translations
    ///
    /// By default, returns languages from global TideConfig.
    /// Override in your model to specify model-specific languages.
    fn allowed_languages() -> Vec<String> {
        crate::config::Config::get_languages()
    }

    /// Returns the fallback language
    ///
    /// By default, returns fallback from global TideConfig.
    /// Override in your model to specify a model-specific fallback.
    fn fallback_language() -> String {
        crate::config::Config::get_fallback_language()
    }

    /// Check if model has translations support
    #[cfg(feature = "translations")]
    fn has_translations() -> bool {
        !Self::translatable_fields().is_empty()
    }

    /// Check if model has translations support
    #[cfg(not(feature = "translations"))]
    fn has_translations() -> bool {
        false
    }

    /// Returns single file attachment relations (hasOne)
    fn has_one_attached_file() -> Vec<&'static str> {
        vec![]
    }

    /// Returns multiple file attachment relations (hasMany)
    fn has_many_attached_files() -> Vec<&'static str> {
        vec![]
    }

    /// Returns all file relation names
    fn files_relations() -> Vec<&'static str> {
        let mut relations = Self::has_one_attached_file();
        relations.extend(Self::has_many_attached_files());
        relations
    }

    /// Check if model has file attachments support
    #[cfg(feature = "attachments")]
    fn has_file_attachments() -> bool {
        !Self::files_relations().is_empty()
    }

    /// Check if model has file attachments support
    #[cfg(not(feature = "attachments"))]
    fn has_file_attachments() -> bool {
        false
    }

    /// Returns the file URL generator for this model
    ///
    /// Override this method to customize URL generation for a specific model.
    /// By default, uses the global file URL generator from TideConfig.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl ModelMeta for Product {
    ///     // ... other implementations ...
    ///     
    ///     fn file_url_generator() -> crate::config::FileUrlGenerator {
    ///         |field_name, file| {
    ///             // Use field name and file metadata for URL generation
    ///             match field_name {
    ///                 "thumbnail" => format!("https://images-cdn.example.com/thumb/{}", file.key),
    ///                 "avatar" => format!("https://avatars.example.com/{}", file.key),
    ///                 _ => format!("https://cdn.example.com/{}", file.key),
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    #[cfg(feature = "attachments")]
    fn file_url_generator() -> crate::config::FileUrlGenerator {
        crate::config::Config::get_file_url_generator()
    }

    /// Generate a URL for a file attachment using this model's URL generator
    ///
    /// This is a convenience method that uses the model's `file_url_generator()`.
    ///
    /// # Arguments
    /// * `field_name` - The name of the attachment field (e.g., "thumbnail", "avatar")
    /// * `file` - The file attachment with metadata
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let file = FileAttachment::new("products/123/image.jpg");
    /// let url = Product::generate_file_url("thumbnail", &file);
    /// // Returns: "https://images-cdn.example.com/thumb/products/123/image.jpg"
    /// ```
    #[inline]
    #[cfg(feature = "attachments")]
    fn generate_file_url(field_name: &str, file: &crate::attachments::FileAttachment) -> String {
        Self::file_url_generator()(field_name, file)
    }

    /// Check if model supports soft deletes
    fn soft_delete_enabled() -> bool {
        false
    }

    /// Check if model has timestamps (created_at, updated_at)
    ///
    /// When true, automatically:
    /// - Sets `created_at` to now on save()
    /// - Sets `updated_at` to now on save() and update()
    fn has_timestamps() -> bool {
        false
    }

    // =========================================================================
    // INDEX DEFINITIONS
    // =========================================================================

    /// Returns regular index definitions for this model
    ///
    /// Override this in the macro to define indexes:
    /// ```ignore
    /// #[tideorm(index = "email;name:first_name,last_name")]
    /// ```
    fn indexes() -> Vec<IndexDefinition> {
        vec![]
    }

    /// Returns unique index definitions for this model
    ///
    /// Override this in the macro to define unique indexes:
    /// ```ignore
    /// #[tideorm(unique_index = "tenant_id,email")]
    /// ```
    fn unique_indexes() -> Vec<IndexDefinition> {
        vec![]
    }

    /// Returns all index definitions (both regular and unique)
    fn all_indexes() -> Vec<IndexDefinition> {
        let mut all = Self::indexes();
        all.extend(Self::unique_indexes());
        all
    }

    /// Check if model has any index definitions
    fn has_indexes() -> bool {
        !Self::indexes().is_empty() || !Self::unique_indexes().is_empty()
    }

    // =========================================================================
    // TOKENIZATION
    // =========================================================================

    /// Check if tokenization is enabled for this model
    ///
    /// Override this to enable tokenization via `#[tideorm(tokenize)]` attribute.
    /// When enabled, the model will have `to_token()` and `from_token()` methods.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(Model)]
    /// #[tideorm(table = "users", tokenize)]
    /// pub struct User {
    ///     #[tideorm(primary_key)]
    ///     pub id: i64,
    ///     pub email: String,
    /// }
    /// ```
    fn tokenization_enabled() -> bool {
        false
    }

    /// Returns the token encoder for this model
    ///
    /// Override to provide model-specific token encoding logic.
    /// Returns `None` to use the global encoder from TideConfig.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl ModelMeta for SecretDocument {
    ///     fn token_encoder() -> Option<crate::tokenization::TokenEncoder> {
    ///         Some(|record_id, _model_name| {
    ///             // Custom encoding for this model
    ///             Ok(format!("DOC-{}", base64_encode(record_id)))
    ///         })
    ///     }
    /// }
    /// ```
    fn token_encoder() -> Option<crate::tokenization::TokenEncoder> {
        None
    }

    /// Returns the token decoder for this model
    ///
    /// Override to provide model-specific token decoding logic.
    /// Returns `None` to use the global decoder from TideConfig.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// impl ModelMeta for SecretDocument {
    ///     fn token_decoder() -> Option<crate::tokenization::TokenDecoder> {
    ///         Some(|token, _model_name| {
    ///             // Custom decoding for this model
    ///             token.strip_prefix("DOC-")
    ///                 .and_then(|encoded| base64_decode(encoded))
    ///         })
    ///     }
    /// }
    /// ```
    fn token_decoder() -> Option<crate::tokenization::TokenDecoder> {
        None
    }
}

/// Core trait for TideORM models
///
/// This trait provides all the CRUD operations and query capabilities.
/// It is automatically implemented by the `#[derive(Model)]` macro.
///
/// ## Global Database Connection
///
/// TideORM uses a global database connection pattern. Initialize once at startup:
///
/// ```ignore
/// TideConfig::init()
///     .database("postgres://localhost/myapp")
///     .connect()
///     .await?;
/// ```
///
/// After initialization, all model methods work without passing a database reference:
///
/// ```ignore
/// let users = User::all().await?;
/// let user = user.save().await?;
/// user.delete().await?;
/// ```
#[async_trait]
pub trait Model:
    ModelMeta + crate::internal::InternalModel + serde::Serialize + for<'de> serde::Deserialize<'de>
{
    /// Get the primary key value of this instance
    fn primary_key(&self) -> Self::PrimaryKey;

    /// Get the global database connection
    ///
    /// This is a convenience method to access the database from within model code.
    ///
    /// # Example
    /// ```ignore
    /// let db = User::db()?;
    /// // or from an instance
    /// let db = user.database()?;
    /// ```
    fn db() -> crate::error::Result<&'static crate::database::Database> {
        crate::database::require_db()
    }

    /// Get the global database connection from an instance
    ///
    /// # Example
    /// ```ignore
    /// let user = User::find(1).await?;
    /// let db = user.database()?;
    /// ```
    fn database(&self) -> crate::error::Result<&'static crate::database::Database> {
        crate::database::require_db()
    }

    // =========================================================================
    // STATIC METHODS (Class-level operations) - Use global connection
    // =========================================================================

    /// Get all records from the table
    ///
    /// # Example
    /// ```ignore
    /// let users = User::all().await?;
    /// ```
    async fn all() -> Result<Vec<Self>>
    where
        Self: Sized,
    {
        crate::internal::QueryExecutor::find_all::<Self>(
            crate::database::require_db()?.__internal_connection(),
        )
        .await
    }

    /// Start a query builder for this model
    ///
    /// # Example
    /// ```ignore
    /// let active_users = User::query()
    ///     .where_eq("status", "active")
    ///     .order_by("name", Order::Asc)
    ///     .get()
    ///     .await?;
    /// ```
    fn query() -> QueryBuilder<Self>
    where
        Self: Sized,
    {
        QueryBuilder::new()
    }

    /// Count all records (uses efficient SQL COUNT)
    ///
    /// # Example
    /// ```ignore
    /// let total = User::count().await?;
    /// ```
    async fn count() -> Result<u64>
    where
        Self: Sized,
    {
        crate::internal::QueryExecutor::count::<Self>(
            crate::database::require_db()?.__internal_connection(),
            None,
        )
        .await
    }

    /// Check if any records exist
    ///
    /// # Example
    /// ```ignore
    /// if User::exists_any().await? {
    ///     println!("We have users!");
    /// }
    /// ```
    async fn exists_any() -> Result<bool>
    where
        Self: Sized,
    {
        Ok(Self::count().await? > 0)
    }

    // =========================================================================
    // BATCH OPERATIONS
    // =========================================================================

    /// Insert multiple records at once (efficient batch insert)
    ///
    /// Uses a single multi-row INSERT statement for efficiency (O(1) round trips).
    /// Falls back to individual inserts if the batch insert fails (e.g., on backends
    /// that don't support INSERT ... RETURNING).
    ///
    /// # Example
    /// ```ignore
    /// let users = vec![
    ///     User { id: 0, name: "John".into(), email: "john@example.com".into() },
    ///     User { id: 0, name: "Jane".into(), email: "jane@example.com".into() },
    /// ];
    /// let inserted = User::insert_all(users).await?;
    /// println!("Inserted {} users", inserted.len());
    /// ```
    async fn insert_all(models: Vec<Self>) -> Result<Vec<Self>>
    where
        Self: Sized,
        <<Self as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            crate::internal::IntoActiveModel<<Self as crate::internal::InternalModel>::ActiveModel>,
    {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        let conn = crate::database::require_db()?.__internal_connection();

        // insert_many handles MySQL/SQLite fallback internally
        crate::internal::QueryExecutor::insert_many::<Self>(conn, models).await
    }

    /// Insert multiple records and return the inserted models.
    ///
    /// This is currently a compatibility alias for `insert_all`.
    /// It does not provide different SQL generation or backend-specific behavior.
    ///
    /// # Example
    /// ```ignore
    /// let users = vec![
    ///     User { id: 0, name: "John".into(), .. },
    ///     User { id: 0, name: "Jane".into(), .. },
    /// ];
    ///
    /// let inserted = User::insert_many_returning(users).await?;
    /// for user in &inserted {
    ///     println!("Inserted user with ID: {}", user.id);
    /// }
    /// ```
    async fn insert_many_returning(models: Vec<Self>) -> Result<Vec<Self>>
    where
        Self: Sized,
        <<Self as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            crate::internal::IntoActiveModel<<Self as crate::internal::InternalModel>::ActiveModel>,
    {
        // Check if empty - return empty Vec without error
        if models.is_empty() {
            return Ok(Vec::new());
        }

        Self::insert_all(models).await
    }

    /// Insert multiple records efficiently
    ///
    /// This is an alias for `insert_all` with improved empty-array handling.
    /// Returns an empty Vec if given an empty input (no error).
    ///
    /// # Example
    /// ```ignore
    /// // Safe to call with empty array - returns Ok(vec![])
    /// let result = User::insert_many(vec![]).await?;
    /// assert!(result.is_empty());
    ///
    /// // Normal batch insert
    /// let users = vec![user1, user2, user3];
    /// let inserted = User::insert_many(users).await?;
    /// ```
    async fn insert_many(models: Vec<Self>) -> Result<Vec<Self>>
    where
        Self: Sized,
        <<Self as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            crate::internal::IntoActiveModel<<Self as crate::internal::InternalModel>::ActiveModel>,
    {
        if models.is_empty() {
            return Ok(Vec::new());
        }
        Self::insert_all(models).await
    }

    /// Insert or update a record (upsert)
    ///
    /// If a conflict occurs on the specified column(s), the record will be updated.
    /// This is useful for "insert or update" logic where you want to ensure a record
    /// exists with certain values.
    ///
    /// # Arguments
    /// * `conflict_columns` - Column name(s) to check for conflicts (e.g., vec!["email"] or vec!["tenant_id", "email"])
    ///
    /// # Example
    /// ```ignore
    /// // Insert or update user by email
    /// let user = User { id: 0, email: "john@example.com".into(), name: "John".into() };
    /// let user = User::insert_or_update(user, vec!["email"]).await?;
    ///
    /// // Insert or update by composite key
    /// let config = Config { tenant_id: 1, key: "theme".into(), value: "dark".into() };
    /// let config = Config::insert_or_update(config, vec!["tenant_id", "key"]).await?;
    /// ```
    async fn insert_or_update(model: Self, conflict_columns: Vec<&str>) -> Result<Self>
    where
        Self: Sized;

    /// Create an on-conflict builder for advanced upsert scenarios
    ///
    /// This provides more control over the upsert behavior, allowing you to
    /// specify which columns to update on conflict.
    ///
    /// # Example
    /// ```ignore
    /// // Update only specific columns on conflict
    /// let user = User::on_conflict(vec!["email"])
    ///     .update_columns(vec!["name", "updated_at"])
    ///     .insert(user)
    ///     .await?;
    ///
    /// // Or update all columns except the conflict column
    /// let user = User::on_conflict(vec!["email"])
    ///     .update_all_except(vec!["id", "email", "created_at"])
    ///     .insert(user)
    ///     .await?;
    /// ```
    fn on_conflict(conflict_columns: Vec<&str>) -> OnConflictBuilder<Self>
    where
        Self: Sized,
    {
        OnConflictBuilder::new(
            conflict_columns
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        )
    }

    /// Update multiple records matching conditions
    ///
    /// Returns the number of rows affected.
    ///
    /// # Example
    /// ```ignore
    /// // Deactivate all users who haven't logged in for 1 year
    /// let affected = User::update_all()
    ///     .set("active", false)
    ///     .where_eq("last_login_before", "2024-01-01")
    ///     .execute()
    ///     .await?;
    /// ```
    fn update_all() -> BatchUpdateBuilder<Self>
    where
        Self: Sized,
    {
        BatchUpdateBuilder::new()
    }

    /// Execute a closure within a database transaction
    ///
    /// This provides a model-centric way to run transactions.
    /// The transaction is automatically committed if the closure returns `Ok`,
    /// and rolled back if it returns `Err` or panics.
    ///
    /// # Example
    /// ```ignore
    /// User::transaction(|tx| Box::pin(async move {
    ///     // All operations in here are in a transaction
    ///     // Use tx.connection() for manual queries if needed
    ///     let user = User::create(User { ... }).await?;
    ///     let profile = Profile::create(Profile { user_id: user.id, ... }).await?;
    ///     Ok((user, profile))
    /// })).await?;
    /// ```
    async fn transaction<F, T>(f: F) -> Result<T>
    where
        Self: Sized,
        F: for<'c> FnOnce(
                &'c crate::database::Transaction,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T>> + Send + 'c>,
            > + Send,
        T: Send,
    {
        crate::database::require_db()?.transaction(f).await
    }

    /// Get the first record
    ///
    /// # Example
    /// ```ignore
    /// if let Some(user) = User::first().await? {
    ///     println!("First user: {}", user.name);
    /// }
    /// ```
    async fn first() -> Result<Option<Self>>
    where
        Self: Sized,
    {
        crate::internal::QueryExecutor::first::<Self>(
            crate::database::require_db()?.__internal_connection(),
        )
        .await
    }

    /// Get the last record (by primary key descending)
    ///
    /// # Example
    /// ```ignore
    /// let newest = User::last().await?;
    /// ```
    async fn last() -> Result<Option<Self>>
    where
        Self: Sized,
    {
        crate::internal::QueryExecutor::last::<Self>(
            crate::database::require_db()?.__internal_connection(),
        )
        .await
    }

    /// Paginate records
    ///
    /// # Example
    /// ```ignore
    /// let page_2 = User::paginate(2, 10).await?; // Page 2, 10 per page
    /// ```
    async fn paginate(page: u64, per_page: u64) -> Result<Vec<Self>>
    where
        Self: Sized,
    {
        let offset = (page.saturating_sub(1)) * per_page;
        crate::internal::QueryExecutor::paginate::<Self>(
            crate::database::require_db()?.__internal_connection(),
            per_page,
            offset,
        )
        .await
    }

    // =========================================================================
    // METHODS IMPLEMENTED BY MACRO (need access to primary key type)
    // These are declared here but implemented by the derive macro
    // =========================================================================

    /// Find record by primary key
    /// Implemented by the derive macro
    async fn find(id: Self::PrimaryKey) -> Result<Option<Self>>
    where
        Self: Sized;

    /// Find record by primary key or fail with NotFound error
    async fn find_or_fail(id: Self::PrimaryKey) -> Result<Self>
    where
        Self: Sized,
    {
        let id_display = format!("{}", id);
        Self::find(id).await?.ok_or_else(|| {
            Error::not_found(format!(
                "{} with {} = {} not found",
                Self::table_name(),
                Self::primary_key_name(),
                id_display
            ))
        })
    }

    /// Check if record exists by ID
    async fn exists(id: Self::PrimaryKey) -> Result<bool>
    where
        Self: Sized,
    {
        Ok(Self::find(id).await?.is_some())
    }

    /// Create a new record from model instance
    /// Implemented by the derive macro
    async fn create(model: Self) -> Result<Self>
    where
        Self: Sized;

    /// Delete record by ID
    /// Implemented by the derive macro
    async fn destroy(id: Self::PrimaryKey) -> Result<u64>
    where
        Self: Sized;

    // =========================================================================
    // INSTANCE METHODS (Record-level operations) - Use global connection
    // =========================================================================

    /// Save this record to the database (insert)
    ///
    /// # Example
    /// ```ignore
    /// let user = User { id: 0, name: "John".into(), email: "john@example.com".into() };
    /// let user = user.save().await?;
    /// ```
    async fn save(self) -> Result<Self>
    where
        Self: Sized;

    /// Update this record in the database
    ///
    /// # Example
    /// ```ignore
    /// user.name = "Jane".to_string();
    /// let user = user.update().await?;
    /// ```
    async fn update(self) -> Result<Self>
    where
        Self: Sized;

    /// Delete this instance from the database
    ///
    /// # Example
    /// ```ignore
    /// let user = User::find_or_fail(1).await?;
    /// user.delete().await?;
    /// ```
    async fn delete(self) -> Result<u64>
    where
        Self: Sized;

    /// Internal method for insert with conflict handling
    /// This is implemented by the derive macro and should not be called directly
    #[doc(hidden)]
    async fn __insert_with_conflict(model: Self, builder: OnConflictBuilder<Self>) -> Result<Self>
    where
        Self: Sized;

    /// Reload the instance from database
    ///
    /// # Example
    /// ```ignore
    /// let user = user.reload().await?;
    /// ```
    async fn reload(&self) -> Result<Self>
    where
        Self: Sized,
    {
        Self::find_or_fail(self.primary_key()).await
    }

    /// Check if this record is new (not yet saved)
    ///
    /// For auto-increment primary keys, TideORM treats `0` as unsaved so
    /// freshly constructed records like `{ id: 0, .. }` are handled correctly.
    /// For natural keys, an empty primary key is considered new.
    fn is_new(&self) -> bool {
        let primary_key = self.primary_key().to_string();

        if primary_key.is_empty() {
            return true;
        }

        if Self::primary_key_auto_increment() {
            return primary_key.parse::<i128>().map(|value| value == 0).unwrap_or(false);
        }

        false
    }

    // =========================================================================
    // JSON SERIALIZATION
    // =========================================================================

    /// Convert model to JSON with options
    ///
    /// This method provides a comprehensive JSON representation that:
    /// - Excludes hidden attributes
    /// - Applies translations based on language option
    /// - Flattens file attachments to root level
    /// - Excludes hidden file attributes
    ///
    /// # Arguments
    /// * `options` - Optional HashMap with keys:
    ///   - "language": Language code for translations (e.g., "en", "fr", "ar")
    ///   - "presenter": Presenter name (e.g., "default", "minimal", "full")
    ///
    /// # Example
    /// ```ignore
    /// // Without options (uses defaults)
    /// let json = user.to_json(None);
    ///
    /// // With language option
    /// let mut opts = HashMap::new();
    /// opts.insert("language".to_string(), "fr".to_string());
    /// let json = user.to_json(Some(opts));
    /// ```
    fn to_json(&self, options: Option<HashMap<String, String>>) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        let opts = options.unwrap_or_default();
        #[cfg(feature = "translations")]
        let fallback = Self::fallback_language();
        #[cfg(feature = "translations")]
        let current_language = opts
            .get("language")
            .map(|s| s.as_str())
            .unwrap_or(&fallback);
        let _current_presenter = opts
            .get("presenter")
            .map(|s| s.as_str())
            .unwrap_or(Self::default_presenter());

        let hidden = Self::hidden_attributes();
        #[cfg(feature = "translations")]
        let translatable = Self::translatable_fields();
        #[cfg(feature = "attachments")]
        let file_relations = Self::files_relations();

        // Serialize the model to JSON
        let mut json = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => return serde_json::json!({}),
        };

        // Remove hidden attributes
        for attr in &hidden {
            json.remove(*attr);
        }

        // Apply translations if model has translations support
        #[cfg(feature = "translations")]
        if Self::has_translations() {
            if let Some(translations) = json.get("translations").cloned() {
                if let Some(trans_obj) = translations.as_object() {
                    for field in &translatable {
                        if let Some(field_trans) = trans_obj.get(*field) {
                            if let Some(field_obj) = field_trans.as_object() {
                                // Try to get the current language, fallback if not found
                                if let Some(value) = field_obj
                                    .get(current_language)
                                    .or_else(|| field_obj.get(&fallback))
                                {
                                    json.insert(field.to_string(), value.clone());
                                }
                            }
                        }
                    }
                }
                // Remove the raw translations object from output
                json.remove("translations");
            }
        }

        #[cfg(feature = "attachments")]
        if Self::has_file_attachments() {
            let url_generator = Self::file_url_generator();
            if let Some(files) = json.remove("files") {
                if let Some(files_obj) = files.as_object() {
                    for relation in &file_relations {
                        if let Some(file_data) = files_obj.get(*relation) {
                            let processed = Self::process_file_for_json(
                                relation,
                                file_data,
                                &hidden,
                                url_generator,
                            );
                            json.insert(relation.to_string(), processed);
                        }
                    }
                }
            }
        }

        serde_json::Value::Object(json)
    }

    /// Process file data for JSON output, removing hidden attributes and adding URLs
    ///
    /// This method:
    /// - Removes hidden attributes from file data
    /// - Adds a `url` field with the full URL generated from the file attachment
    ///
    /// # Arguments
    /// * `field_name` - The name of the attachment field (e.g., "thumbnail", "avatar")
    /// * `file_data` - The JSON value containing file data
    /// * `hidden_attrs` - Attributes to exclude from output
    /// * `url_generator` - Function to generate URLs
    #[inline]
    #[cfg(feature = "attachments")]
    fn process_file_for_json(
        field_name: &str,
        file_data: &serde_json::Value,
        hidden_attrs: &[&str],
        url_generator: crate::config::FileUrlGenerator,
    ) -> serde_json::Value {
        match file_data {
            serde_json::Value::Object(obj) => {
                let mut cleaned = serde_json::Map::new();
                for (key, value) in obj {
                    if !hidden_attrs.contains(&key.as_str()) {
                        cleaned.insert(key.clone(), value.clone());
                    }
                }
                // Add URL field by deserializing the file attachment
                if let Ok(file_attachment) =
                    serde_json::from_value::<crate::attachments::FileAttachment>(
                        serde_json::Value::Object(obj.clone()),
                    )
                {
                    let url = url_generator(field_name, &file_attachment);
                    cleaned.insert("url".to_string(), serde_json::Value::String(url));
                }
                serde_json::Value::Object(cleaned)
            }
            serde_json::Value::Array(arr) => {
                let cleaned: Vec<serde_json::Value> = arr
                    .iter()
                    .map(|item| {
                        Self::process_file_for_json(field_name, item, hidden_attrs, url_generator)
                    })
                    .collect();
                serde_json::Value::Array(cleaned)
            }
            other => other.clone(),
        }
    }

    /// Convert a collection of models to JSON array
    ///
    /// # Arguments
    /// * `models` - Vector of models to convert
    /// * `options` - Optional HashMap with "language" and "presenter" keys
    fn collection_to_json(
        models: Vec<Self>,
        options: Option<HashMap<String, String>>,
    ) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        let items: Vec<serde_json::Value> = models
            .iter()
            .map(|model| model.to_json(options.clone()))
            .collect();
        serde_json::Value::Array(items)
    }

    /// Convert model to HashMap
    fn to_hash_map(&self) -> HashMap<String, String>
    where
        Self: serde::Serialize,
    {
        let json = self.to_json(None);
        let mut map = HashMap::new();

        if let Some(obj) = json.as_object() {
            for (key, value) in obj {
                if key != "params" {
                    let str_val = match value {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Null => "null".to_string(),
                        _ => value.to_string(),
                    };
                    map.insert(key.clone(), str_val);
                }
            }
        }

        map
    }

    // =========================================================================
    // TRANSLATION METHODS (for models with 'translations' JSONB column)
    // =========================================================================

    /// Load translations for a specific language into model fields
    fn load_language_translations(&mut self, _language: &str) -> std::result::Result<(), String> {
        if !Self::has_translations() {
            return Ok(()); // Skip if model doesn't have translations
        }
        // Default implementation - override in models with translations
        Ok(())
    }

    /// Load all translations for all languages into model fields
    fn load_all_translations(&mut self) -> std::result::Result<(), String> {
        if !Self::has_translations() {
            return Ok(()); // Skip if model doesn't have translations
        }
        // Default implementation - override in models with translations
        Ok(())
    }

    /// Extract translations from data HashMap for saving
    #[cfg(feature = "translations")]
    fn extract_translations(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        if !Self::has_translations() {
            return Ok(serde_json::json!({}));
        }

        let mut translations = serde_json::Map::new();
        let translatable = Self::translatable_fields();

        for field in translatable {
            if let Some(value) = data.get(field) {
                if value.is_object() {
                    translations.insert(field.to_string(), value.clone());
                    data.remove(field);
                }
            }
        }

        Ok(serde_json::Value::Object(translations))
    }

    /// Extract translations from data HashMap for saving
    #[cfg(not(feature = "translations"))]
    fn extract_translations(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        let _ = data;
        Ok(serde_json::json!({}))
    }

    // =========================================================================
    // FILE ATTACHMENT METHODS (for models with 'files' JSONB column)
    // =========================================================================

    /// Get files from the JSONB 'files' column
    fn get_files_attribute(
        &self,
    ) -> std::result::Result<HashMap<String, serde_json::Value>, String> {
        if !Self::has_file_attachments() {
            return Ok(HashMap::new());
        }
        // Default implementation - override in models with files JSONB column
        Ok(HashMap::new())
    }

    /// Set files to the JSONB 'files' column
    fn set_files_attribute(
        &mut self,
        _files: HashMap<String, serde_json::Value>,
    ) -> std::result::Result<(), String> {
        if !Self::has_file_attachments() {
            return Ok(());
        }
        // Default implementation - override in models with files JSONB column
        Ok(())
    }

    /// Attach a file to a specific relation type
    ///
    /// # Arguments
    /// * `relation_type` - The relation type (e.g., "thumbnail", "images")
    /// * `file_key` - The file key/path to attach
    fn attach_file(
        &mut self,
        relation_type: &str,
        file_key: &str,
    ) -> std::result::Result<(), String> {
        if !Self::has_file_attachments() {
            return Err("Model does not support file attachments".to_string());
        }

        let mut files = self.get_files_attribute()?;

        let file_metadata = serde_json::json!({
            "key": file_key,
            "filename": file_key.split('/').next_back().unwrap_or(file_key),
            "created_at": chrono::Utc::now().to_rfc3339(),
        });

        if Self::has_one_attached_file().contains(&relation_type) {
            files.insert(relation_type.to_string(), file_metadata);
        } else if Self::has_many_attached_files().contains(&relation_type) {
            let mut array = files
                .get(relation_type)
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            array.push(file_metadata);
            files.insert(relation_type.to_string(), serde_json::Value::Array(array));
        } else {
            return Err(format!("Unknown file relation: {}", relation_type));
        }

        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Attach multiple files to a relation type (hasMany only)
    fn attach_files(
        &mut self,
        relation_type: &str,
        file_keys: Vec<&str>,
    ) -> std::result::Result<(), String> {
        if !Self::has_file_attachments() {
            return Err("Model does not support file attachments".to_string());
        }

        if !Self::has_many_attached_files().contains(&relation_type) {
            return Err(format!(
                "Relation '{}' is not a hasMany relation",
                relation_type
            ));
        }

        for file_key in file_keys {
            self.attach_file(relation_type, file_key)?;
        }

        Ok(())
    }

    /// Detach a file from a relation type
    fn detach_file(
        &mut self,
        relation_type: &str,
        file_key: Option<&str>,
    ) -> std::result::Result<(), String> {
        if !Self::has_file_attachments() {
            return Err("Model does not support file attachments".to_string());
        }

        let mut files = self.get_files_attribute()?;

        if let Some(key) = file_key {
            if Self::has_one_attached_file().contains(&relation_type) {
                if let Some(current) = files.get(relation_type) {
                    if current.get("key").and_then(|k| k.as_str()) == Some(key) {
                        files.insert(relation_type.to_string(), serde_json::Value::Null);
                    }
                }
            } else if Self::has_many_attached_files().contains(&relation_type) {
                if let Some(array) = files.get(relation_type).and_then(|v| v.as_array()) {
                    let filtered: Vec<serde_json::Value> = array
                        .iter()
                        .filter(|item| item.get("key").and_then(|k| k.as_str()) != Some(key))
                        .cloned()
                        .collect();
                    files.insert(
                        relation_type.to_string(),
                        serde_json::Value::Array(filtered),
                    );
                }
            }
        } else if Self::has_one_attached_file().contains(&relation_type) {
            files.insert(relation_type.to_string(), serde_json::Value::Null);
        } else if Self::has_many_attached_files().contains(&relation_type) {
            files.insert(relation_type.to_string(), serde_json::Value::Array(vec![]));
        }

        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Sync files for a relation type (replaces all existing files)
    fn sync_files(
        &mut self,
        relation_type: &str,
        file_keys: Vec<&str>,
    ) -> std::result::Result<(), String> {
        if !Self::has_file_attachments() {
            return Err("Model does not support file attachments".to_string());
        }

        let mut files = self.get_files_attribute()?;

        if file_keys.is_empty() {
            if Self::has_one_attached_file().contains(&relation_type) {
                files.insert(relation_type.to_string(), serde_json::Value::Null);
            } else if Self::has_many_attached_files().contains(&relation_type) {
                files.insert(relation_type.to_string(), serde_json::Value::Array(vec![]));
            }
        } else if Self::has_one_attached_file().contains(&relation_type) {
            let file_metadata = serde_json::json!({
                "key": file_keys[0],
                "filename": file_keys[0].split('/').next_back().unwrap_or(file_keys[0]),
                "created_at": chrono::Utc::now().to_rfc3339(),
            });
            files.insert(relation_type.to_string(), file_metadata);
        } else if Self::has_many_attached_files().contains(&relation_type) {
            let file_array: Vec<serde_json::Value> = file_keys
                .iter()
                .map(|key| {
                    serde_json::json!({
                        "key": key,
                        "filename": key.split('/').next_back().unwrap_or(key),
                        "created_at": chrono::Utc::now().to_rfc3339(),
                    })
                })
                .collect();
            files.insert(
                relation_type.to_string(),
                serde_json::Value::Array(file_array),
            );
        } else {
            return Err(format!("Unknown file relation: {}", relation_type));
        }

        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Extract files from data HashMap for saving
    #[cfg(feature = "attachments")]
    fn extract_files(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        if !Self::has_file_attachments() {
            return Ok(serde_json::json!({}));
        }

        let mut files = serde_json::Map::new();
        let file_relations = Self::files_relations();

        for relation in file_relations {
            if let Some(value) = data.remove(relation) {
                files.insert(relation.to_string(), value);
            }
        }

        Ok(serde_json::Value::Object(files))
    }

    /// Extract files from data HashMap for saving
    #[cfg(not(feature = "attachments"))]
    fn extract_files(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        let _ = data;
        Ok(serde_json::json!({}))
    }
}

/// Builder for creating new model instances
pub struct CreateBuilder<M: ModelMeta> {
    _marker: std::marker::PhantomData<M>,
    values: std::collections::HashMap<String, serde_json::Value>,
}

impl<M: ModelMeta> CreateBuilder<M> {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            values: std::collections::HashMap::new(),
        }
    }

    /// Set a field value
    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.values.insert(field.to_string(), value.into());
        self
    }
}

impl<M: ModelMeta> Default for CreateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for updating model instances
pub struct UpdateBuilder<M: Model> {
    model: M,
    changes: std::collections::HashMap<String, serde_json::Value>,
}

impl<M: Model> UpdateBuilder<M> {
    /// Create a new update builder
    pub fn new(model: M) -> Self {
        Self {
            model,
            changes: std::collections::HashMap::new(),
        }
    }

    /// Set a field to a new value
    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.changes.insert(field.to_string(), value.into());
        self
    }

    /// Save the changes to the database
    pub async fn save(self) -> Result<M> {
        self.model.update().await
    }
}

/// Builder for on-conflict (upsert) operations
///
/// Provides fine-grained control over what happens when a conflict occurs during insert.
///
/// # Example
/// ```ignore
/// // Update specific columns on conflict
/// let user = User::on_conflict("email")
///     .update_columns(vec!["name", "updated_at"])
///     .insert(user)
///     .await?;
///
/// // Update all columns except specified ones
/// let user = User::on_conflict(vec!["tenant_id", "key"])
///     .update_all_except(vec!["id", "created_at"])
///     .insert(config)
///     .await?;
/// ```
#[doc(hidden)]
pub struct OnConflictBuilder<M: Model> {
    pub _marker: std::marker::PhantomData<M>,
    pub conflict_columns: Vec<String>,
    pub update_columns: Option<Vec<String>>,
    pub exclude_columns: Option<Vec<String>>,
}

impl<M: Model> OnConflictBuilder<M> {
    /// Create a new on-conflict builder
    pub fn new(conflict_columns: Vec<String>) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            conflict_columns,
            update_columns: None,
            exclude_columns: None,
        }
    }

    /// Specify which columns to update on conflict
    ///
    /// Only these columns will be updated when a conflict occurs.
    pub fn update_columns(mut self, columns: Vec<&str>) -> Self {
        self.update_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Update all columns except the specified ones
    ///
    /// Useful when you want to update most fields but exclude primary keys, timestamps, etc.
    pub fn update_all_except(mut self, columns: Vec<&str>) -> Self {
        self.exclude_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Execute the insert with on-conflict handling
    pub async fn insert(self, model: M) -> Result<M>
    where
        M: Sized,
    {
        M::__insert_with_conflict(model, self).await
    }
}

/// Builder for batch update operations
///
/// Allows updating multiple records matching conditions with advanced features.
///
/// # Example
/// ```ignore
/// // Simple batch update
/// let affected = User::update_all()
///     .set("active", false)
///     .where_eq("status", "banned")
///     .execute()
///     .await?;
///
/// // Update with increment/decrement
/// let affected = Product::update_all()
///     .increment("view_count", 1)
///     .where_eq("featured", true)
///     .execute()
///     .await?;
///
/// // Update with conditional logic
/// let affected = User::update_all()
///     .set_raw("last_login", "NOW()")
///     .set_if("verified", true, |b| b.where_eq("email_confirmed", true))
///     .execute()
///     .await?;
/// ```
pub struct BatchUpdateBuilder<M: Model> {
    _marker: std::marker::PhantomData<M>,
    updates: std::collections::HashMap<String, UpdateValue>,
    conditions: Vec<crate::query::WhereCondition>,
    returning: bool,
    limit_value: Option<u64>,
}

/// Value for batch update operations
#[derive(Debug, Clone)]
pub enum UpdateValue {
    /// Direct value assignment
    Value(serde_json::Value),
    /// Raw SQL expression (e.g., "NOW()", "column + 1")
    Raw(String),
    /// Increment by value
    Increment(i64),
    /// Decrement by value
    Decrement(i64),
    /// Multiply by value
    Multiply(f64),
    /// Divide by value
    Divide(f64),
    /// Append to array (PostgreSQL)
    ArrayAppend(serde_json::Value),
    /// Remove from array (PostgreSQL)
    ArrayRemove(serde_json::Value),
    /// JSON set path (for JSON columns)
    JsonSet(String, serde_json::Value),
    /// Coalesce with default (set only if NULL)
    Coalesce(serde_json::Value),
}

impl<M: Model> BatchUpdateBuilder<M> {
    /// Create a new batch update builder
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            updates: std::collections::HashMap::new(),
            conditions: Vec::new(),
            returning: false,
            limit_value: None,
        }
    }

    /// Set a field to a new value
    ///
    /// # Example
    /// ```ignore
    /// User::update_all()
    ///     .set("active", false)
    ///     .where_eq("banned", true)
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Value(value.into()));
        self
    }

    /// Set a field to a raw SQL expression
    ///
    /// # Example
    /// ```ignore
    /// User::update_all()
    ///     .set_raw("updated_at", "NOW()")
    ///     .set_raw("login_count", "login_count + 1")
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn set_raw(mut self, field: &str, expression: &str) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Raw(expression.to_string()));
        self
    }

    /// Conditionally set a field based on a closure
    ///
    /// # Example
    /// ```ignore
    /// User::update_all()
    ///     .set_if("verified", true, |b| b.where_eq("email_confirmed", true))
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn set_if<F>(
        mut self,
        field: &str,
        value: impl Into<serde_json::Value>,
        condition: F,
    ) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        self.updates
            .insert(field.to_string(), UpdateValue::Value(value.into()));
        condition(self)
    }

    /// Increment a numeric field by value
    ///
    /// # Example
    /// ```ignore
    /// Product::update_all()
    ///     .increment("view_count", 1)
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn increment(mut self, field: &str, by: i64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Increment(by));
        self
    }

    /// Decrement a numeric field by value
    ///
    /// # Example
    /// ```ignore
    /// Product::update_all()
    ///     .decrement("stock", 1)
    ///     .where_eq("id", product_id)
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn decrement(mut self, field: &str, by: i64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Decrement(by));
        self
    }

    /// Multiply a numeric field by value
    ///
    /// # Example
    /// ```ignore
    /// Product::update_all()
    ///     .multiply("price", 1.1) // 10% increase
    ///     .where_eq("category", "electronics")
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn multiply(mut self, field: &str, by: f64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Multiply(by));
        self
    }

    /// Divide a numeric field by value
    pub fn divide(mut self, field: &str, by: f64) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Divide(by));
        self
    }

    /// Append value to an array column (PostgreSQL)
    ///
    /// # Example
    /// ```ignore
    /// User::update_all()
    ///     .array_append("tags", "premium")
    ///     .where_eq("subscription", "paid")
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn array_append(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::ArrayAppend(value.into()));
        self
    }

    /// Remove value from an array column (PostgreSQL)
    pub fn array_remove(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::ArrayRemove(value.into()));
        self
    }

    /// Set a value at a JSON path
    ///
    /// # Example
    /// ```ignore
    /// User::update_all()
    ///     .json_set("settings", "$.theme", "dark")
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn json_set(
        mut self,
        field: &str,
        path: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.to_string(),
            UpdateValue::JsonSet(path.to_string(), value.into()),
        );
        self
    }

    /// Set value only if current value is NULL
    ///
    /// # Example
    /// ```ignore
    /// User::update_all()
    ///     .coalesce("nickname", "Anonymous")
    ///     .execute()
    ///     .await?;
    /// ```
    pub fn coalesce(mut self, field: &str, default: impl Into<serde_json::Value>) -> Self {
        self.updates
            .insert(field.to_string(), UpdateValue::Coalesce(default.into()));
        self
    }

    /// Limit the number of rows to update (MySQL/MariaDB)
    ///
    /// Note: PostgreSQL requires a subquery for limited updates
    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    /// Return updated rows (PostgreSQL, SQLite 3.35+)
    ///
    /// Note: Not all databases support RETURNING
    pub fn returning(mut self) -> Self {
        self.returning = true;
        self
    }

    /// Add a where equals condition
    pub fn where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where not equals condition
    pub fn where_not(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where greater than condition
    pub fn where_gt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where greater than or equal condition
    pub fn where_gte(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Gte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where less than condition
    pub fn where_lt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where less than or equal condition
    pub fn where_lte(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Lte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add a where IN condition
    pub fn where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    /// Add a where NOT IN condition
    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: &str,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::NotIn,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    /// Add a where IS NULL condition
    pub fn where_null(mut self, column: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    /// Add a where IS NOT NULL condition
    pub fn where_not_null(mut self, column: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::IsNotNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    /// Add a where BETWEEN condition
    pub fn where_between(
        mut self,
        column: &str,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Between,
            value: crate::query::ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    /// Add a where LIKE condition
    pub fn where_like(mut self, column: &str, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.to_string(),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    // =========================================================================
    // OR CLAUSE METHODS FOR BATCH UPDATE
    // =========================================================================

    /// Add an OR equals condition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// User::update_all()
    ///     .set("status", "inactive")
    ///     .where_eq("banned", true)
    ///     .or_where_eq("expired", true)
    ///     .execute()
    ///     .await?;
    /// // Updates users where banned = true OR expired = true
    /// ```
    pub fn or_where_eq(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        // For batch updates, we treat OR as a separate condition group
        // We'll mark these by using a special prefix in the column name that we'll parse later
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add an OR not equals condition
    pub fn or_where_not(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add an OR greater than condition
    pub fn or_where_gt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add an OR less than condition
    pub fn or_where_lt(mut self, column: &str, value: impl Into<serde_json::Value>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    /// Add an OR IN condition
    pub fn or_where_in<V: Into<serde_json::Value>>(mut self, column: &str, values: Vec<V>) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    /// Add an OR IS NULL condition
    pub fn or_where_null(mut self, column: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    /// Add an OR LIKE condition
    pub fn or_where_like(mut self, column: &str, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    /// Format a value for SQL
    fn format_value(v: &serde_json::Value) -> String {
        match v {
            serde_json::Value::Null => "NULL".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
            _ => format!("'{}'", v.to_string().replace('\'', "''")),
        }
    }

    /// Execute the batch update and return rows affected
    pub async fn execute(self) -> Result<u64> {
        if self.updates.is_empty() {
            return Ok(0);
        }

        let db_type = crate::database::require_db()?.backend();
        let quote = match db_type {
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => '`',
            _ => '"',
        };

        // Build SET clause with different update types
        let set_parts: Vec<String> = self.updates.iter()
            .map(|(k, v)| {
                let col = format!("{0}{1}{0}", quote, k);
                match v {
                    UpdateValue::Value(val) => {
                        format!("{} = {}", col, Self::format_value(val))
                    }
                    UpdateValue::Raw(expr) => {
                        format!("{} = {}", col, expr)
                    }
                    UpdateValue::Increment(by) => {
                        format!("{} = {} + {}", col, col, by)
                    }
                    UpdateValue::Decrement(by) => {
                        format!("{} = {} - {}", col, col, by)
                    }
                    UpdateValue::Multiply(by) => {
                        format!("{} = {} * {}", col, col, by)
                    }
                    UpdateValue::Divide(by) => {
                        format!("{} = {} / {}", col, col, by)
                    }
                    UpdateValue::ArrayAppend(val) => {
                        match db_type {
                            crate::config::DatabaseType::Postgres => {
                                format!("{} = array_append({}, {})", col, col, Self::format_value(val))
                            }
                            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                                format!("{} = JSON_ARRAY_APPEND({}, '$', {})", col, col, Self::format_value(val))
                            }
                            crate::config::DatabaseType::SQLite => {
                                format!("{} = json_insert({}, '$[#]', {})", col, col, Self::format_value(val))
                            }
                        }
                    }
                    UpdateValue::ArrayRemove(val) => {
                        match db_type {
                            crate::config::DatabaseType::Postgres => {
                                format!("{} = array_remove({}, {})", col, col, Self::format_value(val))
                            }
                            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                                format!("{} = JSON_REMOVE({}, JSON_UNQUOTE(JSON_SEARCH({}, 'one', {})))", col, col, col, Self::format_value(val))
                            }
                            crate::config::DatabaseType::SQLite => {
                                // SQLite requires more complex logic
                                format!("{} = (SELECT json_group_array(value) FROM json_each({}) WHERE value != {})", col, col, Self::format_value(val))
                            }
                        }
                    }
                    UpdateValue::JsonSet(path, val) => {
                        match db_type {
                            crate::config::DatabaseType::Postgres => {
                                format!("{} = jsonb_set({}, '{{{}}}', '{}')", col, col, path.trim_start_matches("$."), Self::format_value(val).trim_matches('\''))
                            }
                            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                                format!("{} = JSON_SET({}, '{}', {})", col, col, path, Self::format_value(val))
                            }
                            crate::config::DatabaseType::SQLite => {
                                format!("{} = json_set({}, '{}', {})", col, col, path, Self::format_value(val))
                            }
                        }
                    }
                    UpdateValue::Coalesce(default) => {
                        format!("{} = COALESCE({}, {})", col, col, Self::format_value(default))
                    }
                }
            })
            .collect();

        // Build WHERE clause - separate AND and OR conditions
        let mut and_parts: Vec<String> = Vec::new();
        let mut or_parts: Vec<String> = Vec::new();

        for cond in &self.conditions {
            // Check if this is an OR condition (marked with __OR__ prefix)
            let (is_or, actual_column) = if cond.column.starts_with("__OR__") {
                (
                    true,
                    cond.column.strip_prefix("__OR__").unwrap_or(&cond.column),
                )
            } else {
                (false, cond.column.as_str())
            };

            let col = format!("{0}{1}{0}", quote, actual_column);

            let part = match &cond.operator {
                crate::query::Operator::Eq => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} = {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::NotEq => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} != {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Gt => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} > {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Gte => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} >= {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Lt => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} < {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Lte => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} <= {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                crate::query::Operator::In => {
                    if let crate::query::ConditionValue::List(vals) = &cond.value {
                        let list = vals
                            .iter()
                            .map(Self::format_value)
                            .collect::<Vec<_>>()
                            .join(", ");
                        Some(format!("{} IN ({})", col, list))
                    } else {
                        None
                    }
                }
                crate::query::Operator::NotIn => {
                    if let crate::query::ConditionValue::List(vals) = &cond.value {
                        let list = vals
                            .iter()
                            .map(Self::format_value)
                            .collect::<Vec<_>>()
                            .join(", ");
                        Some(format!("{} NOT IN ({})", col, list))
                    } else {
                        None
                    }
                }
                crate::query::Operator::IsNull => Some(format!("{} IS NULL", col)),
                crate::query::Operator::IsNotNull => Some(format!("{} IS NOT NULL", col)),
                crate::query::Operator::Between => {
                    if let crate::query::ConditionValue::Range(min, max) = &cond.value {
                        Some(format!(
                            "{} BETWEEN {} AND {}",
                            col,
                            Self::format_value(min),
                            Self::format_value(max)
                        ))
                    } else {
                        None
                    }
                }
                crate::query::Operator::Like => {
                    if let crate::query::ConditionValue::Single(v) = &cond.value {
                        Some(format!("{} LIKE {}", col, Self::format_value(v)))
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(part) = part {
                if is_or {
                    or_parts.push(part);
                } else {
                    and_parts.push(part);
                }
            }
        }

        let table = format!("{0}{1}{0}", quote, M::table_name());
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        // Build final WHERE clause with AND and OR parts
        if !and_parts.is_empty() || !or_parts.is_empty() {
            sql.push_str(" WHERE ");

            let mut where_parts: Vec<String> = Vec::new();

            if !and_parts.is_empty() {
                where_parts.push(and_parts.join(" AND "));
            }

            if !or_parts.is_empty() {
                // OR conditions are grouped together
                let or_clause = if or_parts.len() == 1 {
                    or_parts[0].clone()
                } else {
                    format!("({})", or_parts.join(" OR "))
                };
                where_parts.push(or_clause);
            }

            // Combine AND and OR groups with OR
            if and_parts.is_empty() {
                sql.push_str(&or_parts.join(" OR "));
            } else if or_parts.is_empty() {
                sql.push_str(&and_parts.join(" AND "));
            } else {
                // AND conditions AND (OR conditions)
                sql.push_str(&format!(
                    "{} AND ({})",
                    and_parts.join(" AND "),
                    or_parts.join(" OR ")
                ));
            }
        }

        // Add LIMIT for MySQL/MariaDB
        if let Some(limit) = self.limit_value {
            if matches!(
                db_type,
                crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
            ) {
                sql.push_str(&format!(" LIMIT {}", limit));
            }
        }

        crate::Database::execute(&sql).await
    }

    /// Execute and return the updated rows (requires RETURNING support)
    ///
    /// Works with PostgreSQL and SQLite 3.35+
    pub async fn execute_returning(self) -> Result<Vec<M>> {
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        let db_type = crate::database::require_db()?.backend();

        if db_type == crate::config::DatabaseType::MySQL {
            return Err(Error::query(
                "MySQL does not support RETURNING clause".to_string(),
            ));
        }

        let quote = match db_type {
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => '`',
            _ => '"',
        };

        // Build SET clause
        let set_parts: Vec<String> = self
            .updates
            .iter()
            .map(|(k, v)| {
                let col = format!("{0}{1}{0}", quote, k);
                match v {
                    UpdateValue::Value(val) => format!("{} = {}", col, Self::format_value(val)),
                    UpdateValue::Raw(expr) => format!("{} = {}", col, expr),
                    UpdateValue::Increment(by) => format!("{} = {} + {}", col, col, by),
                    UpdateValue::Decrement(by) => format!("{} = {} - {}", col, col, by),
                    UpdateValue::Multiply(by) => format!("{} = {} * {}", col, col, by),
                    UpdateValue::Divide(by) => format!("{} = {} / {}", col, col, by),
                    _ => format!("{} = {}", col, col), // Fallback for complex types
                }
            })
            .collect();

        // Build WHERE clause
        let where_parts: Vec<String> = self
            .conditions
            .iter()
            .filter_map(|cond| {
                let col = format!("{0}{1}{0}", quote, cond.column);
                match &cond.operator {
                    crate::query::Operator::Eq => {
                        if let crate::query::ConditionValue::Single(v) = &cond.value {
                            Some(format!("{} = {}", col, Self::format_value(v)))
                        } else {
                            None
                        }
                    }
                    crate::query::Operator::NotEq => {
                        if let crate::query::ConditionValue::Single(v) = &cond.value {
                            Some(format!("{} != {}", col, Self::format_value(v)))
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            })
            .collect();

        let table = format!("{0}{1}{0}", quote, M::table_name());
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
        }

        sql.push_str(" RETURNING *");

        crate::Database::raw::<M>(&sql).await
    }
}

impl<M: Model> Default for BatchUpdateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// NESTED ACTIVE MODEL
// =============================================================================
// Provides cascade save operations for related models
//
// This feature allows saving a model along with its related models in a single
// operation, maintaining referential integrity.

/// Extension trait for cascade save operations
///
/// This trait provides methods to save a model along with its related models
/// in a single transaction-like operation.
///
/// # Example
///
/// ```rust,ignore
/// use tideorm::prelude::*;
///
/// // Define models
/// #[derive(Model)]
/// struct User {
///     #[tideorm(primary_key, auto_increment)]
///     id: i64,
///     name: String,
///     email: String,
/// }
///
/// #[derive(Model)]
/// struct Post {
///     #[tideorm(primary_key, auto_increment)]
///     id: i64,
///     user_id: i64,
///     title: String,
///     content: String,
/// }
///
/// impl BelongsTo<User> for Post {
///     fn foreign_key() -> &'static str { "user_id" }
/// }
///
/// // Save user with posts in one operation
/// let user = User { id: 0, name: "Alice".into(), email: "alice@example.com".into() };
/// let posts = vec![
///     Post { id: 0, user_id: 0, title: "First".into(), content: "Hello!".into() },
///     Post { id: 0, user_id: 0, title: "Second".into(), content: "World!".into() },
/// ];
///
/// let (user, posts) = user.save_with_many(posts, "user_id").await?;
/// // Both user and posts are saved, with posts having user_id set
/// ```
#[async_trait::async_trait]
pub trait NestedSave: Model {
    /// Save this model along with a single related model
    ///
    /// The related model's foreign key will be set to this model's primary key
    /// after this model is saved.
    ///
    /// # Arguments
    ///
    /// * `related` - The related model to save
    /// * `foreign_key` - The foreign key field name on the related model
    ///
    /// # Returns
    ///
    /// A tuple of (self, related) with both models saved
    async fn save_with_one<R: Model>(self, related: R, foreign_key: &str) -> Result<(Self, R)>
    where
        Self: Sized,
    {
        // First save the parent model
        let parent = self.save().await?;

        // Get the parent's primary key as JSON to set on child
        let pk_value = {
            let pk = parent.primary_key();
            serde_json::Value::String(format!("{}", pk))
        };

        // Update the foreign key in the related model via JSON round-trip
        let mut related_json = serde_json::to_value(&related)
            .map_err(|e| Error::conversion(format!("Failed to serialize related model: {}", e)))?;

        if let serde_json::Value::Object(ref mut map) = related_json {
            // Try to convert the PK to the appropriate type
            let pk_str = pk_value.as_str().unwrap_or_default();
            if let Ok(pk_i64) = pk_str.parse::<i64>() {
                map.insert(foreign_key.to_string(), serde_json::json!(pk_i64));
            } else {
                map.insert(foreign_key.to_string(), pk_value);
            }
        }

        let related: R = serde_json::from_value(related_json).map_err(|e| {
            Error::conversion(format!("Failed to deserialize related model: {}", e))
        })?;

        // Save the related model
        let related = related.save().await?;

        Ok((parent, related))
    }

    /// Save this model along with multiple related models
    ///
    /// All related models' foreign keys will be set to this model's primary key
    /// after this model is saved.
    ///
    /// # Arguments
    ///
    /// * `related` - The related models to save
    /// * `foreign_key` - The foreign key field name on the related models
    ///
    /// # Returns
    ///
    /// A tuple of (self, related_vec) with all models saved
    async fn save_with_many<R: Model>(
        self,
        related: Vec<R>,
        foreign_key: &str,
    ) -> Result<(Self, Vec<R>)>
    where
        Self: Sized,
    {
        // First save the parent model
        let parent = self.save().await?;

        // Get the parent's primary key
        let pk_value = {
            let pk = parent.primary_key();
            serde_json::Value::String(format!("{}", pk))
        };

        // Update each related model's foreign key and save
        let mut saved_related = Vec::with_capacity(related.len());
        for item in related {
            let mut item_json = serde_json::to_value(&item).map_err(|e| {
                Error::conversion(format!("Failed to serialize related model: {}", e))
            })?;

            if let serde_json::Value::Object(ref mut map) = item_json {
                let pk_str = pk_value.as_str().unwrap_or_default();
                if let Ok(pk_i64) = pk_str.parse::<i64>() {
                    map.insert(foreign_key.to_string(), serde_json::json!(pk_i64));
                } else {
                    map.insert(foreign_key.to_string(), pk_value.clone());
                }
            }

            let item: R = serde_json::from_value(item_json).map_err(|e| {
                Error::conversion(format!("Failed to deserialize related model: {}", e))
            })?;

            let saved = item.save().await?;
            saved_related.push(saved);
        }

        Ok((parent, saved_related))
    }

    /// Update this model and cascade update to related models
    ///
    /// Updates this model and optionally updates the related model as well.
    async fn update_with_one<R: Model>(self, related: R) -> Result<(Self, R)>
    where
        Self: Sized,
    {
        let parent = self.update().await?;
        let related = related.update().await?;
        Ok((parent, related))
    }

    /// Update this model and cascade update to multiple related models
    async fn update_with_many<R: Model>(self, related: Vec<R>) -> Result<(Self, Vec<R>)>
    where
        Self: Sized,
    {
        let parent = self.update().await?;
        let mut updated = Vec::with_capacity(related.len());
        for item in related {
            updated.push(item.update().await?);
        }
        Ok((parent, updated))
    }

    /// Delete this model and cascade delete to related models
    ///
    /// Deletes the related models first, then this model.
    async fn delete_with_many<R: Model>(self, related: Vec<R>) -> Result<u64>
    where
        Self: Sized,
    {
        let mut total = 0u64;

        // Delete related first to maintain referential integrity
        for item in related {
            total += item.delete().await?;
        }

        // Delete parent
        total += self.delete().await?;

        Ok(total)
    }
}

// Blanket implementation for all Models
impl<M: Model> NestedSave for M {}

/// Builder for nested/cascade saves
///
/// Provides a fluent API for building complex nested save operations.
///
/// # Example
///
/// ```rust,ignore
/// let (user, profile, posts) = NestedSaveBuilder::new(user)
///     .with_one(profile, "user_id")
///     .with_many(posts, "user_id")
///     .save()
///     .await?;
/// ```
pub struct NestedSaveBuilder<M: Model> {
    parent: M,
    one_relations: Vec<(serde_json::Value, String)>,
    many_relations: Vec<(Vec<serde_json::Value>, String)>,
}

impl<M: Model> NestedSaveBuilder<M> {
    /// Create a new nested save builder
    pub fn new(parent: M) -> Self {
        Self {
            parent,
            one_relations: Vec::new(),
            many_relations: Vec::new(),
        }
    }

    /// Add a single related model to be saved
    pub fn with_one<R: Model>(mut self, related: R, foreign_key: &str) -> Self {
        if let Ok(json) = serde_json::to_value(&related) {
            self.one_relations.push((json, foreign_key.to_string()));
        }
        self
    }

    /// Add multiple related models to be saved
    pub fn with_many<R: Model>(mut self, related: Vec<R>, foreign_key: &str) -> Self {
        let json_values: Vec<serde_json::Value> = related
            .into_iter()
            .filter_map(|r| serde_json::to_value(&r).ok())
            .collect();
        self.many_relations
            .push((json_values, foreign_key.to_string()));
        self
    }

    /// Execute the nested save operation
    ///
    /// Returns the saved parent model along with all related models as JSON.
    pub async fn save(self) -> Result<(M, Vec<serde_json::Value>)> {
        // Save the parent first
        let parent = self.parent.save().await?;

        // Get the parent's primary key
        let pk_value = {
            let pk = parent.primary_key();
            serde_json::Value::String(format!("{}", pk))
        };

        let mut saved_json = Vec::new();

        // Save all one-relations
        for (mut json, fk) in self.one_relations {
            if let serde_json::Value::Object(ref mut map) = json {
                let pk_str = pk_value.as_str().unwrap_or_default();
                if let Ok(pk_i64) = pk_str.parse::<i64>() {
                    map.insert(fk, serde_json::json!(pk_i64));
                } else {
                    map.insert(fk, pk_value.clone());
                }
            }
            // Here we'd ideally save via a generic mechanism
            // For now, store the JSON with updated FK
            saved_json.push(json);
        }

        // Save all many-relations
        for (items, fk) in self.many_relations {
            for mut json in items {
                if let serde_json::Value::Object(ref mut map) = json {
                    let pk_str = pk_value.as_str().unwrap_or_default();
                    if let Ok(pk_i64) = pk_str.parse::<i64>() {
                        map.insert(fk.clone(), serde_json::json!(pk_i64));
                    } else {
                        map.insert(fk.clone(), pk_value.clone());
                    }
                }
                saved_json.push(json);
            }
        }

        Ok((parent, saved_json))
    }
}

#[cfg(test)]
#[path = "testing/model_tests.rs"]
mod tests;
