//! Internal ORM adapter layer.
//!
//! Everything that names the ORM engine goes through here, so the engine stays an
//! implementation detail: models reach it through [`InternalModel`], the typed
//! read/write paths live on [`QueryExecutor`], and engine errors become TideORM
//! errors in `translate_error`.

use crate::error::{DbFailure, DbFailureKind, Error, Result};
// `ConnAcquireErr` and `RuntimeErr` arrive through the `entity::prelude::*` re-export
// below; importing them again privately would shadow that public glob.
use crate::soft_delete::{SoftDeleteScope, query_scope_for};

mod backend;
mod batch_insert;
mod column_values;
mod executor;
mod per_poll;
#[cfg(feature = "fulltext")]
pub(crate) mod sql_builder;
pub(crate) mod sql_safety;
mod topology;

pub use column_values::__column_type_of;
pub(crate) use column_values::{
    column_type_of, is_integer, json_to_assignment_value, json_to_column_value,
};
pub use executor::Executor;
pub(crate) use per_poll::per_poll;
pub(crate) use topology::topological_order;

// Re-export the ORM engine through TideORM's facade, broadly, so other modules
// can import selectively.
pub use crate::orm::{
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, Condition,
    ConnectOptions, ConnectionTrait, Database as OrmDatabase, DatabaseConnection as OrmConnection,
    DatabaseTransaction as OrmTransaction, DbBackend as OrmBackend, DbErr as OrmError, DeleteMany,
    DeriveEntityModel, DeriveRelation, EntityTrait, EnumIter, ExecResult, FromQueryResult, Iden,
    IntoActiveModel, Iterable, LoaderTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Related, RelationDef, RelationTrait, Statement as OrmStatement,
    TransactionSession, TransactionTrait, TryGetable, Value,
    entity::prelude::*,
    sea_query::{
        Alias, Asterisk, ColumnDef as OrmColumnDef, ColumnType as OrmColumnType, Expr, ExprTrait,
        Index, MysqlQueryBuilder, OnConflict, PostgresQueryBuilder, Query, SimpleExpr,
        SqliteQueryBuilder, Table, Token, Tokenizer, extension::postgres::PgBinOper,
        inject_parameters,
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

/// A JSON value as text: a string's contents, anything else as JSON.
pub(crate) fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

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
                bindable_value(unsigned)
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

/// `value` as a parameter every driver can bind.
///
/// The SQLite and PostgreSQL drivers panic converting an unsigned 64-bit value
/// past `i64::MAX` to their signed integer, so such a value is bound as an exact
/// decimal, which each backend still compares with an integer column. Generated
/// primary-key conditions bind through here, so a `u64` key read from a request
/// cannot crash `find`.
#[doc(hidden)]
pub fn bindable_value(value: impl Into<Value>) -> Value {
    match value.into() {
        Value::BigUnsigned(Some(unsigned)) if i64::try_from(unsigned).is_err() => {
            Value::Decimal(Some(rust_decimal::Decimal::from(unsigned)))
        }
        value => value,
    }
}

/// Refuse to write `M` through `conn` when its driver cannot store one of `M`'s
/// integer fields and read it back.
///
/// The write would land and reading the stored row back would fail, so the
/// caller would get an error for a row that exists, and a retry would store it
/// twice; a `u64` past `i64::MAX` panics the driver instead. MySQL's driver
/// handles every width.
pub(crate) fn ensure_fields_storable<M, C>(conn: &C) -> Result<()>
where
    M: crate::model::ModelMeta,
    C: ConnectionTrait,
{
    check_fields_storable::<M>(Backend::from(conn.get_database_backend()))
}

fn check_fields_storable<M: crate::model::ModelMeta>(backend: Backend) -> Result<()> {
    let unstorable: Vec<String> = M::driver_limited_fields()
        .iter()
        .filter_map(|&(field, rust_type)| {
            let replacement = match (backend, rust_type) {
                (Backend::Postgres, "i8" | "u8") => "i16",
                (Backend::Postgres, "u16") => "i32",
                (Backend::Postgres | Backend::Sqlite, "u64") => "i64",
                _ => return None,
            };
            Some(format!(
                "field `{field}` is a `{rust_type}`; declare it as `{replacement}`"
            ))
        })
        .collect();
    if unstorable.is_empty() {
        return Ok(());
    }
    Err(Error::conversion(format!(
        "the {} driver cannot store and read back every field of `{}`: {}",
        backend.as_database_type(),
        M::table_name(),
        unstorable.join(", ")
    )))
}

/// The marker for the `index`-th (1-based) bound parameter: `$index` on
/// PostgreSQL, a positional `?` everywhere else.
///
/// `Expr::cust_with_values` only substitutes the marker its query builder
/// emits, so a fragment written with the other backend's marker binds nothing.
pub(crate) fn placeholder(db_type: crate::config::DatabaseType, index: usize) -> String {
    match db_type {
        crate::config::DatabaseType::Postgres => format!("${}", index),
        crate::config::DatabaseType::MySQL
        | crate::config::DatabaseType::MariaDB
        | crate::config::DatabaseType::SQLite => "?".to_string(),
    }
}

/// Bind `value` and return the placeholder that refers to it.
pub(crate) fn push_param(
    db_type: crate::config::DatabaseType,
    params: &mut Vec<Value>,
    value: Value,
) -> String {
    params.push(value);
    placeholder(db_type, params.len())
}

/// Refuse an encryption-blind fallback before it can move plaintext or
/// ciphertext across the persistence boundary.
///
/// `InternalModel::try_into_active_model` and `try_to_entity_model` default to
/// their infallible twins, which is correct for the overwhelming majority of
/// models: they declare no `#[tideorm(encrypted)]` fields, so the two paths are
/// the same conversion and the default saves every model from spelling it twice.
/// A model that *does* declare encrypted fields must override them — the derive
/// does — because the infallible half has nowhere to report a cipher failure and
/// would write plaintext into a ciphertext column. If that override is ever
/// missing, this turns a silent data-integrity bug into an error at the point of
/// conversion.
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
/// The outbound conversions come in pairs. The `try_` half is the persistence
/// path: it is where encrypted fields are encrypted, so it is the only half
/// allowed to reach the database. The infallible half is a plaintext, in-memory
/// convenience that cannot report a cipher failure — never build a statement
/// from it. The `try_` defaults bridge to it only for models with no encrypted
/// fields; see `reject_encryption_blind_conversion`. The inbound conversion,
/// which decrypts, only exists in its fallible form.
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

    /// Convert the generated entity model to a TideORM model, decrypting
    /// encrypted fields.
    fn try_from_entity_model(model: <Self::Entity as EntityTrait>::Model) -> Result<Self>;

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

    /// Resolve an entity column enum from either a field name or column name,
    /// a column name first, as [`canonical_column_name`](crate::model::ModelMeta::canonical_column_name)
    /// resolves one.
    fn column_from_str(name: &str) -> Option<<Self::Entity as EntityTrait>::Column> {
        let column = Self::canonical_column_name(name)?;
        <<Self::Entity as EntityTrait>::Column as crate::orm::Iterable>::iter()
            .find(|candidate| crate::orm::IdenStatic::as_str(candidate) == column)
    }

    /// Get entity primary key columns.
    fn primary_key_columns() -> Vec<<Self::Entity as EntityTrait>::Column> {
        Vec::new()
    }

    /// Get the ORM condition for an exact primary key match.
    fn primary_key_condition(
        primary_key: &<Self as crate::model::ModelMeta>::PrimaryKey,
    ) -> Condition;

    /// Rebuild runtime-only relation wrappers after an in-memory model overwrite.
    fn refresh_runtime_relations_from(&mut self, _previous: &Self) {}

    /// This model with its relation wrappers built from its own fields, as a
    /// load builds them; for a model read back from JSON, whose wrappers a
    /// user's `Deserialize` leaves detached.
    #[doc(hidden)]
    fn __rebuild_relations(self) -> Self
    where
        Self: Sized,
    {
        self
    }

    /// Get one model field as JSON without serializing the full model.
    fn field_json_value(&self, _field: &str) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }

    /// Set one model field, by field or column name, from JSON; `false` when
    /// the model has no such field.
    fn set_field_json(&mut self, _field: &str, _value: serde_json::Value) -> Result<bool> {
        Ok(false)
    }
}

