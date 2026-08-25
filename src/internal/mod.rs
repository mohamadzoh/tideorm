//! Internal ORM adapter layer
//!
//!
//! This module serves as the adapter between TideORM's public API and the
//! current ORM engine.
//!
//! 2. We can swap the underlying ORM engine if needed
//! 3. Error translation happens in one place
//! 4. Query translation is centralized

use crate::error::{DbFailure, DbFailureKind, Error, Result};
use crate::internal::sql_safety::quote_ident_for_backend;
// `ConnAcquireErr` and `RuntimeErr` arrive through the `entity::prelude::*` re-export
// below; importing them again privately would shadow that public glob.
use crate::soft_delete::{SoftDeleteScope, query_scope_for};

mod backend;
#[cfg(feature = "fulltext")]
pub(crate) mod sql_builder;
pub(crate) mod sql_safety;

// Re-export the current ORM engine internally through TideORM's facade.
// Allow unused_imports here: we re-export broadly so other modules can import selectively.
pub use crate::orm::{
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, Condition,
    ConnectOptions, ConnectionTrait, Database as OrmDatabase, DatabaseConnection as OrmConnection,
    DatabaseTransaction as OrmTransaction, DbBackend as OrmBackend, DbErr as OrmError, DeleteMany,
    DeriveEntityModel, DeriveRelation, EntityTrait, EnumIter, ExecResult, FromQueryResult, Iden,
    IntoActiveModel, Iterable, LoaderTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Related, RelationDef, RelationTrait, Statement as OrmStatement,
    TransactionSession, TransactionTrait, TryGetable, Value,
    entity::prelude::*,
    schema::{Schema, SchemaBuilder},
    sea_query::{
        Alias, Asterisk, ColumnDef as OrmColumnDef, ColumnType as OrmColumnType, Expr, ExprTrait,
        Index, MysqlQueryBuilder, OnConflict, PostgresQueryBuilder, Query, SimpleExpr,
        SqliteQueryBuilder, Table, extension::postgres::PgBinOper,
    },
};

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub use crate::orm::sqlx;

pub use backend::Backend;
pub(crate) use backend::{build_statement, build_statement_with_values};

/// A single database parameter value.
///
/// This is the type the public raw-SQL entry points bind — see
/// [`Database::raw_with_params`](crate::database::Database::raw_with_params),
/// [`Database::execute_with_params`](crate::database::Database::execute_with_params)
/// and
/// [`Database::raw_json_with_params`](crate::database::Database::raw_json_with_params).
///
/// It exists so callers have a TideORM-owned name for the parameter type and
/// never have to reach into this hidden module (or name the ORM engine's type)
/// just to call a documented API — reach it as [`tideorm::DbValue`](crate::DbValue)
/// or through the prelude, not through `tideorm::internal`. Most values are
/// built with `.into()` from the corresponding Rust type, so the name is only
/// needed for annotations and explicit `NULL` bindings.
pub type DbValue = Value;

