//! `insert_all`: the batch strategy each backend can run, and putting the rows
//! a multi-row insert returns back in the caller's order.

use std::collections::{HashMap, HashSet};
use std::iter::Peekable;

use super::{
    ActiveModelTrait, Backend, ColumnTrait, Condition, ConnectionTrait, EntityTrait, Executor,
    InternalModel, IntoActiveModel, Iterable, ModelTrait, QueryFilter, Value,
    ensure_fields_storable, model_error_context, translate_error,
};
use crate::error::{Error, ErrorContext, Result};

type EntityModel<M> = <<M as InternalModel>::Entity as EntityTrait>::Model;

/// The most bind parameters one SQLite statement takes
/// (`SQLITE_MAX_VARIABLE_NUMBER` in the bundled build).
const SQLITE_MAX_BIND_PARAMETERS: usize = 32_766;

/// The most bytes of values one MySQL or MariaDB insert carries, unless a
/// single row is larger. The server refuses a statement longer than
/// `max_allowed_packet`, 4 MB by default on MySQL 5.7 and 16 MB on MariaDB.
const MYSQL_MAX_STATEMENT_BYTES: usize = 1 << 20;

/// How a batch is written.
enum Strategy {
    /// Multi-row `INSERT .. RETURNING`, the returned rows put in input order.
    Returning(Order),
    /// Multi-row `INSERT`, then one `SELECT` of the rows by their keys. MySQL
    /// and MariaDB return no rows, but a key the model sets itself is known up
    /// front.
    InsertThenSelect(Vec<ClientKey>),
    /// One `INSERT` per row.
    PerRow,
}

/// How rows returned by a multi-row insert are put in input order.
enum Order {
    /// PostgreSQL returns them in input order.
    AsReturned,
    /// SQLite's order is arbitrary, but it assigns an auto-increment key in
    /// the order it inserts.
    ByIncreasingKey,
    /// Each row is matched to the model that set its key.
    ByClientKey(Vec<ClientKey>),
}

/// The key a model sets itself: its column values, and their text to match a
/// row read back against.
struct ClientKey {
    text: String,
    values: Vec<Value>,
}

/// Insert `models` and return them as stored, in the order they were passed.
///
/// PostgreSQL sends multi-row `INSERT .. RETURNING` statements, and so does
/// SQLite, which returns rows in no particular order: they are matched back to
/// their models by key. The engine renders no `RETURNING` for MySQL or
/// MariaDB, so there a model that sets its own integer or `Uuid` key is
/// inserted with multi-row statements, each read back with a `SELECT`, and
/// any other model is inserted a row at a time. A batch needing more bind
/// parameters than one statement takes is split, and so is a MySQL or MariaDB
/// batch of large rows. Every multi-statement batch runs in a transaction, a
/// savepoint inside the caller's, so it stays all-or-nothing on every backend.
pub(super) async fn insert_many<M>(conn: &Executor<'_>, models: Vec<M>) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
    EntityModel<M>: IntoActiveModel<M::ActiveModel>,
{
    if models.is_empty() {
        return Ok(Vec::new());
    }
    ensure_fields_storable::<M, _>(conn)?;

    let batch_size = models.len();
    let error_context = model_error_context::<M>(format!("insert_many(batch_size={batch_size})"));
    let active_models = models
        .into_iter()
        .map(M::try_into_active_model)
        .collect::<Result<Vec<_>>>()?;
    if batch_size == 1 {
        return insert_individually::<M, _>(conn, active_models, &error_context).await;
    }

    let backend = Backend::from(conn.get_database_backend());
    let strategy = strategy::<M>(backend, &active_models);
    // One statement carries at most `u16::MAX` bind parameters on PostgreSQL
    // and MySQL, and 32,766 on SQLite, so a larger batch is split.
    let parameter_limit = match backend {
        Backend::Sqlite => SQLITE_MAX_BIND_PARAMETERS,
        Backend::Postgres | Backend::MySql => usize::from(u16::MAX),
    };
    let rows_per_statement = (parameter_limit / M::column_names().len().max(1)).max(1);

    if let Strategy::Returning(order) = &strategy
        && batch_size <= rows_per_statement
    {
        return insert_returning::<M, _>(conn, active_models, order, 0, &error_context).await;
    }

    // Several statements from here on. `begin()` on an open transaction opens
    // a SAVEPOINT, so this also nests inside a caller's transaction.
    let txn = conn
        .begin()
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context.clone()))?;
    // The rows go through an executor, which binds their values as it binds
    // every other statement's.
    let in_txn = Executor::new((&txn).into());
    let inserted = match &strategy {
        Strategy::Returning(order) => {
            let mut results = Vec::with_capacity(batch_size);
            let mut remaining = active_models.into_iter();
            loop {
                let chunk: Vec<_> = remaining.by_ref().take(rows_per_statement).collect();
                if chunk.is_empty() {
                    break Ok(results);
                }
                let offset = results.len();
                match insert_returning::<M, _>(&in_txn, chunk, order, offset, &error_context).await
                {
                    Ok(rows) => results.extend(rows),
                    Err(err) => break Err(err),
                }
            }
        }
        Strategy::InsertThenSelect(keys) => {
            let mut results = Vec::with_capacity(batch_size);
            let mut remaining = active_models.into_iter().peekable();
            loop {
                let chunk = mysql_chunk::<M, _>(&mut remaining, rows_per_statement);
                if chunk.is_empty() {
                    break Ok(results);
                }
                let chunk_keys = &keys[results.len()..results.len() + chunk.len()];
                match insert_then_select::<M, _>(&in_txn, chunk, chunk_keys, &error_context).await {
                    Ok(rows) => results.extend(rows),
                    Err(err) => break Err(err),
                }
            }
        }
        Strategy::PerRow => {
            insert_individually::<M, _>(&in_txn, active_models, &error_context).await
        }
    };

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