/// Leave statement logging to TideORM: `TIDE_LOG_QUERIES` and the
/// `QueryLogger` decide what is printed, where the driver would log every
/// statement at `INFO` to any subscriber listening.
pub(crate) fn quiet_driver_logging(options: &mut ConnectOptions) {
    options.sqlx_logging(false);
}

/// Internal connection wrapper
#[doc(hidden)]
pub struct InternalConnection {
    pub(crate) conn: OrmConnection,
    /// Whether the server behind a MySQL-protocol connection is MariaDB,
    /// which SeaORM cannot tell apart and some SQL is rendered differently
    /// for.
    mariadb: bool,
}

impl InternalConnection {
    /// Wrap a freshly opened `conn`, asking a MySQL-protocol server whether
    /// it is MariaDB. The pool has only just opened, so a failure even of
    /// `SELECT VERSION()` means it is unusable, and is returned.
    pub(crate) async fn open(conn: OrmConnection) -> Result<Self> {
        let mut connection = Self::new(conn);
        if Backend::from_orm_backend(connection.conn.get_database_backend()) == Some(Backend::MySql)
        {
            let statement = OrmStatement::from_string(
                connection.conn.get_database_backend(),
                "SELECT VERSION() AS version",
            );
            let version: String = connection
                .conn
                .query_one_raw(statement)
                .await
                .map_err(translate_connection_error)?
                .ok_or_else(|| Error::query("SELECT VERSION() returned no row"))?
                .try_get("", "version")
                .map_err(translate_error)?;
            connection.mariadb = version.to_ascii_lowercase().contains("mariadb");
        }
        Ok(connection)
    }

