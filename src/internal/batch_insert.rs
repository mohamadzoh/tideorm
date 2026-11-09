//! `insert_all`: the batch strategy each backend can run, and putting the rows
//! a multi-row insert returns back in the caller's order.

use crate::config::DatabaseType;
use std::collections::{HashMap, HashSet};
use std::iter::Peekable;

use super::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait, InternalModel,
    Iterable, QueryFilter, Value, advance_key_sequence, holds_auto_increment_key,
    model_error_context, models_read_through, run_statement, translate_error,
};
use crate::error::{Error, ErrorContext, Result};

/// The most bytes of values, as estimated, one MySQL or MariaDB insert
/// carries; a larger single row is sent alone. The server refuses a statement
/// longer than `max_allowed_packet` (4 MB by default on MySQL 5.7, 16 MB on
/// MariaDB), so this stays well under it, SQL and protocol overhead included.
const MYSQL_MAX_STATEMENT_BYTES: usize = 1 << 20;

/// How a batch is written.
enum Strategy {
    /// Multi-row `INSERT .. RETURNING`, the returned rows put in input order.
    Returning(Order),
    /// Multi-row `INSERT`, then one `SELECT` of the rows by their keys. MySQL
    /// returns no rows, but a key the model sets itself is known up front.
    InsertThenSelect(Vec<ClientKey>),
    /// One `INSERT` per row.
    PerRow,
}

