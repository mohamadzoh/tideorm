//! Internal ORM adapter layer.
//!
//! Everything that names the ORM engine goes through here, so the engine stays an
//! implementation detail: models reach it through [`InternalModel`], the typed
//! read/write paths live on [`QueryExecutor`], and engine errors become TideORM
//! errors in `translate_error`.

use crate::config::DatabaseType;
use crate::error::{Error, Result};
// `ConnAcquireErr` and `RuntimeErr` arrive through the `entity::prelude::*` re-export
// below; importing them again privately would shadow that public glob.
use crate::soft_delete::{SoftDeleteScope, query_scope_for};

mod backend;
mod batch_insert;
mod column_values;
mod error_translation;
mod json_loss;
mod key_sequence;
mod per_poll;
mod query_executor;
pub(crate) mod sql_lexer;
pub(crate) mod sql_safety;
mod topology;

pub use column_values::__column_type_of;
pub(crate) use column_values::{
    array_element, column_type_of, column_value, is_integer, json_to_column_value, value_document,
};
pub(crate) use error_translation::{reclassify_error, translate_connection_error, translate_error};
pub(crate) use json_loss::json_loss;
pub(crate) use key_sequence::{advance_key_sequence, holds_auto_increment_key};
pub(crate) use per_poll::per_poll;
pub use query_executor::QueryExecutor;
pub(crate) use topology::topological_order;

// Re-export the ORM engine through TideORM's facade, broadly, so other modules
// can import selectively.
pub use crate::orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, Condition, ConnectOptions,
    ConnectTarget, ConnectionTrait, Database as OrmDatabase, DatabaseConnection as OrmConnection,
    DatabaseTransaction as OrmTransaction, DbErr as OrmError, DeleteMany, EntityTrait, EnumIter,
    ExecResult, FromQueryResult, Iden, IntoActiveModel, Iterable, ModelTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, QueryTrait, Statement as OrmStatement, TransactionTrait,
    TryGetable, Value,
    entity::prelude::*,
    sql_builder::{
        Alias, Asterisk, ColumnType as OrmColumnType, Expr, ExprTrait, MysqlQueryBuilder,
        OnConflict, PostgresQueryBuilder, Query, SimpleExpr, SqliteQueryBuilder,
        extension::postgres::PgBinOper, inject_parameters,
    },
};

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub use crate::orm::driver;

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
/// The SQLite and PostgreSQL drivers cannot encode an unsigned 64-bit value
/// past `i64::MAX` as their signed integer, which fails the statement, so such
/// a value is bound as an exact decimal, which each backend still compares with
/// an integer column. Generated primary-key conditions bind through here, so a
/// `u64` key read from a request finds no row instead of failing `find`.
#[doc(hidden)]
pub fn bindable_value(value: impl Into<Value>) -> Value {
    match value.into() {
        Value::BigUnsigned(Some(unsigned)) if i64::try_from(unsigned).is_err() => {
            Value::Decimal(Some(rust_decimal::Decimal::from(unsigned)))
        }
        value => value,
    }
}

/// Refuse to write `active` to SQLite when one of its values could not be read
/// back: SQLite stores a non-finite float, and a decimal past what its REAL
/// storage can return to a `Decimal`, but the model then fails to load the row.
pub(crate) fn ensure_active_values_storable<M: crate::model::Model>(
    active: &M::ActiveModel,
    conn: &impl ConnectionTrait,
) -> Result<()> {
    if conn.get_database_backend() != DatabaseType::SQLite {
        return Ok(());
    }
    // The generated entity's columns are `M::column_names()`, in that order,
    // so every write goes through them without looking each name up.
    for column in <M::Entity as EntityTrait>::Column::iter() {
        let invalid = match active.get(column).into_value() {
            Some(Value::Float(Some(value))) => !value.is_finite(),
            Some(Value::Double(Some(value))) => !value.is_finite(),
            Some(Value::Decimal(Some(value))) => !sqlite_decimal_is_storable(value),
            _ => false,
        };
        if invalid {
            let name = crate::orm::IdenStatic::as_str(&column);
            return Err(Error::query(format!(
                "SQLite value for {name} must be finite and within its readable model range"
            )));
        }
    }
    Ok(())
}