    /// Wrap `conn`, reporting the statements the engine renders itself — the
    /// typed CRUD paths and raw SQL — to the query log.
    pub(crate) fn new(mut conn: OrmConnection) -> Self {
        conn.set_metric_callback(crate::logging::log_engine_statement);
        Self {
            conn,
            mariadb: false,
        }
    }

    pub fn connection(&self) -> &OrmConnection {
        &self.conn
    }

    /// Whether the server is MariaDB.
    pub(crate) fn is_mariadb(&self) -> bool {
        self.mariadb
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
        return DbFailure::new(transport_failure_kind(driver_error));
    };

    let code = database_error.code().map(|code| code.into_owned());
    let kind = match database_error.kind() {
        ErrorKind::UniqueViolation => DbFailureKind::UniqueViolation,
        ErrorKind::ForeignKeyViolation => DbFailureKind::ForeignKeyViolation,
        ErrorKind::NotNullViolation => DbFailureKind::NotNullViolation,
        ErrorKind::CheckViolation => DbFailureKind::CheckViolation,
        // A SQLSTATE is five characters; SQLite's native codes are at most four
        // digits.
        _ => match code.as_deref() {
            Some(code) if code.len() < 5 && code.bytes().all(|b| b.is_ascii_digit()) => {
                DbFailureKind::from_sqlite_code(code, database_error.message())
            }
            Some(sqlstate) => {
                match mysql_error_number(database_error).map(DbFailureKind::from_mysql_code) {
                    Some(kind) if kind != DbFailureKind::Unclassified => kind,
                    _ => DbFailureKind::from_sqlstate(sqlstate),
                }
            }
            None => DbFailureKind::Unclassified,
        },
    };

    DbFailure::new(kind)
        .with_code(code)
        .with_constraint(database_error.constraint().map(str::to_string))
        .with_table(database_error.table().map(str::to_string))
}

/// Classify a driver failure that carries no database error: the connection
/// broke under the statement — MySQL's `KILL` and a server restart close the
/// socket without a word — or no connection could be had.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn transport_failure_kind(error: &crate::orm::sqlx::Error) -> DbFailureKind {
    use crate::orm::sqlx::Error as DriverError;

    match error {
        DriverError::Io(_) | DriverError::PoolClosed | DriverError::WorkerCrashed => {
            DbFailureKind::ConnectionClosed
        }
        DriverError::PoolTimedOut => DbFailureKind::ConnectionTimeout,
        _ => DbFailureKind::Unclassified,
    }
}

#[cfg(feature = "mysql")]
fn mysql_error_number(error: &dyn crate::orm::sqlx::error::DatabaseError) -> Option<u16> {
    error
        .try_downcast_ref::<crate::orm::sqlx::mysql::MySqlDatabaseError>()
        .map(|error| error.number())
}

#[cfg(all(not(feature = "mysql"), any(feature = "postgres", feature = "sqlite")))]
fn mysql_error_number(_error: &dyn crate::orm::sqlx::error::DatabaseError) -> Option<u16> {
    None
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
    let error = translate_engine_error(err).with_db_failure(failure);
    // A failure inside a transaction can leave it unable to commit; the
    // transaction asks before it does (see `Database::transaction`).
    crate::cache::note_failed_statement(
        error.failure_kind() == crate::error::DbFailureKind::Deadlock,
    );
    error
}