/// The batch strategy `backend` can run for `M`.
fn strategy<M>(backend: Backend, active_models: &[M::ActiveModel]) -> Strategy
where
    M: InternalModel + crate::model::Model,
{
    // MariaDB has `RETURNING` too, but the engine renders none for the MySQL
    // family, which it reports as one backend, so MariaDB batches like MySQL.
    if backend == Backend::Postgres {
        return Strategy::Returning(Order::AsReturned);
    }
    let auto_increment = M::primary_key_auto_increment();
    if backend == Backend::Sqlite && auto_increment && M::primary_key_names().len() == 1 {
        return Strategy::Returning(Order::ByIncreasingKey);
    }
    if auto_increment {
        return Strategy::PerRow;
    }
    match (backend, client_keys::<M>(active_models, backend)) {
        (Backend::Sqlite, Some(keys)) => Strategy::Returning(Order::ByClientKey(keys)),
        (Backend::MySql, Some(keys)) => Strategy::InsertThenSelect(keys),
        _ => Strategy::PerRow,
    }
}

/// The key each model sets itself, or `None` when one is unset, of a type that
/// might not come back from the database exactly as it was sent, or shared by
/// two models, whose rows could not be told apart; the database's own
/// constraint then decides the batch row by row.
fn client_keys<M: InternalModel>(
    active_models: &[M::ActiveModel],
    backend: Backend,
) -> Option<Vec<ClientKey>> {
    let columns = M::primary_key_columns();
    let keys = active_models
        .iter()
        .map(|active| {
            let values = columns
                .iter()
                .map(|column| active.get(*column).into_value())
                .collect::<Option<Vec<Value>>>()?;
            values
                .iter()
                .all(|value| returns_exactly(value, backend))
                .then(|| ClientKey {
                    text: format!("{values:?}"),
                    values,
                })
        })
        .collect::<Option<Vec<_>>>()?;
    let mut seen = HashSet::with_capacity(keys.len());
    keys.iter()
        .all(|key| seen.insert(key.text.as_str()))
        .then_some(keys)
}

/// Whether a key value comes back from `backend` exactly as it was sent:
/// integers and UUIDs everywhere, and text on SQLite, which keeps it as
/// written, while a MySQL `CHAR` key comes back without trailing spaces.
fn returns_exactly(value: &Value, backend: Backend) -> bool {
    match value {
        Value::TinyInt(Some(_))
        | Value::SmallInt(Some(_))
        | Value::Int(Some(_))
        | Value::BigInt(Some(_))
        | Value::TinyUnsigned(Some(_))
        | Value::SmallUnsigned(Some(_))
        | Value::Unsigned(Some(_))
        | Value::BigUnsigned(Some(_))
        | Value::Uuid(Some(_)) => true,
        Value::String(Some(_)) => backend == Backend::Sqlite,
        _ => false,
    }
}

/// The text of `row`'s key, as [`ClientKey::text`] renders a sent one.
fn stored_key<M: InternalModel>(row: &EntityModel<M>) -> String {
    let values: Vec<Value> = M::primary_key_columns()
        .into_iter()
        .map(|column| row.get(column))
        .collect();
    format!("{values:?}")
}

/// Put `rows` in the order of `keys`, the keys of the models that were
/// inserted, failing if a row is missing or was not among them.
fn order_by_keys<M: InternalModel>(
    rows: Vec<EntityModel<M>>,
    keys: &[ClientKey],
) -> Result<Vec<EntityModel<M>>> {
    let mut positions: HashMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(position, key)| (key.text.as_str(), position))
        .collect();
    let mut ordered: Vec<Option<EntityModel<M>>> =
        std::iter::repeat_with(|| None).take(keys.len()).collect();
    for row in rows {
        let key = stored_key::<M>(&row);
        let position = positions.remove(key.as_str()).ok_or_else(|| {
            Error::query(format!(
                "insert_all read back a row under a key it did not insert: {key}"
            ))
        })?;
        ordered[position] = Some(row);
    }
    ordered
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| Error::query("insert_all could not read back every row it inserted"))
}

