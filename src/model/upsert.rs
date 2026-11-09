//! The statement behind `insert_or_update()` and `on_conflict(..).insert()`.

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, OnConflict, QueryFilter,
};
use crate::orm::QuerySelect;
use crate::orm::sql_builder::Expr;

use super::{Model, OnConflictBuilder};

/// Insert `model`, or update the stored row it collides with on `builder`'s
/// conflict columns, and return the row as stored. No callback runs, as none
/// does for any upsert. `managed_created_at` names the model's managed
/// `created_at` columns, which an update keeps unless it names them.
#[doc(hidden)]
pub async fn __upsert<M>(
    model: M,
    builder: OnConflictBuilder<M>,
    managed_created_at: &[&str],
) -> Result<M>
where
    M: Model,
{
    crate::validation::Validate::validate(&model).map_err(Error::from)?;
    let table = M::table_name();

    for column in &builder.conflict_columns {
        // Compared as the column it names, as the conflict target is.
        if super::__encrypted_column::<M>(column).is_some() {
            return Err(Error::query(format!(
                "encrypted field '{}' cannot be used as an insert_or_update conflict column for {}; encrypted fields use randomized ciphertext, so use a plaintext unique key instead",
                column, table
            )));
        }
    }
    // The column lists take field or column names; they are compared as
    // column names.
    let conflict_cols: Vec<String> = builder
        .conflict_columns
        .into_iter()
        .map(|column| M::canonical_column_name(&column).map_or(column, str::to_string))
        .collect();
    if conflict_cols.is_empty() {
        return Err(Error::query("upsert requires at least one conflict column"));
    }
    let is_key_column = |column: &str| M::primary_key_names().contains(&column);
    let auto_increment = M::primary_key_auto_increment();

    // A key the database has not numbered yet conflicts with no row: keyed by
    // it alone the upsert inserts the row, and otherwise the database numbers
    // it as an insert would, rather than storing its placeholder `0`.
    let key_is_new = auto_increment && model.is_new();
    let include_pk = !key_is_new
        && (conflict_cols.iter().any(|column| is_key_column(column)) || !auto_increment);

    let resolve = |column: &str, role: &str| {
        M::column_from_str(column)
            .ok_or_else(|| Error::query(format!("unknown {role} column '{column}' for {table}")))
    };
    let conflict_columns = conflict_cols
        .iter()
        .map(|column| resolve(column, "conflict"))
        .collect::<Result<Vec<_>>>()?;

    // The values the row is looked up by afterwards, before the write
    // encrypts any: a conflict column is never encrypted, and a key never is.
    let lookup = model.clone();
    let mut active = model.try_into_active_model()?;
    // The insert half stamps managed timestamps like `create()` does; a
    // conflict on the key needs the key in the insert as well, and any other
    // conflict leaves an auto-increment key to the database.
    if auto_increment {
        for key in M::primary_key_columns() {
            if include_pk {
                active.set(key, lookup.get(key));
            } else {
                active.not_set(key);
            }
        }
    }

    let update_cols = update_column_names::<M>(
        builder.update_columns,
        builder.exclude_columns,
        &conflict_cols,
        managed_created_at,
    )?;
    let update_columns = update_cols
        .iter()
        .map(|column| resolve(column, "update"))
        .collect::<Result<Vec<_>>>()?;

    // Validate the column lists even when a generated key makes this an insert.
    if key_is_new && conflict_cols.iter().all(|column| is_key_column(column)) {
        return super::__insert_row::<M>(active).await;
    }

    let error_context = move || {
        crate::internal::model_error_context::<M>(format!(
            "insert_or_update into {} on conflict ({})",
            table,
            conflict_cols.join(", ")
        ))
    };
    let connection = super::crud::writable_connection::<M>(&active)?;
    let executor = connection.executor();

    if matches!(
        executor.get_database_backend(),
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) {
        if update_cols.iter().any(|column| is_key_column(column)) {
            return Err(Error::query(
                "MySQL upsert update_columns() cannot update a primary key; omit primary-key columns from the update list",
            ));
        }
        return upsert_mysql::<M>(
            lookup,
            active,
            conflict_columns,
            update_columns,
            error_context,
        )
        .await;
    }

    // `ON CONFLICT` names its target, so a conflict on another unique key
    // fails the statement instead of updating a row the target does not name,
    // and `RETURNING` hands back the row the statement wrote, inserted or
    // updated. Whether a NULL conflict value conflicts is the constraint's to
    // say (`NULLS NOT DISTINCT`), so it goes to the database too. With nothing
    // to overwrite, the conflict columns are set to themselves: the stored
    // row is kept, and still returned.
    let mut on_conflict = OnConflict::columns(conflict_columns.iter().copied());
    if update_columns.is_empty() {
        for column in &conflict_columns {
            on_conflict.value(*column, Expr::col((M::Entity::default(), *column)));
        }
    } else {
        on_conflict.update_columns(update_columns);
    }
    let explicit_key = crate::internal::holds_auto_increment_key::<M>(&active);
    let row = connection
        .invalidating(crate::internal::run_statement(
            M::Entity::insert(active)
                .on_conflict(on_conflict)
                .exec_with_returning(&executor),
            error_context,
        ))
        .await;
    super::crud::stored_row::<M>(&connection, &executor, row, explicit_key).await
}

