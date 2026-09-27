#![allow(missing_docs)]

use std::future::Future;

use crate::error::{Error, Result};

use super::Model;

pub(crate) async fn all<M>() -> Result<Vec<M>>
where
    M: Model,
{
    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::find_all::<M, _>(&connection.executor()).await
}

pub(crate) async fn count<M>() -> Result<u64>
where
    M: Model,
{
    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::count::<M, _>(&connection.executor()).await
}

pub(crate) async fn exists_any<M>() -> Result<bool>
where
    M: Model,
{
    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::exists_any::<M, _>(&connection.executor()).await
}

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
            let (field, message) = errors
                .first()
                .map(|(field, message)| (field.clone(), message.clone()))
                .unwrap_or_else(|| ("unknown".to_string(), "Validation failed".to_string()));
            return Err(Error::validation(
                field,
                format!("{message} (model {index} of the batch)"),
            ));
        }
    }

    let connection = crate::database::__current_connection()?;
    let inserted =
        crate::internal::QueryExecutor::insert_many::<M>(&connection.executor(), models).await?;
    // New rows leave every loaded model as it was, so only cached reads go:
    // the dirty-tracking baselines stay, the inserted models' among them, as
    // `create()` leaves its own.
    if !inserted.is_empty() {
        crate::QueryCache::global().invalidate_model(M::table_name());
    }
    Ok(inserted)
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

pub(crate) async fn first<M>() -> Result<Option<M>>
where
    M: Model,
{
    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::first::<M, _>(&connection.executor()).await
}

pub(crate) async fn last<M>() -> Result<Option<M>>
where
    M: Model,
{
    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::last::<M, _>(&connection.executor()).await
}

pub(crate) async fn paginate<M>(page: u64, per_page: u64) -> Result<Vec<M>>
where
    M: Model,
{
    // The same check `QueryBuilder::page` makes, so both refuse alike. Every
    // backend takes LIMIT and OFFSET as signed 64-bit integers, which it
    // already bounds both by.
    let offset = crate::query::page_offset(page, per_page)
        .map_err(|(field, message)| Error::validation(field, message))?;
    let (limit, offset) = (per_page as i64, offset as i64);

    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::paginate::<M, _>(&connection.executor(), limit, offset).await
}

/// Read the row with this primary key whether or not it is soft-deleted, for
/// `reload` and `soft_delete`, whose record is the one in hand.
pub(crate) async fn find_including_trashed<M>(id: M::PrimaryKey) -> Result<Option<M>>
where
    M: Model,
{
    use crate::internal::{EntityTrait, InternalModel, QueryFilter};

    // Resolved outside the profiled statement so an outage keeps its
    // `Error::Connection` class, exactly as it does for `find`. A key matches
    // at most one row; `all` reads it without the bound `LIMIT` that `one`
    // adds, which recent SQLite releases recompile on every run.
    let connection = crate::database::__current_connection()?;
    let rows = crate::profiling::__profile_future(
        <M as InternalModel>::Entity::find()
            .filter(<M as InternalModel>::primary_key_condition(&id))
            .all(&connection.executor()),
    )
    .await?;

    rows.into_iter()
        .next()
        .map(M::try_from_entity_model)
        .transpose()
}

pub(crate) async fn reload<M>(model: &M) -> Result<M>
where
    M: Model,
{
    let primary_key = model.primary_key();
    let id_display = M::primary_key_display(&primary_key);

    find_including_trashed::<M>(primary_key)
        .await?
        .ok_or_else(|| {
            Error::not_found(format!(
                "{} with {} no longer exists",
                M::table_name(),
                id_display
            ))
        })
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