/// How rows returned by a multi-row insert are put in input order.
enum Order {
    /// PostgreSQL and MariaDB return them in input order.
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
/// PostgreSQL and MariaDB send multi-row `INSERT .. RETURNING` statements,
/// and so does SQLite, which returns rows in no particular order: they are
/// matched back to their models by key. MySQL has no `RETURNING`, so there a
/// model that sets its own integer or `Uuid` key is inserted with multi-row
/// statements, each read back with a `SELECT`, and any other model is
/// inserted a row at a time. A batch needing more bind parameters than one
/// statement takes is split, and so is a MySQL or MariaDB batch of large
/// rows. Every batch of several rows runs in a transaction, a
/// savepoint inside the caller's, so it stays all-or-nothing on every backend,
/// down to a statement whose returned rows do not match the batch.
pub(super) async fn insert_many<M>(
    connection: &crate::database::ConnectionRef,
    models: Vec<M>,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
{
    let executor = connection.executor();
    let conn = &executor;
    if models.is_empty() {
        return Ok(Vec::new());
    }

    let batch_size = models.len();
    let error_context = model_error_context::<M>(format!("insert_many(batch_size={batch_size})"));
    let active_models = models
        .into_iter()
        .map(M::try_into_active_model)
        .collect::<Result<Vec<_>>>()?;
    for active in &active_models {
        super::ensure_active_values_storable::<M>(active, conn)?;
    }
    let held_keys = held_keys::<M>(&active_models);
    let explicit_keys = held_keys > 0;
    if batch_size == 1 {
        let results = insert_individually::<M>(connection, active_models, &error_context).await?;
        if explicit_keys {
            advance_key_sequence::<M>(conn, results.iter().map(|model| model.primary_key()))
                .await?;
        }
        return Ok(results);
    }

    let backend = conn.get_database_backend();
    let strategy = strategy::<M>(backend, &active_models, held_keys);
    let rows_per_statement = rows_per_statement(backend, M::column_names().len())?;

    // One statement of several rows runs in a transaction too, since what it
    // returns is checked after the rows are in: a trigger can leave one out.
    // `begin()` on an open transaction opens a SAVEPOINT, so this also nests
    // inside a caller's transaction.
    let scope = crate::database::TransactionScope::begin(connection, Default::default())
        .await
        .map_err(translate_error)
        .map_err(|err| err.with_context(error_context.clone()))?;
    let record = scope.record().clone();
    super::per_poll(
        async {
            // Loads must belong to this batch's transaction as well as its
            // statements. Using the caller's connection would put baselines
            // outside the batch's rollback record, or in its parent's record.
            let batch_connection = scope.connection();
            let inserted = insert_by_strategy::<M>(
                &batch_connection,
                &strategy,
                active_models,
                Chunks {
                    rows: rows_per_statement,
                    limit_bytes: matches!(backend, DatabaseType::MySQL | DatabaseType::MariaDB),
                },
                &error_context,
            )
            .await;
            // Of the keys stored, the largest numbered by the database is
            // below the sequence already, so the largest of all is the one
            // to move it past.
            let inserted = match inserted {
                Ok(results) if explicit_keys => advance_key_sequence::<M>(
                    &batch_connection.executor(),
                    results.iter().map(|model| model.primary_key()),
                )
                .await
                .map(|()| results),
                inserted => inserted,
            };

            drop(batch_connection);
            scope
                .end(inserted, |err| {
                    translate_error(err).with_context(error_context.clone())
                })
                .await
        },
        || crate::cache::install_pending_invalidations(&record),
    )
    .await
}

fn rows_per_statement(backend: DatabaseType, columns: usize) -> Result<usize> {
    // Fails only for a backend whose driver is not compiled in.
    let parameter_limit = backend.max_bind_parameters().map_err(translate_error)?;
    Ok((parameter_limit / columns.max(1)).max(1))
}

/// How many rows one statement of a batch carries.
#[derive(Clone, Copy)]
struct Chunks {
    /// At most this many rows, as the bind parameter limit allows.
    rows: usize,
    /// On MySQL and MariaDB, also at most [`MYSQL_MAX_STATEMENT_BYTES`] of
    /// values unless the first row alone is larger.
    limit_bytes: bool,
}

/// Insert `active_models` as `strategy` says, as many in one statement as
/// `chunks` allows, and return them as stored, in input order.
async fn insert_by_strategy<M>(
    connection: &crate::database::ConnectionRef,
    strategy: &Strategy,
    active_models: Vec<M::ActiveModel>,
    chunks: Chunks,
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
{
    let count = active_models.len();
    match strategy {
        Strategy::Returning(order) => {
            let mut results = Vec::with_capacity(count);
            let mut remaining = active_models.into_iter().peekable();
            loop {
                let chunk = next_chunk::<M, _>(&mut remaining, chunks);
                if chunk.is_empty() {
                    return Ok(results);
                }
                let offset = results.len();
                results.extend(
                    insert_returning::<M>(connection, chunk, order, offset, error_context).await?,
                );
            }
        }
        Strategy::InsertThenSelect(keys) => {
            let mut results = Vec::with_capacity(count);
            let mut remaining = active_models.into_iter().peekable();
            loop {
                let chunk = next_chunk::<M, _>(&mut remaining, chunks);
                if chunk.is_empty() {
                    return Ok(results);
                }
                let chunk_keys = &keys[results.len()..results.len() + chunk.len()];
                results.extend(
                    insert_then_select::<M>(connection, chunk, chunk_keys, error_context).await?,
                );
            }
        }
        Strategy::PerRow => {
            insert_individually::<M>(connection, active_models, error_context).await
        }
    }
}

/// How many of `active_models` hold their auto-increment key.
fn held_keys<M: crate::model::Model>(active_models: &[M::ActiveModel]) -> usize {
    active_models
        .iter()
        .filter(|active| holds_auto_increment_key::<M>(active))
        .count()
}

/// The batch strategy `backend` can run for `M`, `held` of whose models hold
/// their auto-increment key.
fn strategy<M>(backend: DatabaseType, active_models: &[M::ActiveModel], held: usize) -> Strategy
where
    M: InternalModel + crate::model::Model,
{
    let auto_increment = M::primary_key_auto_increment();
    // A model holding its own key under an auto-increment column has it
    // written; rows the database numbers beside it would neither share one
    // statement's columns nor follow the keys' order. A batch where every
    // model holds its key is matched back by key, as a client-set one is.
    if held != 0 && held != active_models.len() {
        return Strategy::PerRow;
    }
    let numbered = auto_increment && held == 0;
    if matches!(backend, DatabaseType::Postgres | DatabaseType::MariaDB) {
        return Strategy::Returning(Order::AsReturned);
    }
    if backend == DatabaseType::SQLite && numbered && M::primary_key_names().len() == 1 {
        return Strategy::Returning(Order::ByIncreasingKey);
    }
    if numbered {
        return Strategy::PerRow;
    }
    match (backend, client_keys::<M>(active_models, backend)) {
        (DatabaseType::SQLite, Some(keys)) => Strategy::Returning(Order::ByClientKey(keys)),
        (DatabaseType::MySQL, Some(keys)) => Strategy::InsertThenSelect(keys),
        _ => Strategy::PerRow,
    }
}

/// The key each model sets itself, or `None` when one is unset, of a type that
/// might not come back from the database exactly as it was sent, or shared by
/// two models, whose rows could not be told apart; the database's own
/// constraint then decides the batch row by row.
fn client_keys<M: InternalModel>(
    active_models: &[M::ActiveModel],
    backend: DatabaseType,
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
fn returns_exactly(value: &Value, backend: DatabaseType) -> bool {
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
        Value::String(Some(_)) => backend == DatabaseType::SQLite,
        _ => false,
    }
}

/// The text of `row`'s key, as [`ClientKey::text`] renders a sent one.
fn stored_key<M: InternalModel>(row: &M) -> String {
    let values: Vec<Value> = M::primary_key_columns()
        .into_iter()
        .map(|column| row.get(column))
        .collect();
    format!("{values:?}")
}

/// Put `rows` in the order of `keys`, the keys of the models that were
/// inserted, failing if a row is missing or was not among them.
fn order_by_keys<M: InternalModel>(rows: Vec<M>, keys: &[ClientKey]) -> Result<Vec<M>> {
    let mut positions: HashMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(position, key)| (key.text.as_str(), position))
        .collect();
    let mut ordered: Vec<Option<M>> = std::iter::repeat_with(|| None).take(keys.len()).collect();
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
async fn insert_returning<M>(
    connection: &crate::database::ConnectionRef,
    active_models: Vec<M::ActiveModel>,
    order: &Order,
    offset: usize,
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
{
    let conn = connection.executor();
    let count = active_models.len();
    let insert = M::Entity::insert_many(active_models).exec_with_returning(&conn);
    let rows = run_statement(insert, || error_context.clone()).await?;
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
            let mut models = models_read_through::<M>(connection, rows);
            models.sort_by_cached_key(|model| {
                serde_json::to_value(model.primary_key())
                    .ok()
                    .and_then(|key| key.as_i64())
            });
            return Ok(models);
        }
    };
    Ok(models_read_through(connection, rows))
}

/// The next rows of `remaining` for one insert, as many as `chunks` allows.
fn next_chunk<M, I>(remaining: &mut Peekable<I>, chunks: Chunks) -> Vec<M::ActiveModel>
where
    M: InternalModel,
    I: Iterator<Item = M::ActiveModel>,
{
    let mut chunk = Vec::new();
    let mut bytes = 0;
    while chunk.len() < chunks.rows {
        let Some(next) = remaining.peek() else {
            break;
        };
        if chunks.limit_bytes {
            let size = row_bytes::<M>(next);
            if !chunk.is_empty() && bytes + size > MYSQL_MAX_STATEMENT_BYTES {
                break;
            }
            bytes += size;
        }
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
async fn insert_then_select<M>(
    connection: &crate::database::ConnectionRef,
    active_models: Vec<M::ActiveModel>,
    keys: &[ClientKey],
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
{
    let conn = connection.executor();
    let insert = M::Entity::insert_many(active_models).exec_without_returning(&conn);
    run_statement(insert, || error_context.clone()).await?;

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
    let select = M::Entity::find().filter(condition).all(&conn);
    let rows = run_statement(select, || error_context.clone()).await?;

    Ok(models_read_through(
        connection,
        order_by_keys::<M>(rows, keys)?,
    ))
}

/// Insert models one row at a time, preserving the caller's ordering.
///
/// The caller is responsible for providing a transactional connection so the
/// whole batch stays all-or-nothing.
async fn insert_individually<M>(
    connection: &crate::database::ConnectionRef,
    active_models: Vec<M::ActiveModel>,
    error_context: &ErrorContext,
) -> Result<Vec<M>>
where
    M: InternalModel + crate::model::Model,
{
    let conn = connection.executor();
    let mut results = Vec::with_capacity(active_models.len());
    for active in active_models {
        let result = run_statement(active.insert(&conn), || error_context.clone()).await?;
        results.push(result.finish_load(connection));
    }
    Ok(results)
}

#[cfg(test)]
#[path = "../../tests/unit/internal_batch_insert_tests.rs"]
mod tests;