/// Bind one JSON value as a database parameter.
///
/// `Null` becomes a typed NULL binding, which is only ever correct where SQL
/// itself expects a NULL operand — never on either side of `=` or `!=`, where
/// the comparison is UNKNOWN for every row and the filter silently matches
/// nothing. `QueryBuilder::condition_spec` rewrites those into `IS NULL` /
/// `IS NOT NULL` before reaching this function, and ordering comparisons
/// against NULL are rejected outright, so no comparison arrives here with a
/// null to bind.
///
/// Arrays and objects are bound as their JSON text. The JSON operators do not
/// come through here — `query::db_sql` binds those with backend-appropriate
/// JSON parameters — so this stays the generic scalar path.
pub(crate) fn json_to_db_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::String(None),
        serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Value::BigInt(Some(integer))
            } else if let Some(unsigned) = number.as_u64() {
                Value::BigUnsigned(Some(unsigned))
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

/// Refuse an encryption-blind fallback before it can move plaintext or
/// ciphertext across the persistence boundary.
///
/// `InternalModel`'s fallible conversions default to their infallible twins,
/// which is correct for the overwhelming majority of models: they declare no
/// `#[tideorm(encrypted)]` fields, so the two paths are the same conversion and
/// the default saves every model from spelling it twice. A model that *does*
/// declare encrypted fields must override the fallible half — the derive always
/// does — because the infallible half has nowhere to report a cipher failure and
/// would either write plaintext into a ciphertext column or hand ciphertext back
/// as if it were plaintext. If that override is ever missing, this turns a silent
/// data-integrity bug into an error at the point of conversion.
pub(crate) fn reject_encryption_blind_conversion<M>(conversion: &str) -> Result<()>
where
    M: crate::model::ModelMeta,
{
    if !M::has_encrypted_fields() {
        return Ok(());
    }

    Err(Error::internal(format!(
        "`{}` fell back to the plaintext conversion for `{}`, which cannot handle the encrypted \
         field(s): {}",
        conversion,
        M::table_name(),
        M::encrypted_fields().join(", ")
    )))
}

/// Internal trait that maps TideORM models to ORM engine entities.
/// This is implemented by TideORM's model macros.
///
/// The conversions come in pairs. The `try_` half is the persistence path: it is
/// where encrypted fields are encrypted on the way out and decrypted on the way
/// in, so it is the only half allowed to reach the database. The infallible half
/// is a plaintext, in-memory convenience that cannot report a cipher failure —
/// never build a statement from it. The `try_` defaults bridge to it only for
/// models with no encrypted fields; see `reject_encryption_blind_conversion`.
#[doc(hidden)]
pub trait InternalModel: crate::model::ModelMeta + Sized + Send + Sync + Clone {
    type Entity: EntityTrait;
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity> + ActiveModelBehavior + Send;

    /// Convert a TideORM model to the ORM engine's active model, in plaintext.
    fn into_active_model(self) -> Self::ActiveModel;

    /// Convert a TideORM model to the ORM engine's active model and allow
    /// model-level preprocessing such as encrypted field writes.
    fn try_into_active_model(self) -> Result<Self::ActiveModel> {
        reject_encryption_blind_conversion::<Self>("try_into_active_model")?;
        Ok(self.into_active_model())
    }

    /// Convert the generated entity model to a TideORM model, in plaintext.
    fn from_entity_model(model: <Self::Entity as EntityTrait>::Model) -> Self;

    /// Convert the generated entity model to a TideORM model and allow
    /// model-level postprocessing such as encrypted field reads.
    fn try_from_entity_model(model: <Self::Entity as EntityTrait>::Model) -> Result<Self> {
        reject_encryption_blind_conversion::<Self>("try_from_entity_model")?;
        Ok(Self::from_entity_model(model))
    }

    /// Convert a TideORM model into its generated entity model, in plaintext.
    ///
    /// The plaintext rendering is what in-memory comparisons want — comparing
    /// two ciphertexts of a randomized cipher reports a change on every field —
    /// so this stays the comparison path even for encrypted models.
    fn to_entity_model(&self) -> <Self::Entity as EntityTrait>::Model;

    /// Convert a TideORM model into its generated entity model and allow
    /// model-level preprocessing such as encrypted field writes.
    fn try_to_entity_model(&self) -> Result<<Self::Entity as EntityTrait>::Model> {
        reject_encryption_blind_conversion::<Self>("try_to_entity_model")?;
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

/// Read the SQLSTATE, constraint name and table a driver reported.
///
/// The engine's own error kind answers the constraint violations on every
/// backend; everything else is classified from the SQLSTATE, which is where
/// syntax, privilege and lock failures become distinguishable.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn runtime_failure(runtime: &RuntimeErr) -> DbFailure {
    use crate::orm::sqlx::error::ErrorKind;

    let RuntimeErr::SqlxError(driver_error) = runtime else {
        return DbFailure::new(DbFailureKind::Unclassified);
    };
    let Some(database_error) = driver_error.as_database_error() else {
        return DbFailure::new(DbFailureKind::Unclassified);
    };

    let code = database_error.code().map(|code| code.into_owned());
    let kind = match database_error.kind() {
        ErrorKind::UniqueViolation => DbFailureKind::UniqueViolation,
        ErrorKind::ForeignKeyViolation => DbFailureKind::ForeignKeyViolation,
        ErrorKind::NotNullViolation => DbFailureKind::NotNullViolation,
        ErrorKind::CheckViolation => DbFailureKind::CheckViolation,
        _ => code
            .as_deref()
            .map_or(DbFailureKind::Unclassified, DbFailureKind::from_sqlstate),
    };

    DbFailure::new(kind)
        .with_code(code)
        .with_constraint(database_error.constraint().map(str::to_string))
        .with_table(database_error.table().map(str::to_string))
}

/// Without a driver feature compiled in there is no driver error to read, so
/// every runtime failure stays unclassified.
#[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
fn runtime_failure(_runtime: &RuntimeErr) -> DbFailure {
    DbFailure::new(DbFailureKind::Unclassified)
}

/// Recover the structured driver detail behind an engine error.
///
/// Only the engine variants that actually came back from a driver produce a
/// failure — the rest is engine bookkeeping with nothing to preserve. The engine
/// error itself is kept as the failure's own source, which is what makes
/// `{:#}`-style chains and `anyhow` interop reach the backend instead of
/// stopping at TideORM's rendered message.
pub(crate) fn driver_failure(err: &OrmError) -> Option<DbFailure> {
    let failure = match err {
        // A pool acquire failure never reaches a driver, so it has no SQLSTATE,
        // but the engine already classified it and both cases are transient.
        OrmError::ConnectionAcquire(ConnAcquireErr::Timeout) => {
            DbFailure::new(DbFailureKind::ConnectionTimeout)
        }
        OrmError::ConnectionAcquire(ConnAcquireErr::ConnectionClosed) => {
            DbFailure::new(DbFailureKind::ConnectionClosed)
        }
        OrmError::Conn(runtime) | OrmError::Exec(runtime) | OrmError::Query(runtime) => {
            runtime_failure(runtime)
        }
        _ => return None,
    };

    Some(failure.with_source(Box::new(err.clone())))
}

/// Translate ORM engine errors to TideORM errors.
///
/// The structured driver detail rides along, so the returned error can answer
/// for its SQLSTATE and constraint name and its `source` chain still reaches the
/// driver.
pub(crate) fn translate_error(err: OrmError) -> Error {
    let failure = driver_failure(&err);
    translate_engine_error(err).with_db_failure(failure)
}

/// Map an engine error onto the TideORM variant that describes it.
fn translate_engine_error(err: OrmError) -> Error {
    match err {
        OrmError::RecordNotFound(msg) => Error::not_found(msg),
        OrmError::ConnectionAcquire(e) => Error::connection(e.to_string()),
        OrmError::Conn(e) => Error::connection(e.to_string()),
        OrmError::Exec(e) => Error::query(e.to_string()),
        OrmError::Query(e) => Error::query(e.to_string()),
        OrmError::ConvertFromU64(msg) => Error::conversion(msg),
        OrmError::TryIntoErr { from, into, source } => {
            Error::conversion(format!("Error converting `{from}` into `{into}`: {source}"))
        }
        OrmError::Type(msg) => Error::conversion(msg),
        OrmError::Json(msg) => Error::conversion(msg),
        // Raised by `TryFrom<ActiveModel>` when an attribute was never set, which
        // is a per-field completeness failure and carries the field name — the
        // exact shape of `Error::Validation`.
        OrmError::AttrNotSet(attribute) => Error::validation(attribute, "attribute is not set"),
        OrmError::KeyArityMismatch { expected, received } => Error::query(format!(
            "Primary key arity mismatch: expected {expected} key column(s), received {received}"
        )),
        OrmError::UnpackInsertId => Error::query("Failed to get insert ID".to_string()),
        OrmError::UpdateGetPrimaryKey => {
            Error::query("Failed to get primary key after update".to_string())
        }
        OrmError::BackendNotSupported { db, ctx } => Error::backend_not_supported(ctx, db),
        OrmError::PrimaryKeyNotSet { ctx } => {
            Error::primary_key_not_set(format!("primary key not set for {ctx}"), "model")
        }
        OrmError::RecordNotInserted => Error::query("None of the records are inserted".to_string()),
        OrmError::RecordNotUpdated => Error::not_found("None of the records are updated"),
        // A migration failure is a statement that did not execute, which is what
        // `Error::Query` describes — and it is what TideORM's own migrator
        // (`migration::migrator`) already reports for the same failures, so
        // engine-raised migration errors must not land somewhere else.
        OrmError::Migration(msg) => Error::query(msg),
        OrmError::AccessDenied {
            permission,
            resource,
        } => Error::access_denied(permission, resource),
        OrmError::RbacError(msg) => Error::rbac(msg),
        OrmError::Custom(msg) => Error::internal(msg),
        // Everything left over — a poisoned engine mutex, plus whatever a future
        // engine release adds — has no TideORM counterpart and stays internal on
        // purpose. This arm is deliberately a catch-all so an engine upgrade
        // cannot break the build.
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

fn query_result_exists_bool(row: &QueryResult) -> Result<bool> {
    if let Ok(value) = row.try_get_by_index::<bool>(0) {
        return Ok(value);
    }

    if let Ok(value) = row.try_get_by_index::<i64>(0) {
        return Ok(value != 0);
    }

    if let Ok(value) = row.try_get_by_index::<i32>(0) {
        return Ok(value != 0);
    }

    if let Ok(value) = row.try_get_by_index::<u64>(0) {
        return Ok(value != 0);
    }

    if let Ok(value) = row.try_get_by_index::<u32>(0) {
        return Ok(value != 0);
    }

    Err(Error::query(
        "Unable to decode database EXISTS result as a boolean or integer",
    ))
}

/// Insert models one row at a time, preserving the caller's ordering.
///
/// Used by the backends that cannot execute a multi-row `INSERT ... RETURNING`.
/// The caller is responsible for providing a transactional connection so the
/// whole batch stays all-or-nothing.
async fn insert_models_individually<M, C>(
    conn: &C,
    models: Vec<M>,
    error_context: &crate::error::ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
    <<M as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<M::ActiveModel>,
    C: ConnectionTrait,
{
    let mut results = Vec::with_capacity(models.len());
    for model in models {
        let active = model.try_into_active_model()?;
        let result = crate::profiling::__profile_future(async move { active.insert(conn).await })
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(error_context.clone()))?;
        results.push(M::try_from_entity_model(result)?);
    }

    Ok(results)
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
            Some(row) => query_result_exists_bool(&row),
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

    // Deletes deliberately do not live here. `into_active_model` produces an
    // insert-shaped `ActiveModel` — an auto-increment primary key is `NotSet` —
    // so an executor-side delete would fail with `PrimaryKeyNotSet` before it
    // reached the database. The macro emits `__into_delete_active_model`, which
    // sets the primary key, and `Model::delete` uses that instead.

    /// Insert multiple records in a single batch INSERT statement
    ///
    /// This constructs a multi-row INSERT instead of individual inserts,
    /// reducing the number of database round trips from O(n) to O(1).
    ///
    /// On PostgreSQL and MariaDB, uses `INSERT ... RETURNING` for efficiency.
    /// On MySQL and SQLite, falls back to individual inserts since they don't
    /// support multi-row `INSERT ... RETURNING`. Those inserts run inside a
    /// transaction — a savepoint when the caller already opened one — so the
    /// batch is all-or-nothing on every backend and the difference between
    /// backends stays an efficiency one.
    pub async fn insert_many<M, C>(conn: &C, models: Vec<M>) -> Result<Vec<M>>
    where
        M: InternalModel + crate::model::Model,
        <<M as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<M::ActiveModel>,
        C: ConnectionTrait + TransactionTrait,
    {
        if models.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = models.len();
        let error_context =
            model_error_context::<M>(format!("insert_many(batch_size={})", batch_size));

        // For single model, use regular insert for simplicity
        if models.len() == 1 {
            let active = models
                .into_iter()
                .next()
                .ok_or_else(|| Error::internal("insert_many batch iterator unexpectedly empty"))?
                .try_into_active_model()?;
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
            // MySQL/SQLite: fall back to individual inserts because they don't
            // support multi-row INSERT ... RETURNING. Wrap them in a transaction
            // so a failure halfway through cannot leave earlier rows committed.
            // `begin()` on an open transaction opens a SAVEPOINT, so this also
            // nests correctly inside a caller's transaction.
            let txn = conn
                .begin()
                .await
                .map_err(translate_error)
                .map_err(|err| err.with_context(error_context.clone()))?;

            let inserted = insert_models_individually::<M, _>(&txn, models, &error_context).await;

            match inserted {
                Ok(results) => {
                    txn.commit()
                        .await
                        .map_err(translate_error)
                        .map_err(|err| err.with_context(error_context.clone()))?;
                    Ok(results)
                }
                Err(err) => {
                    let _ = txn.rollback().await;
                    Err(err)
                }
            }
        }
    }
}

#[cfg(test)]
mod error_translation_tests {
    use super::{ConnAcquireErr, DbFailureKind, Error, OrmError, driver_failure, translate_error};
    use std::error::Error as StdError;

    #[test]
    fn migration_failures_are_query_errors_like_the_migrator_reports() {
        match translate_error(OrmError::Migration("relation exists".to_string())) {
            Error::Query { message, .. } => assert_eq!(message, "relation exists"),
            other => panic!("unexpected translation: {other:?}"),
        }
    }

    #[test]
    fn access_control_failures_get_their_own_variants() {
        match translate_error(OrmError::AccessDenied {
            permission: "delete".to_string(),
            resource: "users".to_string(),
        }) {
            Error::AccessDenied {
                permission,
                resource,
            } => {
                assert_eq!(permission, "delete");
                assert_eq!(resource, "users");
            }
            other => panic!("unexpected translation: {other:?}"),
        }

        match translate_error(OrmError::RbacError("denied".to_string())) {
            Error::Rbac { message } => assert_eq!(message, "denied"),
            other => panic!("unexpected translation: {other:?}"),
        }
    }

    #[test]
    fn engine_errors_without_a_tideorm_counterpart_stay_internal() {
        for err in [
            OrmError::Custom("boom".to_string()),
            OrmError::MutexPoisonError,
        ] {
            let translated = translate_error(err);
            assert!(
                matches!(translated, Error::Internal { .. }),
                "unexpected translation: {translated:?}"
            );
        }
    }

    #[test]
    fn pool_acquire_failures_keep_a_retryable_classification() {
        let translated = translate_error(OrmError::ConnectionAcquire(ConnAcquireErr::Timeout));

        assert!(translated.is_connection_error());
        assert_eq!(translated.failure_kind(), DbFailureKind::ConnectionTimeout);
        assert!(translated.is_retryable());
    }

    #[test]
    fn the_engine_error_survives_as_the_source_of_the_translated_error() {
        let translated = translate_error(OrmError::ConnectionAcquire(
            ConnAcquireErr::ConnectionClosed,
        ));

        // Error -> DbFailure -> engine error. The last hop is what a `{:#}`
        // chain or an `anyhow` report needs to reach the driver.
        let failure = StdError::source(&translated).expect("driver failure is the source");
        assert!(
            failure.source().is_some(),
            "the engine error must stay reachable through the failure"
        );
    }

    #[test]
    fn engine_bookkeeping_errors_carry_no_driver_failure() {
        assert!(driver_failure(&OrmError::MutexPoisonError).is_none());
        assert!(driver_failure(&OrmError::RecordNotInserted).is_none());
    }
}

#[cfg(test)]
mod encryption_blind_conversion_tests {
    use super::reject_encryption_blind_conversion;
    use crate::model::ModelMeta;

    #[derive(Clone)]
    struct PlainModel;

    impl ModelMeta for PlainModel {
        type PrimaryKey = i64;

        fn table_name() -> &'static str {
            "plain_models"
        }

        fn primary_key_names() -> &'static [&'static str] {
            &["id"]
        }

        fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
            primary_key.to_string()
        }

        fn column_names() -> &'static [&'static str] {
            &["id", "name"]
        }

        fn field_names() -> &'static [&'static str] {
            &["id", "name"]
        }
    }

    #[derive(Clone)]
    struct EncryptedModel;

    impl ModelMeta for EncryptedModel {
        type PrimaryKey = i64;

        fn table_name() -> &'static str {
            "encrypted_models"
        }

        fn primary_key_names() -> &'static [&'static str] {
            &["id"]
        }

        fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
            primary_key.to_string()
        }

        fn column_names() -> &'static [&'static str] {
            &["id", "secret"]
        }

        fn field_names() -> &'static [&'static str] {
            &["id", "secret"]
        }

        fn encrypted_fields() -> Vec<&'static str> {
            vec!["secret"]
        }
    }

    #[test]
    fn models_without_encrypted_fields_keep_the_plaintext_fallback() {
        assert!(reject_encryption_blind_conversion::<PlainModel>("try_into_active_model").is_ok());
    }

    #[test]
    fn an_encrypted_model_never_falls_back_to_the_plaintext_conversion() {
        let error = reject_encryption_blind_conversion::<EncryptedModel>("try_from_entity_model")
            .expect_err("the plaintext fallback cannot decrypt");
        let message = error.to_string();

        assert!(message.contains("try_from_entity_model"), "{message}");
        assert!(message.contains("encrypted_models"), "{message}");
        assert!(message.contains("secret"), "{message}");
    }
}

#[cfg(test)]
#[path = "../../tests/unit/internal_tests.rs"]
mod tests;
