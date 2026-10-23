//! Moving a PostgreSQL key sequence past auto-increment keys a write supplied.

use super::{
    ActiveModelTrait, Backend, ConnectionTrait, Executor, InternalModel,
    build_statement_with_values, translate_error,
};
use crate::config::DatabaseType;
use crate::error::Result;

/// Moves the sequence of `$2`'s column `$3` to `$1` when the next value it
/// would hand out is `$1` or less, and never back. A role without `UPDATE` on
/// the sequence is left alone, since a failing `setval` would abort the
/// transaction the write runs in, and so is a descending sequence or one that
/// cannot reach `$1`. `CASE` keeps the privilege checks ahead of
/// `pg_sequence_last_value`, which needs `SELECT` or `USAGE`.
const ADVANCE: &str = "SELECT pg_catalog.setval(s.seq, \
         GREATEST($1, COALESCE(pg_catalog.pg_sequence_last_value(s.seq), $1))) \
     FROM (SELECT pg_catalog.pg_get_serial_sequence($2, $3)::regclass AS seq) AS s \
     JOIN pg_catalog.pg_sequence AS p ON p.seqrelid = s.seq \
     WHERE CASE WHEN pg_catalog.has_sequence_privilege(s.seq, 'UPDATE') \
                 AND pg_catalog.has_sequence_privilege(s.seq, 'SELECT, USAGE') \
           THEN p.seqincrement > 0 AND $1 <= p.seqmax AND $1 >= COALESCE( \
                pg_catalog.pg_sequence_last_value(s.seq) + p.seqincrement, p.seqstart) \
           ELSE false END";

/// Whether `active` is inserted with an auto-increment key the model holds,
/// not one the database numbers.
pub(crate) fn holds_auto_increment_key<M: crate::model::Model>(active: &M::ActiveModel) -> bool {
    M::primary_key_auto_increment()
        && <M as InternalModel>::primary_key_columns()
            .iter()
            .any(|column| active.get(*column).is_set())
}

/// After rows of `M` were written with auto-increment keys they held rather
/// than ones the database numbered, `keys`, move the key's PostgreSQL sequence
/// past the largest. MySQL's `AUTO_INCREMENT` and SQLite's rowid move past an
/// explicit key by themselves; a sequence does not, so the next key it
/// numbered would collide with one of the rows.
pub(crate) async fn advance_key_sequence<M: crate::model::Model>(
    executor: &Executor<'_>,
    keys: impl IntoIterator<Item = M::PrimaryKey>,
) -> Result<()> {
    let [column] = M::primary_key_names() else {
        return Ok(());
    };
    if !M::primary_key_auto_increment()
        || Backend::from(executor.get_database_backend()) != Backend::Postgres
    {
        return Ok(());
    }
    let Some(largest) = keys
        .into_iter()
        .filter_map(|key| serde_json::to_value(&key).ok()?.as_i64())
        .max()
    else {
        return Ok(());
    };
    let table = crate::query::db_sql::quote_table::<M>(DatabaseType::Postgres);
    let statement = build_statement_with_values(
        Backend::Postgres,
        ADVANCE,
        vec![largest.into(), table.into(), (*column).into()],
    );
    executor
        .query_all_raw(statement)
        .await
        .map_err(translate_error)?;
    Ok(())
}
