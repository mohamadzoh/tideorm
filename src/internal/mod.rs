//! Internal ORM adapter layer
//!
//!
//! This module serves as the adapter between TideORM's public API and the
//! current ORM engine.
//!
//! 2. We can swap the underlying ORM engine if needed
//! 3. Error translation happens in one place
//! 4. Query translation is centralized

use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident_for_backend;
use crate::soft_delete::{SoftDeleteScope, query_scope_for};

mod backend;

pub(crate) mod sql_safety;

// Re-export the current ORM engine internally through TideORM's facade.
// Allow unused_imports here: we re-export broadly so other modules can import selectively.
#[allow(unused_imports)]
pub use crate::orm::{
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, Condition,
    ConnectOptions, ConnectionTrait, Database as OrmDatabase, DatabaseConnection as OrmConnection,
    DatabaseTransaction as OrmTransaction, DbBackend as OrmBackend, DbErr as OrmError, DeleteMany,
    DeriveEntityModel, DeriveRelation, EntityTrait, EnumIter, ExecResult, FromQueryResult, Iden,
    IntoActiveModel, Iterable, LoaderTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Related, RelationDef, RelationTrait, Statement as OrmStatement,
    TransactionTrait, TryGetable, Value,
    entity::prelude::*,
    schema::{Schema, SchemaBuilder},
    sea_query::{
        Alias, Asterisk, ColumnDef as OrmColumnDef, ColumnType as OrmColumnType, Expr, ExprTrait,
        Index, MysqlQueryBuilder, OnConflict, PostgresQueryBuilder, Query, SimpleExpr,
        SqliteQueryBuilder, Table, extension::postgres::PgBinOper,
    },
    sqlx,
};

pub use backend::Backend;
pub(crate) use backend::{build_statement, build_statement_with_values};

pub(crate) fn json_to_db_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::String(None),
        serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Value::BigInt(Some(integer))
            } else if let Some(float) = number.as_f64() {
                Value::Double(Some(float))
            } else {
                Value::String(Some(number.to_string()))
            }
        }
        serde_json::Value::String(text) => Value::String(Some(text.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Value::String(Some(value.to_string()))
        }
    }
}

pub(crate) fn push_param(
    db_type: crate::config::DatabaseType,
    params: &mut Vec<Value>,
    value: Value,
) -> String {
    let placeholder = match db_type {
        crate::config::DatabaseType::Postgres => format!("${}", params.len() + 1),
        crate::config::DatabaseType::MySQL
        | crate::config::DatabaseType::MariaDB
        | crate::config::DatabaseType::SQLite => "?".to_string(),
    };
    params.push(value);
    placeholder
}

/// Internal trait that maps TideORM models to ORM engine entities.
/// This is implemented by TideORM's model macros.
#[doc(hidden)]
pub trait InternalModel: crate::model::ModelMeta + Sized + Send + Sync + Clone {
    type Entity: EntityTrait;
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity> + ActiveModelBehavior + Send;

    /// Convert a TideORM model to the ORM engine's active model.
    fn into_active_model(self) -> Self::ActiveModel;

    /// Convert a TideORM model to the ORM engine's active model and allow
    /// model-level preprocessing such as encrypted field writes.
    fn try_into_active_model(self) -> Result<Self::ActiveModel> {
        Ok(self.into_active_model())
    }

    /// Convert the generated entity model to a TideORM model.
    fn from_entity_model(model: <Self::Entity as EntityTrait>::Model) -> Self;

    /// Convert the generated entity model to a TideORM model and allow
    /// model-level postprocessing such as encrypted field reads.
    fn try_from_entity_model(model: <Self::Entity as EntityTrait>::Model) -> Result<Self> {
        Ok(Self::from_entity_model(model))
    }

    /// Convert a TideORM model into its generated entity model.
    fn to_entity_model(&self) -> <Self::Entity as EntityTrait>::Model;

    /// Convert a TideORM model into its generated entity model and allow
    /// model-level preprocessing such as encrypted field writes.
    fn try_to_entity_model(&self) -> Result<<Self::Entity as EntityTrait>::Model> {
        Ok(self.to_entity_model())
    }

    /// Resolve an entity column enum from either a field name or column name.
    fn column_from_str(name: &str) -> Option<<Self::Entity as EntityTrait>::Column>;

    /// Get entity primary key columns.
    fn primary_key_columns() -> Vec<<Self::Entity as EntityTrait>::Column> {
        Vec::new()
    }

    /// Get the ORM condition for an exact primary key match.
    fn primary_key_condition(
        primary_key: &<Self as crate::model::ModelMeta>::PrimaryKey,
    ) -> Condition;