/// Translate an engine error from reaching or probing the server.
///
/// Whatever the driver called it, such a failure is a connection failure; the
/// structured driver detail is carried over so the SQLSTATE and the source
/// chain survive the reclassification.
/// Translate a failure to open a pool on `url`.
///
/// The engine quotes a URL it cannot parse or has no driver for, password and
/// all. That error is reported with the URL's credentials masked, and without
/// the engine error behind it, so logging it cannot leak them.
pub(crate) fn translate_connect_error(err: OrmError, url: &str) -> Error {
    match translate_connection_error(err) {
        Error::Connection { message, .. } if message.contains(url) => {
            Error::connection(message.replace(url, &mask_url_credentials(url)))
        }
        other => other,
    }
}

/// `url` with its credentials masked: everything between the scheme and the
/// last `@` before the query (its user and password, delimited the way a URL
/// parser delimits them), and the value of a query parameter that carries a
/// password, which the PostgreSQL driver reads as `?password=..`. The CLI
/// prints URLs through it too.
#[doc(hidden)]
pub fn mask_url_credentials(url: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let base = match (base.find("://"), base.rfind('@')) {
        (Some(scheme_end), Some(at)) if at > scheme_end => {
            format!("{}://***{}", &base[..scheme_end], &base[at..])
        }
        _ => base.to_string(),
    };
    let Some(query) = query else {
        return base;
    };
    let query: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, _))
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "password" | "sslpassword" | "passwd" | "pwd"
                ) =>
            {
                format!("{key}=***")
            }
            _ => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", query.join("&"))
}

pub(crate) fn translate_connection_error(err: OrmError) -> Error {
    reclassify_error(err, |message, source| Error::Connection { message, source })
}

/// Translate `err` and file it under the variant `class` builds, unless it
/// is a connection failure, which keeps its own. The variant changes, so the
/// structured driver failure is carried over explicitly: otherwise a
/// serialization failure would arrive with no SQLSTATE and no source chain.
pub(crate) fn reclassify_error(
    err: OrmError,
    class: impl FnOnce(String, Option<Box<crate::error::DbFailure>>) -> Error,
) -> Error {
    let message = err.to_string();
    match translate_error(err) {
        connection @ Error::Connection { .. } => connection,
        other => class(message, other.into_db_failure()),
    }
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

#[doc(hidden)]
pub fn model_error_context<M>(query: impl Into<String>) -> crate::error::ErrorContext
where
    M: crate::model::Model,
{
    crate::error::ErrorContext::new()
        .table(M::table_name())
        .query(query.into())
}

/// [`model_error_context`] for a statement that addresses one row by primary key.
#[doc(hidden)]
pub fn primary_key_error_context<M>(
    primary_key: &M::PrimaryKey,
    query: impl Into<String>,
) -> crate::error::ErrorContext
where
    M: crate::model::Model,
{
    let condition = M::primary_key_display(primary_key);
    model_error_context::<M>(query)
        .condition(condition.clone())
        .operator_chain(condition)
}

pub(crate) fn count_to_u64(count: i64, context: &str) -> Result<u64> {
    u64::try_from(count).map_err(|_| {
        Error::query(format!(
            "Database returned a negative count ({count}) for {context}"
        ))
    })
}

fn build_count_select<M>() -> Select<M::Entity>
where
    M: InternalModel + crate::model::Model,
{
    scoped_find::<M>()
        .select_only()
        .column_as(Expr::col(Asterisk).count(), "count")
}

/// `select` limited to its first row, with the `LIMIT 1` written into the SQL.
///
/// `Select::one` binds that limit as a parameter, and recent SQLite releases
/// recompile a statement whose `LIMIT` is a bound parameter every time it runs.
/// `select` must not carry a limit of its own.
fn one_row_statement<E: EntityTrait>(select: Select<E>, backend: OrmBackend) -> OrmStatement {
    let mut statement = select.build(backend);
    statement.sql.push_str(" LIMIT 1");
    statement
}

/// The first row `select` finds, read through [`one_row_statement`]. The
/// generated upsert reload uses it.
#[doc(hidden)]
pub async fn first_row<E, C>(
    select: Select<E>,
    conn: &C,
) -> std::result::Result<Option<E::Model>, OrmError>
where
    E: EntityTrait,
    C: ConnectionTrait,
{
    let statement = one_row_statement(select, conn.get_database_backend());
    E::Model::find_by_statement(statement).one(conn).await
}

/// `select` narrowed to `limit` rows after the first `offset`, with the limit
/// written into the SQL and the offset bound.
///
/// That keeps one statement per page size, reused for every page, where a bound
/// limit would be recompiled on every run (see [`one_row_statement`]).
fn page_statement<E: EntityTrait>(
    select: Select<E>,
    backend: OrmBackend,
    limit: i64,
    offset: i64,
) -> OrmStatement {
    let mut statement = select.build(backend);
    let values = &mut statement
        .values
        .get_or_insert_with(|| crate::orm::Values(Vec::new()))
        .0;
    values.push(Value::BigInt(Some(offset)));
    let offset_marker = placeholder(Backend::from(backend).as_database_type(), values.len());
    statement
        .sql
        .push_str(&format!(" LIMIT {limit} OFFSET {offset_marker}"));
    statement
}

/// `SELECT 1 .. LIMIT 1` over the model's scoped rows.
///
/// SeaORM's own `SelectExt::exists` drops the limit and relies on the driver to
/// stop after one row, which MySQL cannot do: it drains every matching row.
fn build_exists_any_statement<M>(backend: OrmBackend) -> OrmStatement
where
    M: InternalModel + crate::model::Model,
{
    let select = scoped_find::<M>()
        .select_only()
        .column_as(Expr::cust("1"), "tideorm_exists");
    one_row_statement(select, backend)
}

/// `M::Entity::find()` with `M`'s soft-delete scope applied, so trashed rows
/// stay hidden. Generated eager loaders start their relation queries here.
#[doc(hidden)]
pub fn scoped_find<M>() -> Select<M::Entity>
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

/// Run a statement's future profiled, with its error translated and given
/// the context `context` builds.
#[doc(hidden)]
pub async fn run_profiled<T>(
    statement: impl std::future::Future<Output = std::result::Result<T, OrmError>>,
    context: impl FnOnce() -> crate::error::ErrorContext,
) -> Result<T> {
    crate::profiling::__profile_future(statement)
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(context()))
}

