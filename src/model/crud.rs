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
    crate::internal::QueryExecutor::insert_many::<M>(&connection.executor(), models).await
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
    if page == 0 {
        return Err(Error::validation("page", "must be at least 1"));
    }

    if per_page == 0 {
        return Err(Error::validation("per_page", "must be greater than 0"));
    }

    // `page` is already known to be non-zero, but the product can still
    // overflow for an out-of-range page number; report that as bad input.
    let offset = (page - 1).checked_mul(per_page).ok_or_else(|| {
        Error::validation(
            "page",
            "page is too large for this page size; (page - 1) * per_page overflows",
        )
    })?;

    // Every backend takes LIMIT and OFFSET as signed 64-bit integers.
    let limit = i64::try_from(per_page)
        .map_err(|_| Error::validation("per_page", "must be at most i64::MAX"))?;
    let offset = i64::try_from(offset).map_err(|_| {
        Error::validation(
            "page",
            "page is too large for this page size; (page - 1) * per_page exceeds i64::MAX",
        )
    })?;

    let connection = crate::database::__current_connection()?;
    crate::internal::QueryExecutor::paginate::<M, _>(&connection.executor(), limit, offset).await
}

/// Look up a model by primary key while honoring its soft-delete scope.
///
/// The macro-generated `Model::find` intentionally applies no scope, so callers that
/// should hide trashed rows (`exists`, `find_or_fail`) go through here instead; for a
/// model without soft delete the scope is empty and this finds what `Model::find` does.
pub(crate) async fn find_active<M>(id: M::PrimaryKey) -> Result<Option<M>>
where
    M: Model,
{
    use crate::internal::{InternalModel, QueryFilter};

    // Resolved outside the profiled statement so an outage keeps its
    // `Error::Connection` class, exactly as it does for `find`.
    let connection = crate::database::__current_connection()?;
    // A key matches at most one row; `all` reads it without the bound `LIMIT`
    // that `one` adds, which recent SQLite releases recompile on every run.
    let rows = crate::profiling::__profile_future(
        crate::internal::scoped_find::<M>()
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

    M::find(primary_key).await?.ok_or_else(|| {
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