    /// Get the primary key column (optional, for single-column operations).
    fn primary_key_column() -> Option<<Self::Entity as EntityTrait>::Column> {
        Self::primary_key_columns().into_iter().next()
    }

    /// Rebuild runtime-only relation wrappers after an in-memory model overwrite.
    fn refresh_runtime_relations_from(&mut self, _previous: &Self) {}

    /// Get one model field as JSON without serializing the full model.
    fn field_json_value(&self, _field: &str) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }
}

/// Internal connection wrapper
#[doc(hidden)]
pub struct InternalConnection {
    pub(crate) conn: OrmConnection,
}

impl InternalConnection {
    pub async fn connect(url: &str) -> Result<Self> {
        let conn = OrmDatabase::connect(url)
            .await
            .map_err(|e| Error::connection(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &OrmConnection {
        &self.conn
    }
}

/// Translate ORM engine errors to TideORM errors.
pub(crate) fn translate_error(err: OrmError) -> Error {
    match err {
        OrmError::RecordNotFound(msg) => Error::not_found(msg),
        OrmError::ConnectionAcquire(e) => Error::connection(e.to_string()),
        OrmError::Conn(e) => Error::connection(e.to_string()),
        OrmError::Exec(e) => Error::query(e.to_string()),
        OrmError::Query(e) => Error::query(e.to_string()),
        OrmError::ConvertFromU64(msg) => Error::conversion(msg),
        OrmError::UnpackInsertId => Error::query("Failed to get insert ID".to_string()),
        OrmError::UpdateGetPrimaryKey => {
            Error::query("Failed to get primary key after update".to_string())
        }
        OrmError::Custom(msg) => Error::internal(msg),
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
    backend: Backend,
) -> bool {
    if let Some(db_type) = configured_db_type {
        return match db_type {
            crate::config::DatabaseType::Postgres => matches!(backend, Backend::Postgres),
            crate::config::DatabaseType::MariaDB => matches!(backend, Backend::MySql),
            crate::config::DatabaseType::MySQL | crate::config::DatabaseType::SQLite => false,
        };
    }

    matches!(backend, Backend::Postgres)
}

pub(crate) fn count_to_u64(count: i64, context: &str) -> Result<u64> {
    u64::try_from(count).map_err(|_| {
        Error::query(format!(
            "Database returned a negative count ({count}) for {context}"
        ))
    })
}

fn build_count_select<M>(condition: Option<Condition>) -> Select<M::Entity>
where
    M: InternalModel + crate::model::Model,
{
    let mut select = scoped_find::<M>()
        .select_only()
        .column_as(Expr::col(Asterisk).count(), "count");

    if let Some(condition) = condition {
        select = select.filter(condition);
    }

    select
}

fn build_exists_any_statement<M>(backend: Backend) -> OrmStatement
where
    M: InternalModel + crate::model::Model,
{
    let table = quote_ident_for_backend(backend, M::table_name());
    let mut sql = format!("SELECT EXISTS(SELECT 1 FROM {}", table);

    if matches!(
        query_scope_for::<M>(false, false),
        SoftDeleteScope::ActiveOnly
    ) {
        let deleted_at = quote_ident_for_backend(backend, M::deleted_at_column());
        sql.push_str(&format!(" WHERE {}.{} IS NULL", table, deleted_at));
    }

    sql.push(')');

    build_statement(backend, sql)
}

fn scoped_find<M>() -> Select<M::Entity>
where
    M: InternalModel + crate::model::Model,
{
    let mut select = M::Entity::find();

    if matches!(
        query_scope_for::<M>(false, false),
        SoftDeleteScope::ActiveOnly
    ) && let Some(deleted_at_column) = M::column_from_str(M::deleted_at_column())
    {
        select = select.filter(deleted_at_column.is_null());
    }

    select
}

/// Internal query executor
#[doc(hidden)]
pub struct QueryExecutor;

impl QueryExecutor {
    /// Find all records
    pub async fn find_all<M, C>(conn: &C) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        let results = scoped_find::<M>().all(conn);
        let results = crate::profiling::__profile_future(results)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("find_all()")))?;

        results.into_iter().map(M::try_from_entity_model).collect()
    }

    /// Get first record
    pub async fn first<M, C>(conn: &C) -> Result<Option<M>>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        let result = scoped_find::<M>().one(conn);
        let result = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("first()")))?;

        result.map(M::try_from_entity_model).transpose()
    }

    /// Get last record (by primary key descending)
    pub async fn last<M, C>(conn: &C) -> Result<Option<M>>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        // Order by primary key descending to get the actual last record
        let mut select = scoped_find::<M>();
        let mut query_label = String::from("last()");

        // Use the primary key column if available, otherwise fall back to unordered
        let pk_columns = M::primary_key_columns();
        if !pk_columns.is_empty() {
            for pk_col in pk_columns {
                select = select.order_by_desc(pk_col);
            }
            query_label = format!("last(order_by={} desc)", M::primary_key_names().join(", "));
        }

        let result = select.one(conn);
        let result = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>(query_label)))?;

        result.map(M::try_from_entity_model).transpose()
    }

    /// Count records
    pub async fn count<M, C>(conn: &C, condition: Option<Condition>) -> Result<u64>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count: i64,
        }

        let result = build_count_select::<M>(condition)
            .into_model::<CountResult>()
            .one(conn);
        let result: Option<CountResult> = crate::profiling::__profile_future(result)
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("count(*)")))?;

        result
            .map(|r| count_to_u64(r.count, "count(*)"))
            .transpose()
            .map(|count| count.unwrap_or(0))
    }

    /// Check whether any records exist.
    pub async fn exists_any<M, C>(conn: &C) -> Result<bool>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        let backend = Backend::from(conn.get_database_backend());
        let statement = build_exists_any_statement::<M>(backend);
        let result = crate::profiling::__profile_future(conn.query_one_raw(statement))
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(model_error_context::<M>("exists_any()")))?;

        match result {
            Some(row) => {
                let exists = match backend {
                    Backend::Postgres => row.try_get_by_index(0).unwrap_or(false),
                    _ => {
                        let value: i32 = row.try_get_by_index(0).unwrap_or(0);
                        value > 0
                    }
                };
                Ok(exists)
            }
            None => Ok(false),
        }
    }

    /// Paginate records
    pub async fn paginate<M, C>(conn: &C, limit: u64, offset: u64) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        let results = scoped_find::<M>().offset(offset).limit(limit).all(conn);
        let results = crate::profiling::__profile_future(results)
            .await
            .map_err(translate_error)
            .map_err(|err| {
                err.with_context(model_error_context::<M>(format!(
                    "paginate(limit={}, offset={})",
                    limit, offset
                )))
            })?;

        results.into_iter().map(M::try_from_entity_model).collect()
    }

    /// Delete a record
    pub async fn delete<M, C>(conn: &C, model: M) -> Result<u64>
    where
        M: InternalModel + crate::model::Model,
        C: ConnectionTrait,
    {
        let active = model.into_active_model();
        let result = active.delete(conn);
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
    pub async fn insert_many<M, C>(conn: &C, models: Vec<M>) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
        <<M as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<M::ActiveModel>,
        C: ConnectionTrait,
    {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = models.len();
        let error_context =
            model_error_context::<M>(format!("insert_many(batch_size={})", batch_size));

        // For single model, use regular insert for simplicity
        if models.len() == 1 {
            let active = models.into_iter().next().unwrap().try_into_active_model()?;
            let result =
                crate::profiling::__profile_future(async move { active.insert(conn).await })
                    .await
                    .map_err(translate_error)
                    .map_err(|err| err.with_context(error_context.clone()))?;
            return Ok(vec![M::try_from_entity_model(result)?]);
        }

        // Check if we can use exec_with_returning (Postgres, MariaDB 10.5+).
        // The current ORM engine exposes both MySQL and MariaDB as OrmBackend::MySql, so prefer
        // TideORM's configured database type when it is available.
        let backend = Backend::from(conn.get_database_backend());
        let supports_returning = supports_batch_insert_returning(
            crate::config::TideConfig::get_database_type(),
            backend,
        );

        if supports_returning {
            // Build batch insert using the ORM engine's insert_many with RETURNING
            let active_models: Vec<_> = models
                .into_iter()
                .map(M::try_into_active_model)
                .collect::<Result<Vec<_>>>()?;

            let results = M::Entity::insert_many(active_models).exec_with_returning(conn);
            let results = crate::profiling::__profile_future(results)
                .await
                .map_err(translate_error)
                .map_err(|err| err.with_context(error_context.clone()))?;

            results.into_iter().map(M::try_from_entity_model).collect()
        } else {
            // MySQL/SQLite: fall back to individual inserts
            // MySQL doesn't support multi-row INSERT ... RETURNING
            let mut results = Vec::with_capacity(models.len());
            for model in models {
                let active = model.try_into_active_model()?;
                let result =
                    crate::profiling::__profile_future(async move { active.insert(conn).await })
                        .await
                        .map_err(translate_error)
                        .map_err(|err| err.with_context(error_context.clone()))?;
                results.push(M::try_from_entity_model(result)?);
            }
            Ok(results)
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/internal_tests.rs"]
mod tests;
