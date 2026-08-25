#![allow(missing_docs)]

use std::future::Future;

use crate::error::{Error, Result};

use super::Model;

pub(crate) fn db() -> Result<crate::database::Database> {
    crate::database::require_db()
}

pub(crate) async fn all<M>() -> Result<Vec<M>>
where
    M: Model + Sized,
{
    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::find_all::<M, _>(conn.connection()).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::find_all::<M, _>(tx.as_ref()).await
        }
    }
}

pub(crate) async fn count<M>() -> Result<u64>
where
    M: Model + Sized,
{
    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::count::<M, _>(conn.connection(), None).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::count::<M, _>(tx.as_ref(), None).await
        }
    }
}

pub(crate) async fn exists_any<M>() -> Result<bool>
where
    M: Model + Sized,
{
    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::exists_any::<M, _>(conn.connection()).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::exists_any::<M, _>(tx.as_ref()).await
        }
    }
}

pub(crate) async fn insert_all<M>(models: Vec<M>) -> Result<Vec<M>>
where
    M: Model + Sized,
    <<M as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        crate::internal::IntoActiveModel<<M as crate::internal::InternalModel>::ActiveModel>,
{
    if models.is_empty() {
        return Ok(Vec::new());
    }

    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::insert_many::<M, _>(conn.connection(), models).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::insert_many::<M, _>(tx.as_ref(), models).await
        }
    }
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
    M: Model + Sized,
{
    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::first::<M, _>(conn.connection()).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::first::<M, _>(tx.as_ref()).await
        }
    }
}

pub(crate) async fn last<M>() -> Result<Option<M>>
where
    M: Model + Sized,
{
    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::last::<M, _>(conn.connection()).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::last::<M, _>(tx.as_ref()).await
        }
    }
}

pub(crate) async fn paginate<M>(page: u64, per_page: u64) -> Result<Vec<M>>
where
    M: Model + Sized,
{
    if page == 0 {
        return Err(Error::validation("page", "must be at least 1"));
    }

    if per_page == 0 {
        return Err(Error::validation("per_page", "must be greater than 0"));
    }

    // `page` is 1-based and already known to be non-zero here, but the product
    // still has to be checked: an out-of-range page number used to panic in
    // debug builds and wrap around to a small offset in release ones, silently
    // returning the wrong page instead of reporting the bad input.
    let offset = (page - 1).checked_mul(per_page).ok_or_else(|| {
        Error::validation(
            "page",
            "page is too large for this page size; (page - 1) * per_page overflows",
        )
    })?;

    match crate::database::__current_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::paginate::<M, _>(conn.connection(), per_page, offset)
                .await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::paginate::<M, _>(tx.as_ref(), per_page, offset).await
        }
    }
}

/// Look up a model by primary key while honoring its soft-delete scope.
///
/// The macro-generated `Model::find` intentionally applies no scope, so callers that
/// should hide trashed rows (`exists`, `find_or_fail`) go through here instead. Models
/// without soft delete fall straight back to `Model::find`.
pub(crate) async fn find_active<M>(id: M::PrimaryKey) -> Result<Option<M>>
where
    M: Model + Sized,
{
    use crate::internal::{ColumnTrait, EntityTrait, InternalModel, QueryFilter};

    if !M::soft_delete_enabled() {
        return M::find(id).await;
    }

    let Some(deleted_at_column) = M::column_from_str(M::deleted_at_column()) else {
        return M::find(id).await;
    };

    // Resolved before the profiled future so a connection failure keeps its own
    // classification. Funnelling it through the engine's error type instead
    // would flatten it into `OrmError::Custom`, which translates to
    // `Error::Internal` — and `exists`/`find_or_fail` would then report an
    // outage differently from `find`, breaking retry and health-check branches.
    let connection = crate::database::__current_connection()?;

    let result = crate::profiling::__profile_future(async move {
        let scoped_find = || {
            <<M as InternalModel>::Entity as EntityTrait>::find()
                .filter(<M as InternalModel>::primary_key_condition(&id))
                .filter(deleted_at_column.is_null())
        };

        match connection {
            crate::database::ConnectionRef::Database(conn) => {
                scoped_find().one(conn.connection()).await
            }
            crate::database::ConnectionRef::Transaction(tx) => scoped_find().one(tx.as_ref()).await,
        }
    })
    .await?;

    result.map(M::try_from_entity_model).transpose()
}

pub(crate) async fn reload<M>(model: &M) -> Result<M>
where
    M: Model + Sized,
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
mod tests {
    use super::*;
    // The macro-generated entity module emits `Result<_, DbErr>`, so it must not see
    // tideorm's own one-parameter `Result<T>` alias that `use super::*` brings in here.
    use std::result::Result;

    #[tideorm::model(table = "crud_pagination_users")]
    struct PaginationUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
    }

    #[tideorm::model(table = "crud_soft_delete_users", soft_delete)]
    struct SoftDeleteUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        name: String,
        deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[tokio::test]
    async fn paginate_rejects_a_zero_page_number() {
        let error = paginate::<PaginationUser>(0, 10)
            .await
            .expect_err("page 0 should be rejected");

        assert!(
            error.to_string().contains("must be at least 1"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn paginate_rejects_a_zero_page_size() {
        let error = paginate::<PaginationUser>(1, 0)
            .await
            .expect_err("per_page 0 should be rejected");

        assert!(
            error.to_string().contains("must be greater than 0"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn paginate_reports_an_offset_that_would_overflow() {
        // Used to panic in debug builds and wrap to a small offset in release.
        let error = paginate::<PaginationUser>(u64::MAX, 4)
            .await
            .expect_err("an overflowing offset should be reported");

        assert!(
            error.to_string().contains("overflows"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn find_active_reports_a_missing_connection_as_a_connection_error() {
        // `find` reports an outage as `Error::Connection`; the soft-delete path
        // used to rewrap it as an engine `Custom` error, which translates to
        // `Error::Internal` and takes `exists`/`find_or_fail` off every
        // connection-specific branch.
        crate::database::Database::reset_global();

        let error = find_active::<SoftDeleteUser>(1)
            .await
            .expect_err("a missing global connection should be reported");

        assert!(
            matches!(error, Error::Connection { .. }),
            "expected a connection error, got: {error:?}"
        );
    }
}
