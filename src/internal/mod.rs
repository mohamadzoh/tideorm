//! Internal SeaORM adapter layer
//!
//!
//! This module serves as the adapter between TideORM's public API and SeaORM's internals.
//! All SeaORM interactions happen through this layer, ensuring:
//!
//! 2. We can swap the underlying ORM engine if needed
//! 3. Error translation happens in one place
//! 4. Query translation is centralized

use crate::error::{Error, Result};

// Re-export SeaORM internally (but this module itself is #[doc(hidden)])
// Allow unused_imports here: we re-export broadly so other modules can import selectively
#[allow(unused_imports)]
pub use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, Condition,
    ConnectOptions, ConnectionTrait, Database as SeaDatabase, DatabaseConnection,
    DatabaseTransaction, DbBackend, DbErr, DeleteMany, DeriveEntityModel, DeriveRelation,
    EntityTrait, EnumIter, ExecResult, FromQueryResult, Iden, IntoActiveModel, Iterable,
    LoaderTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    Related, RelationDef, RelationTrait, Statement, TransactionTrait, TryGetable, Value,
    entity::prelude::*,
    sea_query::{Alias, Asterisk, Expr, ExprTrait},
};

/// Internal trait that maps TideORM models to SeaORM entities
/// This is implemented by the `#[derive(Model)]` macro
#[doc(hidden)]
pub trait InternalModel: Sized + Send + Sync + Clone {
    type Entity: EntityTrait;
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity> + ActiveModelBehavior + Send;

    /// Convert TideORM model to SeaORM ActiveModel
    fn into_active_model(self) -> Self::ActiveModel;

    /// Convert SeaORM Model to TideORM model
    fn from_sea_model(model: <Self::Entity as EntityTrait>::Model) -> Self;

    /// Convert a TideORM model into its generated SeaORM model.
    fn to_sea_model(&self) -> <Self::Entity as EntityTrait>::Model;

    /// Resolve a SeaORM column enum from either a field name or column name.
    fn column_from_str(name: &str) -> Option<<Self::Entity as EntityTrait>::Column>;

    /// Get SeaORM primary key column (optional, for find_by_id)
    fn primary_key_column() -> Option<<Self::Entity as EntityTrait>::Column> {
        None
    }
}

/// Internal connection wrapper
#[doc(hidden)]
pub struct InternalConnection {
    pub(crate) conn: DatabaseConnection,
}

impl InternalConnection {
    pub async fn connect(url: &str) -> Result<Self> {
        let conn = SeaDatabase::connect(url)
            .await
            .map_err(|e| Error::connection(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.conn
    }
}

/// Translate SeaORM DbErr to TideORM Error
pub(crate) fn translate_error(err: DbErr) -> Error {
    match err {
        DbErr::RecordNotFound(msg) => Error::not_found(msg),
        DbErr::ConnectionAcquire(e) => Error::connection(e.to_string()),
        DbErr::Conn(e) => Error::connection(e.to_string()),
        DbErr::Exec(e) => Error::query(e.to_string()),
        DbErr::Query(e) => Error::query(e.to_string()),
        DbErr::ConvertFromU64(msg) => Error::conversion(msg),
        DbErr::UnpackInsertId => Error::query("Failed to get insert ID".to_string()),
        DbErr::UpdateGetPrimaryKey => {
            Error::query("Failed to get primary key after update".to_string())
        }
        DbErr::Custom(msg) => Error::internal(msg),
        _ => Error::internal(err.to_string()),
    }
}

fn model_error_context<M>(query: impl Into<String>) -> crate::error::ErrorContext
where
    M: crate::model::Model,
{
    crate::error::ErrorContext::new()
        .table(M::table_name())
        .query(query.into())
}

fn supports_batch_insert_returning(
    configured_db_type: Option<crate::config::DatabaseType>,
    backend: DbBackend,
) -> bool {
    if let Some(db_type) = configured_db_type {
        return match db_type {
            crate::config::DatabaseType::Postgres => matches!(backend, DbBackend::Postgres),
            crate::config::DatabaseType::MariaDB => matches!(backend, DbBackend::MySql),
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::SQLite => false,
        };
    }

    matches!(backend, DbBackend::Postgres)
}

/// Internal query executor
#[doc(hidden)]
pub struct QueryExecutor;

impl QueryExecutor {
    /// Find all records
    pub async fn find_all<M>(conn: &DatabaseConnection) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
    {
        let results = M::Entity::find()
            .all(conn);
        let results = crate::profiling::__profile_future(results)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("find_all()")))?;

        Ok(results.into_iter().map(M::from_sea_model).collect())
    }