/// The columns an upsert overwrites on a conflict, as column names: those
/// `update_columns` names, or else every column but those `exclude_columns`
/// names, the key, the conflict columns and a managed `created_at`.
fn update_column_names<M: Model>(
    update_columns: Option<Vec<String>>,
    exclude_columns: Option<Vec<String>>,
    conflict_cols: &[String],
    managed_created_at: &[&str],
) -> Result<Vec<&'static str>> {
    let table = M::table_name();
    if let Some(columns) = update_columns {
        return columns
            .iter()
            .map(|column| {
                M::canonical_column_name(column).ok_or_else(|| {
                    Error::query(format!("unknown update column '{column}' for {table}"))
                })
            })
            .collect();
    }
    let exclude = exclude_columns
        .unwrap_or_default()
        .iter()
        .map(|name| {
            M::canonical_column_name(name).ok_or_else(|| {
                Error::query(format!(
                    "unknown column '{name}' in update_all_except() for {table}"
                ))
            })
        })
        .collect::<Result<Vec<&str>>>()?;
    // The key and the conflict columns are what matched the stored row, so
    // they stay as they are; so does a managed `created_at`, unless named.
    Ok(M::column_names()
        .iter()
        .copied()
        .filter(|column| {
            !exclude.contains(column)
                && !managed_created_at.contains(column)
                && !M::primary_key_names().contains(column)
                && !conflict_cols.iter().any(|conflict| conflict == column)
        })
        .collect())
}

/// The MySQL-family upsert. `ON DUPLICATE KEY UPDATE` fires on a conflict
/// with any unique key, not only the conflict columns, so it can update a row
/// they do not name. Instead, one transaction locks the row the conflict
/// columns name and updates it, or inserts the model when there is none, where
/// a conflict on another unique key fails as the duplicate it is and writes
/// nothing. A NULL conflict value matches no row: MySQL's unique keys never
/// treat NULLs as equal.
async fn upsert_mysql<M>(
    lookup: M,
    active: M::ActiveModel,
    conflict_columns: Vec<<M::Entity as EntityTrait>::Column>,
    update_columns: Vec<<M::Entity as EntityTrait>::Column>,
    error_context: impl Fn() -> crate::error::ErrorContext + Send + Sync + 'static,
) -> Result<M>
where
    M: Model,
{
    crate::database::__current_db()?
        .transaction(async move |_| {
            let connection = crate::database::__current_connection()?;
            validate_mysql_conflict_target::<M>(&connection, &conflict_columns).await?;
            let conflicts_on_null = conflict_columns.iter().any(|column| {
                let value = lookup.get(*column);
                value == value.as_null()
            });
            let existing = if conflicts_on_null {
                None
            } else {
                let finder = conflict_columns
                    .iter()
                    .fold(M::Entity::find(), |finder, column| {
                        finder.filter(column.eq(lookup.get(*column)))
                    })
                    .lock_exclusive();
                crate::internal::run_statement(finder.all(&connection.executor()), &error_context)
                    .await?
                    .into_iter()
                    .next()
            };

            let Some(row) = existing else {
                return super::__insert_row::<M>(active).await;
            };
            let stored = row.clone().finish_load(&connection);
            if update_columns.is_empty() {
                return Ok(stored);
            }
            let primary_key = stored.primary_key();
            let mut update = row.into_active_model();
            for column in update_columns {
                if let Some(value) = active.get(column).into_value() {
                    update.set(column, value);
                }
            }
            super::__update_row::<M>(update, &primary_key).await
        })
        .await
}

/// MySQL's emulation must have the same full unique-key target as ON
/// CONFLICT: a unique index whose key parts are the conflict columns, whole.
/// A target found valid is remembered per pool, since only DDL changes it.
async fn validate_mysql_conflict_target<M: Model>(
    connection: &crate::database::ConnectionRef,
    columns: &[<M::Entity as EntityTrait>::Column],
) -> Result<()> {
    use crate::orm::sql_builder::Iden;
    use std::collections::{BTreeSet, HashSet};
    type Target = (u64, Option<&'static str>, &'static str, BTreeSet<String>);
    // shortcut: a unique key dropped while the process runs goes unnoticed
    // until it restarts; key the entries by a schema version if that matters.
    static VALID: std::sync::LazyLock<parking_lot::Mutex<HashSet<Target>>> =
        std::sync::LazyLock::new(Default::default);

    let target: BTreeSet<String> = columns.iter().map(Iden::to_string).collect();
    let key = (
        connection.identity(),
        M::schema_name(),
        M::table_name(),
        target,
    );
    if VALID.lock().contains(&key) {
        return Ok(());
    }
    let executor = connection.executor();
    let indexes = crate::schema::catalog::table_indexes(
        &executor,
        executor.get_database_backend(),
        key.1,
        key.2,
    )
    .await?;
    let target = &key.3;
    let valid = target.len() == columns.len()
        && indexes.iter().any(|index| {
            index.unique
                && index.whole_columns
                && index.columns.len() == target.len()
                && index.columns.iter().all(|column| target.contains(column))
        });
    if !valid {
        return Err(Error::query(format!(
            "upsert conflict columns ({}) must match a complete unique index on {}",
            target.iter().cloned().collect::<Vec<_>>().join(", "),
            M::table_name()
        )));
    }
    VALID.lock().insert(key);
    Ok(())
}
