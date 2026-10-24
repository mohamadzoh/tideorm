#![allow(missing_docs)]

use std::future::Future;

use crate::error::{Error, Result};

use super::Model;

pub(crate) async fn insert_all<M>(models: Vec<M>) -> Result<Vec<M>>
where
    M: Model,
    <<M as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        crate::internal::IntoActiveModel<<M as crate::internal::InternalModel>::ActiveModel>,
{
    if models.is_empty() {
        return Ok(Vec::new());
    }

    for (index, model) in models.iter().enumerate() {
        if let Err(errors) = model.validate() {
            return Err(match Error::from(errors) {
                Error::Validation { field, message } => Error::validation(
                    field,
                    format!("{message} (the batch's model at index {index})"),
                ),
                other => other,
            });
        }
    }

    crate::internal::QueryExecutor::insert_many::<M>(models).await
}

type EntityModel<M> =
    <<M as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model;

/// The connection a write of `active` runs on, once every value it writes is
/// one the driver can read back.
pub(super) fn writable_connection<M: Model>(
    active: &M::ActiveModel,
) -> Result<crate::database::ConnectionRef> {
    let connection = crate::database::__current_connection()?;
    let executor = connection.executor();
    crate::internal::ensure_fields_storable::<M, _>(&executor)?;
    crate::internal::ensure_active_values_storable::<M>(active, &executor)?;
    Ok(connection)
}

/// The model a write returned `row` for, read back through `connection`; a
/// write that supplied an auto-increment key moves the key's sequence past it.
pub(super) async fn stored_row<M: Model>(
    connection: &crate::database::ConnectionRef,
    executor: &crate::internal::Executor<'_>,
    row: Result<EntityModel<M>>,
    explicit_key: bool,
) -> Result<M> {
    let model = super::__loading_through(connection, || M::try_from_entity_model(row?))?;
    if explicit_key {
        crate::internal::advance_key_sequence::<M>(executor, [model.primary_key()]).await?;
    }
    Ok(model)
}

/// Insert `active` as a new row of `M` and return the row as stored, with no
/// callback: `create()` runs its own around it, and an upsert runs none.
#[doc(hidden)]
pub async fn __insert_row<M>(active: M::ActiveModel) -> Result<M>
where
    M: Model,
    EntityModel<M>: crate::internal::IntoActiveModel<M::ActiveModel>,
{
    use crate::internal::ActiveModelTrait;

    let connection = writable_connection::<M>(&active)?;
    let executor = connection.executor();
    let explicit_key = crate::internal::holds_auto_increment_key::<M>(&active);
    let row = connection
        .invalidating(crate::internal::run_profiled(
            active.insert(&executor),
            || {
                crate::internal::model_error_context::<M>(format!(
                    "insert into {}",
                    M::table_name()
                ))
            },
        ))
        .await;
    stored_row::<M>(&connection, &executor, row, explicit_key).await
}

/// Write `active` over the row `primary_key` names and return the row as
/// stored, with no callback: `update()` runs its own around it.
#[doc(hidden)]
pub async fn __update_row<M>(active: M::ActiveModel, primary_key: &M::PrimaryKey) -> Result<M>
where
    M: Model,
    EntityModel<M>: crate::internal::IntoActiveModel<M::ActiveModel>,
{
    use crate::internal::ActiveModelTrait;

    let connection = writable_connection::<M>(&active)?;
    let executor = connection.executor();
    let row = connection
        .invalidating(crate::internal::run_profiled(
            active.update(&executor),
            || {
                crate::internal::primary_key_error_context::<M>(
                    primary_key,
                    format!("update where {}", M::primary_key_display(primary_key)),
                )
            },
        ))
        .await;
    let model = super::__loading_through(&connection, || M::try_from_entity_model(row?))?;
    Ok(model)
}

/// Remove `model`'s row for good and report how many rows went, with no
/// callback: `__force_delete()` runs its own around it.
#[doc(hidden)]
pub async fn __delete_row<M: Model>(model: &M) -> Result<u64> {
    use crate::internal::{EntityTrait, InternalModel, QueryFilter};

    let primary_key = model.primary_key();
    let connection = crate::database::__current_connection()?;
    // The key condition binds a `u64` past `i64::MAX` as a decimal, where the
    // model's own value would panic the driver.
    let executor = connection.executor();
    let result = connection
        .invalidating(crate::internal::run_profiled(
            <M as InternalModel>::Entity::delete_many()
                .filter(M::primary_key_condition(&primary_key))
                .exec(&executor),
            || {
                crate::internal::primary_key_error_context::<M>(
                    &primary_key,
                    format!("delete where {}", M::primary_key_display(&primary_key)),
                )
            },
        ))
        .await;
    if result.as_ref().is_ok_and(|result| result.rows_affected > 0) {
        connection.recording(|| super::__forget_dirty_snapshot(model));
    }
    Ok(result?.rows_affected)
}

pub(crate) async fn transaction<F, T>(f: F) -> Result<T>
where
    F: for<'c> FnOnce(
            &'c crate::database::Transaction,
        ) -> std::pin::Pin<Box<dyn Future<Output = Result<T>> + Send + 'c>>
        + Send,
    T: Send,
{
    crate::database::__current_db()?.transaction(f).await
}

/// The row `primary_key` names, trashed or not, as `reload()` and
/// `soft_delete()` read back the record in hand.
pub(crate) async fn reload_by_key<M>(primary_key: M::PrimaryKey) -> Result<M>
where
    M: Model,
{
    let connection = crate::database::__current_connection()?;
    crate::internal::find_by_primary_key::<M>(&connection, &primary_key, true, "reload")
        .await?
        .ok_or_else(|| {
            Error::not_found(format!(
                "{} with {} no longer exists",
                M::table_name(),
                M::primary_key_display(&primary_key)
            ))
        })
}

pub(crate) async fn reload<M>(model: &M) -> Result<M>
where
    M: Model,
{
    reload_by_key::<M>(model.primary_key()).await
}

pub(crate) fn is_new<M>(model: &M) -> bool
where
    M: Model,
{
    M::primary_key_is_new(&model.primary_key())
}

#[cfg(test)]
#[path = "../../tests/unit/model_crud_tests.rs"]
mod tests;
