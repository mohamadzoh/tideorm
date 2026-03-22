use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::query::QueryBuilder;

use super::{BatchUpdateBuilder, OnConflictBuilder, crud, serialization};

/// Core trait for TideORM models
///
/// This trait provides all the CRUD operations and query capabilities.
/// It is automatically implemented by TideORM's model macros.
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
    super::ModelMeta
    + crate::internal::InternalModel
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
{
    /// Get the primary key value of this instance
    fn primary_key(&self) -> Self::PrimaryKey;

    /// Get the global database connection
    fn db() -> crate::error::Result<crate::database::Database> {
        crud::db()
    }

    /// Get the global database connection from an instance
    fn database(&self) -> crate::error::Result<crate::database::Database> {
        crud::database()
    }

    async fn all() -> Result<Vec<Self>>
    where
        Self: Sized,
    {
        crud::all::<Self>().await
    }

    fn query() -> QueryBuilder<Self>
    where
        Self: Sized,
    {
        QueryBuilder::new()
    }

    fn query_with(db: &crate::database::Database) -> QueryBuilder<Self>
    where
        Self: Sized,
    {
        QueryBuilder::new().with_database(db.clone())
    }

    async fn count() -> Result<u64>
    where
        Self: Sized,
    {
        crud::count::<Self>().await
    }

    async fn delete_all() -> Result<u64>
    where
        Self: Sized,
    {
        Self::query().delete_all().await
    }

    async fn exists_any() -> Result<bool>
    where
        Self: Sized,
    {
        crud::exists_any::<Self>().await
    }

    async fn insert_all(models: Vec<Self>) -> Result<Vec<Self>>
    where
        Self: Sized,
        <<Self as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            crate::internal::IntoActiveModel<<Self as crate::internal::InternalModel>::ActiveModel>,
    {
        crud::insert_all::<Self>(models).await
    }

    async fn insert_many_returning(models: Vec<Self>) -> Result<Vec<Self>>
    where
        Self: Sized,
        <<Self as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            crate::internal::IntoActiveModel<<Self as crate::internal::InternalModel>::ActiveModel>,
    {
        crud::insert_many_returning::<Self>(models).await
    }

    async fn insert_many(models: Vec<Self>) -> Result<Vec<Self>>
    where
        Self: Sized,
        <<Self as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            crate::internal::IntoActiveModel<<Self as crate::internal::InternalModel>::ActiveModel>,
    {
        crud::insert_many::<Self>(models).await
    }

    async fn insert_or_update(model: Self, conflict_columns: Vec<&str>) -> Result<Self>
    where
        Self: Sized;

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

    fn update_all() -> BatchUpdateBuilder<Self>
    where
        Self: Sized,
    {
        BatchUpdateBuilder::new()
    }

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
        crud::transaction::<F, T>(f).await
    }

    async fn first() -> Result<Option<Self>>
    where
        Self: Sized,
    {
        crud::first::<Self>().await
    }

    async fn last() -> Result<Option<Self>>
    where
        Self: Sized,
    {
        crud::last::<Self>().await
    }

    async fn paginate(page: u64, per_page: u64) -> Result<Vec<Self>>
    where
        Self: Sized,
    {
        crud::paginate::<Self>(page, per_page).await
    }

    async fn find(id: Self::PrimaryKey) -> Result<Option<Self>>
    where
        Self: Sized;

    async fn find_with(
        id: Self::PrimaryKey,
        db: &crate::database::Database,
    ) -> Result<Option<Self>>
    where
        Self: Sized;

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

    async fn exists(id: Self::PrimaryKey) -> Result<bool>
    where
        Self: Sized,
    {
        Ok(Self::find(id).await?.is_some())
    }

    async fn create(model: Self) -> Result<Self>
    where
        Self: Sized;

    async fn destroy(id: Self::PrimaryKey) -> Result<u64>
    where
        Self: Sized;

    async fn save(self) -> Result<Self>
    where
        Self: Sized;

    async fn update(self) -> Result<Self>
    where
        Self: Sized;

    async fn delete(self) -> Result<u64>
    where
        Self: Sized;

    #[doc(hidden)]
    async fn __insert_with_conflict(model: Self, builder: OnConflictBuilder<Self>) -> Result<Self>
    where
        Self: Sized;

    async fn reload(&self) -> Result<Self>
    where
        Self: Sized,
    {
        crud::reload(self).await
    }

    fn is_new(&self) -> bool {
        crud::is_new(self)
    }

    fn to_json(&self, options: Option<HashMap<String, String>>) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        serialization::to_json::<Self>(self, options)
    }

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

    fn collection_to_json(
        models: Vec<Self>,
        options: Option<HashMap<String, String>>,
    ) -> serde_json::Value
    where
        Self: serde::Serialize,
    {
        serialization::collection_to_json::<Self>(models, options)
    }

    fn to_hash_map(&self) -> HashMap<String, String>
    where
        Self: serde::Serialize,
    {
        serialization::to_hash_map::<Self>(self)
    }

    fn load_language_translations(&mut self, _language: &str) -> std::result::Result<(), String> {
        serialization::load_language_translations(self, _language)
    }

    #[cfg(feature = "translations")]
    fn extract_translations(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_translations::<Self>(data)
    }

    #[cfg(not(feature = "translations"))]
    fn extract_translations(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_translations::<Self>(data)
    }

    fn get_files_attribute(
        &self,
    ) -> std::result::Result<HashMap<String, serde_json::Value>, String> {
        serialization::get_files_attribute(self)
    }

    fn set_files_attribute(
        &mut self,
        files: HashMap<String, serde_json::Value>,
    ) -> std::result::Result<(), String> {
        serialization::set_files_attribute(self, files)
    }

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

    #[cfg(feature = "attachments")]
    fn extract_files(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_files::<Self>(data)
    }

    #[cfg(not(feature = "attachments"))]
    fn extract_files(
        data: &mut HashMap<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, String> {
        serialization::extract_files::<Self>(data)
    }
}