    /// Get first record
    pub async fn first<M>(conn: &DatabaseConnection) -> Result<Option<M>>
    where
        M: InternalModel + crate::model::Model,
    {
        let result = M::Entity::find()
            .one(conn);
        let result = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("first()")))?;

        Ok(result.map(M::from_sea_model))
    }

    /// Get last record (by primary key descending)
    pub async fn last<M>(conn: &DatabaseConnection) -> Result<Option<M>>
    where
        M: InternalModel + crate::model::Model,
    {
        // Order by primary key descending to get the actual last record
        let mut select = M::Entity::find();
        let mut query_label = String::from("last()");

        // Use the primary key column if available, otherwise fall back to unordered
        if let Some(pk_col) = M::primary_key_column() {
            select = select.order_by_desc(pk_col);
            query_label = format!("last(order_by={} desc)", M::primary_key_name());
        }

        let result = select
            .one(conn);
        let result = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>(query_label)))?;

        Ok(result.map(M::from_sea_model))
    }

    /// Count records
    pub async fn count<M>(conn: &DatabaseConnection, _condition: Option<Condition>) -> Result<u64>
    where
        M: InternalModel + crate::model::Model,
    {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count: i64,
        }

        let result = M::Entity::find()
            .select_only()
            .column_as(Expr::col(Asterisk).count(), "count")
            .into_model::<CountResult>()
            .one(conn);
        let result: Option<CountResult> = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("count(*)")))?;

        Ok(result.map(|r| r.count as u64).unwrap_or(0))
    }

    /// Paginate records
    pub async fn paginate<M>(conn: &DatabaseConnection, limit: u64, offset: u64) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
    {
        let results = M::Entity::find()
            .offset(offset)
            .limit(limit)
            .all(conn);
        let results = crate::profiling::__profile_future(results)
            .await
            .map_err(translate_error)
            .map_err(|err| {
                err.with_context(model_error_context::<M>(format!(
                    "paginate(limit={}, offset={})",
                    limit, offset
                )))
            })?;

        Ok(results.into_iter().map(M::from_sea_model).collect())
    }

    /// Delete a record
    pub async fn delete<M>(conn: &DatabaseConnection, model: M) -> Result<u64>
    where
        M: InternalModel + crate::model::Model,
    {
        let active = model.into_active_model();
        let result = active
            .delete(conn);
        let result = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("delete(model)")))?;
        Ok(result.rows_affected)
    }

    /// Insert multiple records in a single batch INSERT statement
    ///
    /// This constructs a multi-row INSERT instead of individual inserts,
    /// reducing the number of database round trips from O(n) to O(1).
    ///
    /// On PostgreSQL and MariaDB, uses `INSERT ... RETURNING` for efficiency.
    /// On MySQL and SQLite, falls back to individual inserts since they don't
    /// support multi-row `INSERT ... RETURNING`.
    pub async fn insert_many<M>(conn: &DatabaseConnection, models: Vec<M>) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
        <<M as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<M::ActiveModel>,
    {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = models.len();
        let error_context =
            model_error_context::<M>(format!("insert_many(batch_size={})", batch_size));

        // For single model, use regular insert for simplicity
        if models.len() == 1 {
            let active = models.into_iter().next().unwrap().into_active_model();
            let result = crate::profiling::__profile_future(async move { active.insert(conn).await })
                .await
                .map_err(translate_error)
                .map_err(|err| err.with_context(error_context.clone()))?;
            return Ok(vec![M::from_sea_model(result)]);
        }

        // Check if we can use exec_with_returning (Postgres, MariaDB 10.5+).
        // SeaORM exposes both MySQL and MariaDB as DbBackend::MySql, so prefer
        // TideORM's configured database type when it is available.
        let backend = conn.get_database_backend();
        let supports_returning = supports_batch_insert_returning(
            crate::config::TideConfig::get_database_type(),
            backend,
        );

        if supports_returning {
            // Build batch insert using SeaORM's insert_many with RETURNING
            let active_models: Vec<_> = models.into_iter().map(|m| m.into_active_model()).collect();

            let results = M::Entity::insert_many(active_models)
                .exec_with_returning(conn);
            let results = crate::profiling::__profile_future(results)
                .await
                .map_err(translate_error)
                .map_err(|err| err.with_context(error_context.clone()))?;

            Ok(results.into_iter().map(M::from_sea_model).collect())
        } else {
            // MySQL/SQLite: fall back to individual inserts
            // MySQL doesn't support multi-row INSERT ... RETURNING
            let mut results = Vec::with_capacity(models.len());
            for model in models {
                let active = model.into_active_model();
                let result = crate::profiling::__profile_future(async move { active.insert(conn).await })
                    .await
                    .map_err(translate_error)
                    .map_err(|err| err.with_context(error_context.clone()))?;
                results.push(M::from_sea_model(result));
            }
            Ok(results)
        }
    }
}

#[cfg(test)]
#[path = "../testing/internal_tests.rs"]
mod tests;