/// The row with primary key `id` on `connection`, a trashed one only when
/// `include_trashed`. The generated `find`/`find_with`, `reload()` and
/// `soft_delete()` read through it; `what` names the call in an error.
///
/// A key matches at most one row, so the lookup reads with `all()`: `one()`
/// adds a bound `LIMIT`, which recent SQLite releases recompile the statement
/// for on every run.
#[doc(hidden)]
pub async fn find_by_primary_key<M>(
    connection: &crate::database::ConnectionRef,
    id: &M::PrimaryKey,
    include_trashed: bool,
    what: &str,
) -> Result<Option<M>>
where
    M: crate::model::Model,
{
    let select = if include_trashed {
        M::Entity::find()
    } else {
        scoped_find::<M>()
    };
    let rows = run_profiled(
        select
            .filter(M::primary_key_condition(id))
            .all(&connection.executor()),
        || primary_key_error_context::<M>(id, format!("{what}({})", M::primary_key_display(id))),
    )
    .await?;
    // The baseline dirty tracking keeps is the one of the database the row
    // came from.
    crate::model::__loading_through(connection, || {
        rows.into_iter()
            .next()
            .map(M::try_from_entity_model)
            .transpose()
    })
}

/// The typed reads the `Model` defaults run, each on the current connection.
///
/// The connection is resolved before the profiled statement, so a missing or
/// unreachable database keeps its `Error::Connection` class.
#[doc(hidden)]
pub struct QueryExecutor;