/// Insert `active_models`, the batch's rows from `offset`, with one
/// `INSERT .. RETURNING`, and return them in input order.
async fn insert_returning<M, C>(
    conn: &C,
    active_models: Vec<M::ActiveModel>,
    order: &Order,
    offset: usize,
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
    EntityModel<M>: IntoActiveModel<M::ActiveModel>,
    C: ConnectionTrait,
{
    let count = active_models.len();
    let insert = M::Entity::insert_many(active_models).exec_with_returning(conn);
    let rows = crate::profiling::__profile_future(insert)
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context.clone()))?;
    // A trigger can skip rows (PostgreSQL routing one to another table,
    // SQLite's `RAISE(IGNORE)`), which would leave the rest out of line with
    // the models they came from.
    if rows.len() != count {
        return Err(Error::query(format!(
            "insert_all sent {count} rows and the database returned {}",
            rows.len()
        ))
        .with_context(error_context.clone()));
    }

    let rows = match order {
        Order::AsReturned => rows,
        Order::ByClientKey(keys) => order_by_keys::<M>(rows, &keys[offset..offset + count])?,
        Order::ByIncreasingKey => {
            let mut models = rows
                .into_iter()
                .map(M::try_from_entity_model)
                .collect::<Result<Vec<_>>>()?;
            models.sort_by_cached_key(|model| {
                serde_json::to_value(model.primary_key())
                    .ok()
                    .and_then(|key| key.as_i64())
            });
            return Ok(models);
        }
    };
    rows.into_iter().map(M::try_from_entity_model).collect()
}

/// The next rows of `remaining` for one MySQL or MariaDB insert: at most
/// `max_rows`, and at most [`MYSQL_MAX_STATEMENT_BYTES`] of values unless the
/// first row alone is larger.
fn mysql_chunk<M, I>(remaining: &mut Peekable<I>, max_rows: usize) -> Vec<M::ActiveModel>
where
    M: InternalModel,
    I: Iterator<Item = M::ActiveModel>,
{
    let mut chunk = Vec::new();
    let mut bytes = 0;
    while chunk.len() < max_rows {
        let Some(next) = remaining.peek() else {
            break;
        };
        let size = row_bytes::<M>(next);
        if !chunk.is_empty() && bytes + size > MYSQL_MAX_STATEMENT_BYTES {
            break;
        }
        bytes += size;
        chunk.extend(remaining.next());
    }
    chunk
}

/// Roughly how many bytes `active`'s values take in a statement.
fn row_bytes<M: InternalModel>(active: &M::ActiveModel) -> usize {
    <M::Entity as EntityTrait>::Column::iter()
        .filter_map(|column| active.get(column).into_value())
        .map(|value| match value {
            Value::String(Some(text)) => text.len(),
            Value::Bytes(Some(bytes)) => bytes.len(),
            Value::Json(Some(json)) => json.to_string().len(),
            _ => 8,
        })
        .sum()
}

/// Insert `active_models` with one `INSERT`, then read them back by `keys`
/// with one `SELECT`, in input order.
async fn insert_then_select<M, C>(
    conn: &C,
    active_models: Vec<M::ActiveModel>,
    keys: &[ClientKey],
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
    C: ConnectionTrait,
{
    let insert = M::Entity::insert_many(active_models).exec_without_returning(conn);
    crate::profiling::__profile_future(insert)
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context.clone()))?;

    let columns = M::primary_key_columns();
    let condition = match columns.as_slice() {
        [column] => Condition::all()
            .add(column.is_in(keys.iter().filter_map(|key| key.values.first().cloned()))),
        _ => keys.iter().fold(Condition::any(), |any, key| {
            let matches = columns
                .iter()
                .zip(&key.values)
                .fold(Condition::all(), |all, (column, value)| {
                    all.add(column.eq(value.clone()))
                });
            any.add(matches)
        }),
    };
    let select = M::Entity::find().filter(condition).all(conn);
    let rows = crate::profiling::__profile_future(select)
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context.clone()))?;

    order_by_keys::<M>(rows, keys)?
        .into_iter()
        .map(M::try_from_entity_model)
        .collect()
}

/// Insert models one row at a time, preserving the caller's ordering.
///
/// The caller is responsible for providing a transactional connection so the
/// whole batch stays all-or-nothing.
async fn insert_individually<M, C>(
    conn: &C,
    active_models: Vec<M::ActiveModel>,
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
    EntityModel<M>: IntoActiveModel<M::ActiveModel>,
    C: ConnectionTrait,
{
    let mut results = Vec::with_capacity(active_models.len());
    for active in active_models {
        let result = crate::profiling::__profile_future(async move { active.insert(conn).await })
            .await
            .map_err(translate_error)
            .map_err(|err| err.with_context(error_context.clone()))?;
        results.push(M::try_from_entity_model(result)?);
    }
    Ok(results)
}

#[cfg(test)]
#[path = "../../tests/unit/internal_batch_insert_tests.rs"]
mod tests;