/// The largest decimal magnitude SQLite can return: a `REAL` column stores a
/// `Decimal` as an `f64`, and 2^96 rounds past `Decimal::MAX`, so the `f64`
/// just below it.
pub(crate) const SQLITE_DECIMAL_MAX: f64 = 79_228_162_514_264_337_593_543_950_336_f64.next_down();

/// Whether SQLite can store `value` and read it back as a `Decimal`.
pub(crate) fn sqlite_decimal_is_storable(value: rust_decimal::Decimal) -> bool {
    use rust_decimal::prelude::ToPrimitive;
    value
        .to_f64()
        .is_some_and(|number| number.abs() <= SQLITE_DECIMAL_MAX)
}

/// Bind `value` and return the placeholder that refers to it.
pub(crate) fn push_param(
    db_type: crate::config::DatabaseType,
    params: &mut Vec<Value>,
    value: Value,
) -> String {
    params.push(value);
    db_type.dialect().marker(params.len())
}

/// Internal trait that maps TideORM models to ORM engine entities.
/// This is implemented by TideORM's model macros.
///
/// The model is its entity's model: rows decode into it, encrypted fields
/// decrypted as they decode, and the engine's `IntoActiveModel` writes its
/// columns as they are, encrypted ones left unset. A decoded row is not yet a
/// loaded model: its relation wrappers are detached until
/// [`finish_load`](Self::finish_load). A write builds its active model with
/// [`try_into_active_model`](Self::try_into_active_model), which encrypts.
#[doc(hidden)]
pub trait InternalModel:
    crate::model::ModelMeta
    + Sized
    + Send
    + Sync
    + Clone
    + ModelTrait<Entity: EntityTrait<Model = Self>>
    + FromQueryResult
    + IntoActiveModel<<Self as InternalModel>::ActiveModel>
{
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity> + Send;

    /// The active model a write sends: keys and managed timestamps set, and
    /// encrypted fields encrypted.
    fn try_into_active_model(self) -> Result<Self::ActiveModel>;

    /// A decoded row, read through `connection`, as a loaded model: relation
    /// wrappers built from its columns and querying the pool the row came
    /// from, and its dirty-tracking baseline recorded. Every load runs it once.
    fn finish_load(self, connection: &crate::database::ConnectionRef) -> Self;

    /// Whether every column holds what `other`'s does, compared as values, so
    /// a NaN float equals itself. Encrypted columns are compared in
    /// plaintext: two ciphertexts of a randomized cipher differ.
    fn same_columns(&self, other: &Self) -> bool {
        <<Self::Entity as EntityTrait>::Column as crate::orm::Iterable>::iter()
            .all(|column| self.get(column) == other.get(column))
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
    fn primary_key_columns() -> Vec<<Self::Entity as EntityTrait>::Column>;

    /// Get the ORM condition for an exact primary key match.
    fn primary_key_condition(
        primary_key: &<Self as crate::model::ModelMeta>::PrimaryKey,
    ) -> Condition;

    /// This model with its relation wrappers built from its own fields, as a
    /// load builds them; for a model read back from JSON, whose wrappers a
    /// user's `Deserialize` leaves detached.
    #[doc(hidden)]
    fn __rebuild_relations(self) -> Self;

    /// Get one model field as JSON without serializing the full model.
    fn field_json_value(&self, field: &str) -> Result<Option<serde_json::Value>>;

    /// The column of the field `name` names, by field or column name, a
    /// field's name first, as the derive's field accessors read one.
    fn field_column(name: &str) -> Option<<Self::Entity as EntityTrait>::Column> {
        let field = Self::canonical_field_name(name)?;
        let index = Self::field_names()
            .iter()
            .position(|known| *known == field)?;
        Self::column_from_str(Self::column_names().get(index)?)
    }

    /// One field, by field or column name, as the value its column holds:
    /// what dirty tracking and the entity manager compare. `None` when the
    /// model has no such field.
    fn field_value(&self, field: &str) -> Option<Value> {
        Self::field_column(field).map(|column| self.get(column))
    }

    /// Set one field, by field or column name, to `value`, an integer of any
    /// width for an integer field; `false` when the model has no such field.
    /// Relation wrappers are left alone.
    fn set_field_value(&mut self, field: &str, value: Value) -> Result<bool> {
        let Some(column) = Self::field_column(field) else {
            return Ok(false);
        };
        let value = column_values::integer_as(value, Self::get_value_type(column));
        self.try_set(column, value).map_err(|error| {
            Error::conversion(format!("Failed to set field '{field}': {error}"))
        })?;
        Ok(true)
    }

    /// Copy one field, by field or column name, from `source`; `false` when
    /// the model has no such field. The value itself is copied, encrypted
    /// fields in plaintext.
    fn copy_field_from(&mut self, source: &Self, field: &str) -> Result<bool> {
        match source.field_value(field) {
            Some(value) => self.set_field_value(field, value),
            None => Ok(false),
        }
    }

    /// Set one model field, by field or column name, from JSON; `false` when
    /// the model has no such field.
    fn set_field_json(&mut self, field: &str, value: serde_json::Value) -> Result<bool>;
}

/// Internal connection wrapper
#[doc(hidden)]
pub struct InternalConnection {
    pub(crate) conn: OrmConnection,
    pub(crate) cache_scope: crate::database::cache_scope::CacheScope,
    /// Where SQLite opened the main database file, canonicalized; `None`
    /// for an in-memory or temporary database and for other backends.
    #[cfg(feature = "sqlite")]
    pub(crate) sqlite_file: Option<std::path::PathBuf>,
}

impl InternalConnection {
    /// Wrap a freshly opened `conn`, identifying the database it reaches for
    /// the query cache.
    pub(crate) async fn open(conn: OrmConnection) -> Result<Self> {
        #[allow(unused_mut)]
        let mut connection = Self::new(conn);
        #[cfg(feature = "sqlite")]
        {
            connection.sqlite_file = connection
                .conn
                .sqlite_main_file()
                .await
                .map_err(translate_connection_error)?
                .map(|file| std::fs::canonicalize(&file).unwrap_or(file));
        }
        let cache_scope = crate::database::cache_scope::identify(&connection);
        *connection.cache_scope.write() = cache_scope;
        Ok(connection)
    }

    /// Wrap `conn`, reporting the statements the engine renders itself — the
    /// typed CRUD paths and raw SQL — to the query log.
    pub(crate) fn new(mut conn: OrmConnection) -> Self {
        conn.set_metric_callback(crate::logging::observe_engine_statement);
        Self {
            conn,
            cache_scope: Default::default(),
            #[cfg(feature = "sqlite")]
            sqlite_file: None,
        }
    }

    pub fn connection(&self) -> &OrmConnection {
        &self.conn
    }
}

/// `url` with its credentials masked, as the engine quotes it in its own
/// connection errors. The CLI prints URLs through it too.
#[doc(hidden)]
pub use crate::orm::mask_url_credentials;

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

/// Run a statement's future, with its error translated and given the context
/// `context` builds.
#[doc(hidden)]
pub async fn run_statement<T>(
    statement: impl std::future::Future<Output = std::result::Result<T, OrmError>>,
    context: impl FnOnce() -> crate::error::ErrorContext,
) -> Result<T> {
    statement
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(context()))
}

/// The row with primary key `id` on `connection`, a trashed one only when
/// `include_trashed`. The generated `find`/`find_with`, `reload()` and
/// `soft_delete()` read through it; `what` names the call in an error.
///
/// A key matches at most one row, so the lookup reads with `all()` and needs
/// no `LIMIT`.
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
    let rows = run_statement(
        select
            .filter(M::primary_key_condition(id))
            .all(&connection.executor()),
        || primary_key_error_context::<M>(id, format!("{what}({})", M::primary_key_display(id))),
    )
    .await?;
    // The baseline dirty tracking keeps is the one of the database the row
    // came from.
    Ok(rows
        .into_iter()
        .next()
        .map(|row| row.finish_load(connection)))
}

/// The decoded `rows` read through `connection`, loaded, each remembered as
/// read from that connection's pool.
pub(crate) fn models_read_through<M: crate::model::Model>(
    connection: &crate::database::ConnectionRef,
    rows: impl IntoIterator<Item = M>,
) -> Vec<M> {
    rows.into_iter()
        .map(|row| row.finish_load(connection))
        .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/internal_tests.rs"]
mod tests;