impl QueryExecutor {
    /// Every row in the model's soft-delete scope.
    pub async fn find_all<M>() -> Result<Vec<M>>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        let rows = run_profiled(scoped_find::<M>().all(&connection.executor()), || {
            model_error_context::<M>("find_all()")
        })
        .await?;
        rows.into_iter().map(M::try_from_entity_model).collect()
    }

    /// The first row in scope, in the backend's natural order.
    pub async fn first<M>() -> Result<Option<M>>
    where
        M: crate::model::Model,
    {
        Self::first_of::<M>(scoped_find::<M>(), "first()".to_string()).await
    }

    /// The row in scope with the highest primary key; unordered when the model
    /// declares no primary key column.
    pub async fn last<M>() -> Result<Option<M>>
    where
        M: crate::model::Model,
    {
        let mut select = scoped_find::<M>();
        let mut query_label = String::from("last()");

        let pk_columns = M::primary_key_columns();
        if !pk_columns.is_empty() {
            for pk_col in pk_columns {
                select = select.order_by_desc(pk_col);
            }
            query_label = format!("last(order_by={} desc)", M::primary_key_names().join(", "));
        }

        Self::first_of::<M>(select, query_label).await
    }

    /// The first row `select` finds, read through [`first_row`].
    async fn first_of<M>(select: Select<M::Entity>, query_label: String) -> Result<Option<M>>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        let row = run_profiled(first_row(select, &connection.executor()), || {
            model_error_context::<M>(query_label)
        })
        .await?;
        row.map(M::try_from_entity_model).transpose()
    }

    /// `COUNT(*)` over the model's scoped rows.
    ///
    /// The statement always returns exactly one row, so it is read with `all`,
    /// which adds no bound `LIMIT` (see [`one_row_statement`]), and a missing
    /// row is a decode failure rather than an empty table.
    pub async fn count<M>() -> Result<u64>
    where
        M: crate::model::Model,
    {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count: i64,
        }

        let connection = crate::database::__current_connection()?;
        let rows: Vec<CountResult> = run_profiled(
            build_count_select::<M>()
                .into_model::<CountResult>()
                .all(&connection.executor()),
            || model_error_context::<M>("count(*)"),
        )
        .await?;

        let row = rows
            .into_iter()
            .next()
            .ok_or_else(|| Error::query("Database returned no row for count(*)"))?;
        count_to_u64(row.count, "count(*)")
    }

    /// Whether any row is in the model's scope.
    pub async fn exists_any<M>() -> Result<bool>
    where
        M: crate::model::Model,
    {
        let connection = crate::database::__current_connection()?;
        let executor = connection.executor();
        let statement = build_exists_any_statement::<M>(executor.get_database_backend());
        let row = run_profiled(executor.query_one_raw(statement), || {
            model_error_context::<M>("exists_any()")
        })
        .await?;
        Ok(row.is_some())
    }

    /// Page `page` of `per_page` rows in scope, in primary key order.
    ///
    /// Without an order PostgreSQL returns rows in heap order, which an UPDATE
    /// changes: a row rewritten between two page requests moves, and the next
    /// page repeats it and skips another.
    pub async fn paginate<M>(page: u64, per_page: u64) -> Result<Vec<M>>
    where
        M: crate::model::Model,
    {
        // The same check `QueryBuilder::page` makes, so both refuse alike.
        // Every backend takes LIMIT and OFFSET as signed 64-bit integers,
        // which it already bounds both by.
        let offset = crate::query::page_offset(page, per_page)
            .map_err(|(field, message)| Error::validation(field, message))?;
        let (limit, offset) = (per_page as i64, offset as i64);

        let mut select = scoped_find::<M>();
        for pk_col in M::primary_key_columns() {
            select = select.order_by_asc(pk_col);
        }
        let connection = crate::database::__current_connection()?;
        let executor = connection.executor();
        let statement = page_statement(select, executor.get_database_backend(), limit, offset);
        let rows = run_profiled(
            <M::Entity as EntityTrait>::Model::find_by_statement(statement).all(&executor),
            || model_error_context::<M>(format!("paginate(limit={}, offset={})", limit, offset)),
        )
        .await?;
        rows.into_iter().map(M::try_from_entity_model).collect()
    }

    // Deletes deliberately do not live here: the macro-generated `delete` and
    // `destroy` filter on `primary_key_condition`, which binds the key through
    // `bindable_value`, rather than deleting an `ActiveModel`.

    /// Insert `models` on the current connection and return them as stored,
    /// in the order they were passed; see `batch_insert` for the statements
    /// each backend runs.
    pub async fn insert_many<M>(models: Vec<M>) -> Result<Vec<M>>
    where
        M: crate::model::Model,
        <<M as InternalModel>::Entity as EntityTrait>::Model: IntoActiveModel<M::ActiveModel>,
    {
        let connection = crate::database::__current_connection()?;
        batch_insert::insert_many(&connection.executor(), models).await
    }
}

#[cfg(test)]
#[path = "../../tests/unit/internal_tests.rs"]
mod tests;
