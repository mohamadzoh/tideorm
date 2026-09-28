//! The statement behind `insert_or_update()` and `on_conflict(..).insert()`.

use crate::error::{Error, Result};
use crate::internal::{
    ActiveModelTrait, Backend, ColumnTrait, ConnectionTrait, EntityTrait, InternalModel,
    IntoActiveModel, ModelTrait, OnConflict, QueryFilter,
};
use crate::orm::QuerySelect;
use crate::orm::sea_query::Expr;

use super::{Model, OnConflictBuilder};

type EntityModel<M> = <<M as InternalModel>::Entity as EntityTrait>::Model;

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
    EntityModel<M>: IntoActiveModel<M::ActiveModel>,
{
    crate::validation::Validate::validate(&model).map_err(Error::from)?;
    let table = M::table_name();

    let encrypted = super::encrypted_field_columns::<M>()?;
    for column in &builder.conflict_columns {
        if encrypted
            .iter()
            .any(|(field, encrypted_column)| column == field || column == encrypted_column)
        {
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
    let is_key_column = |column: &str| M::primary_key_names().contains(&column);
    let auto_increment = M::primary_key_auto_increment();

    // A key the database has not numbered yet conflicts with no row: keyed by
    // it alone the upsert inserts the row, and otherwise the database numbers
    // it as an insert would, rather than storing its placeholder `0`.
    let key_is_new = auto_increment && model.is_new();
    if key_is_new && conflict_cols.iter().all(|column| is_key_column(column)) {
        return super::__insert_row::<M>(model.try_into_active_model()?).await;
    }
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

    // The values the row is looked up by afterwards. The plaintext conversion
    // is safe here: a conflict column is never encrypted, and a key never is.
    let lookup = model.to_entity_model();
    let mut active = model.try_into_active_model()?;
    // The insert half stamps managed timestamps like `create()` does; a
    // conflict on the key needs the key in the insert as well.
    if include_pk && auto_increment {
        for key in M::primary_key_columns() {
            active.set(key, lookup.get(key));
        }
    }

    let update_cols: Vec<&str> = match (builder.update_columns, builder.exclude_columns) {
        (Some(columns), _) => columns
            .iter()
            .map(|column| {
                M::canonical_column_name(column).ok_or_else(|| {
                    Error::query(format!("unknown update column '{column}' for {table}"))
                })
            })
            .collect::<Result<_>>()?,
        (None, exclude) => {
            let exclude = exclude
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
            // The key and the conflict columns are what matched the stored
            // row, so they stay as they are; so does a managed `created_at`,
            // unless named.
            M::column_names()
                .iter()
                .copied()
                .filter(|column| {
                    !exclude.contains(column)
                        && !managed_created_at.contains(column)
                        && !is_key_column(column)
                        && !conflict_cols.iter().any(|conflict| conflict == column)
                })
                .collect()
        }
    };
    let update_columns = update_cols
        .iter()
        .map(|column| resolve(column, "update"))
        .collect::<Result<Vec<_>>>()?;

    let error_context = move || {
        crate::internal::model_error_context::<M>(format!(
            "insert_or_update into {} on conflict ({})",
            table,
            conflict_cols.join(", ")
        ))
    };
    let connection = crate::database::__current_connection()?;
    let executor = connection.executor();
    crate::internal::ensure_fields_storable::<M, _>(&executor)?;

    if Backend::from(executor.get_database_backend()) == Backend::MySql {
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
    let row = crate::internal::run_profiled(
        M::Entity::insert(active)
            .on_conflict(on_conflict)
            .exec_with_returning(&executor),
        error_context,
    )
    .await?;
    crate::QueryCache::global().invalidate_model(table);
    M::try_from_entity_model(row)
}

/// The MySQL-family upsert. `ON DUPLICATE KEY UPDATE` fires on a conflict
/// with any unique key, not only the conflict columns, so it can update a row
/// they do not name. Instead, one transaction locks the row the conflict
/// columns name and updates it, or inserts the model when there is none, where
/// a conflict on another unique key fails as the duplicate it is and writes
/// nothing. A NULL conflict value matches no row: MySQL's unique keys never
/// treat NULLs as equal.
async fn upsert_mysql<M>(
    lookup: EntityModel<M>,
    active: M::ActiveModel,
    conflict_columns: Vec<<M::Entity as EntityTrait>::Column>,
    update_columns: Vec<<M::Entity as EntityTrait>::Column>,
    error_context: impl Fn() -> crate::error::ErrorContext + Send + Sync + 'static,
) -> Result<M>
where
    M: Model,
    EntityModel<M>: IntoActiveModel<M::ActiveModel>,
{
    crate::database::__current_db()?
        .transaction(move |_| {
            Box::pin(async move {
                let conflicts_on_null = conflict_columns.iter().any(|column| {
                    let value = lookup.get(*column);
                    value == value.as_null()
                });
                let existing = if conflicts_on_null {
                    None
                } else {
                    let connection = crate::database::__current_connection()?;
                    let finder = conflict_columns
                        .iter()
                        .fold(M::Entity::find(), |finder, column| {
                            finder.filter(column.eq(lookup.get(*column)))
                        })
                        .lock_exclusive();
                    crate::internal::run_profiled(
                        finder.all(&connection.executor()),
                        &error_context,
                    )
                    .await?
                    .into_iter()
                    .next()
                };

                let Some(row) = existing else {
                    return super::__insert_row::<M>(active).await;
                };
                if update_columns.is_empty() {
                    return M::try_from_entity_model(row);
                }
                let primary_key = M::try_from_entity_model(row.clone())?.primary_key();
                let mut update = row.into_active_model();
                for column in update_columns {
                    if let Some(value) = active.get(column).into_value() {
                        update.set(column, value);
                    }
                }
                super::__update_row::<M>(update, &primary_key).await
            })
        })
        .await
}
