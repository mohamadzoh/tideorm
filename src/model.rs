//! Model trait and utilities.

mod batch;
mod builders;
mod crud;
mod meta;
mod nested;
mod serialization;

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::query::QueryBuilder;

pub use batch::{BatchUpdateBuilder, UpdateValue};
pub use builders::{CreateBuilder, OnConflictBuilder, UpdateBuilder};
pub use meta::{IndexDefinition, ModelMeta};
pub use nested::{NestedSave, NestedSaveBuilder};

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
        crud::db()
    }

    /// Get the global database connection from an instance
    ///
    /// # Example
    /// ```ignore
    /// let user = User::find(1).await?;
    /// let db = user.database()?;
    /// ```
    fn database(&self) -> crate::error::Result<&'static crate::database::Database> {
        crud::database()
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
        crud::all::<Self>().await
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

    /// Start a query builder for this model using an explicit database connection.
    ///
    /// This bypasses the global singleton and is useful for multi-tenant setups,
    /// parallel tests, and dependency-injected database handles.
    fn query_with(db: &crate::database::Database) -> QueryBuilder<Self>
    where
        Self: Sized,
    {
        QueryBuilder::new().with_database(db.clone())
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
        crud::count::<Self>().await
    }

    /// Delete every record for this model.
    ///
    /// This is an explicit full-table operation. Filtered deletions should keep
    /// using `Self::query().where_*(...).delete()` so accidental unfiltered bulk
    /// deletes remain blocked by default.
    async fn delete_all() -> Result<u64>
    where
        Self: Sized,
    {
        Self::query().delete_all().await
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
        crud::exists_any::<Self>().await
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
        crud::insert_all::<Self>(models).await
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
        crud::insert_many_returning::<Self>(models).await
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
        crud::insert_many::<Self>(models).await
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
        crud::transaction::<Self, F, T>(f).await
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
        crud::first::<Self>().await
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
        crud::last::<Self>().await
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
        crud::paginate::<Self>(page, per_page).await
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

    /// Find record by primary key using an explicit database connection.
    async fn find_with(
        id: Self::PrimaryKey,
        db: &crate::database::Database,
    ) -> Result<Option<Self>>
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
        crud::reload(self).await
    }

    /// Check if this record is new (not yet saved)
    ///
    /// For auto-increment primary keys, TideORM treats `0` as unsaved so
    /// freshly constructed records like `{ id: 0, .. }` are handled correctly.
    /// For natural keys, an empty primary key is considered new.
    fn is_new(&self) -> bool {
        crud::is_new(self)
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
        serialization::to_json::<Self>(self, options)
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
        serialization::process_file_for_json(field_name, file_data, hidden_attrs, url_generator)
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
        serialization::collection_to_json::<Self>(models, options)
    }

    /// Convert model to HashMap
    fn to_hash_map(&self) -> HashMap<String, String>
    where
        Self: serde::Serialize,
    {
        serialization::to_hash_map::<Self>(self)
    }

    // =========================================================================
    // TRANSLATION METHODS (for models with 'translations' JSONB column)
    // =========================================================================

    /// Load translations for a specific language into model fields
    fn load_language_translations(&mut self, _language: &str) -> std::result::Result<(), String> {
        serialization::load_language_translations::<Self>(_language)
    }

    /// Load all translations for all languages into model fields
    fn load_all_translations(&mut self) -> std::result::Result<(), String> {
        serialization::load_all_translations::<Self>()
    }

    /// Extract translations from data HashMap for saving
    #[cfg(feature = "translations")]
    fn extract_translations(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_translations::<Self>(data)
    }

    /// Extract translations from data HashMap for saving
    #[cfg(not(feature = "translations"))]
    fn extract_translations(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_translations::<Self>(data)
    }

    // =========================================================================
    // FILE ATTACHMENT METHODS (for models with 'files' JSONB column)
    // =========================================================================

    /// Get files from the JSONB 'files' column
    fn get_files_attribute(
        &self,
    ) -> std::result::Result<HashMap<String, serde_json::Value>, String> {
        serialization::get_files_attribute::<Self>()
    }

    /// Set files to the JSONB 'files' column
    fn set_files_attribute(
        &mut self,
        files: HashMap<String, serde_json::Value>,
    ) -> std::result::Result<(), String> {
        serialization::set_files_attribute::<Self>(files)
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
        let mut files = self.get_files_attribute()?;
        serialization::attach_file::<Self>(relation_type, file_key, &mut files)?;
        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Attach multiple files to a relation type (hasMany only)
    fn attach_files(
        &mut self,
        relation_type: &str,
        file_keys: Vec<&str>,
    ) -> std::result::Result<(), String> {
        let mut files = self.get_files_attribute()?;
        serialization::attach_files::<Self>(relation_type, file_keys, &mut files)?;
        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Detach a file from a relation type
    fn detach_file(
        &mut self,
        relation_type: &str,
        file_key: Option<&str>,
    ) -> std::result::Result<(), String> {
        let mut files = self.get_files_attribute()?;
        serialization::detach_file::<Self>(relation_type, file_key, &mut files)?;
        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Sync files for a relation type (replaces all existing files)
    fn sync_files(
        &mut self,
        relation_type: &str,
        file_keys: Vec<&str>,
    ) -> std::result::Result<(), String> {
        let mut files = self.get_files_attribute()?;
        serialization::sync_files::<Self>(relation_type, file_keys, &mut files)?;
        self.set_files_attribute(files)?;
        Ok(())
    }

    /// Extract files from data HashMap for saving
    #[cfg(feature = "attachments")]
    fn extract_files(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_files::<Self>(data)
    }

    /// Extract files from data HashMap for saving
    #[cfg(not(feature = "attachments"))]
    fn extract_files(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_files::<Self>(data)
    }
}

#[cfg(test)]
#[path = "testing/model_tests.rs"]
mod tests;
