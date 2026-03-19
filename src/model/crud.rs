#![allow(missing_docs)]

use std::future::Future;
use std::pin::Pin;

use crate::database::Connection;
use crate::error::Result;

use super::Model;

pub(crate) fn db() -> Result<crate::database::Database> {
    crate::database::require_db()
}

pub(crate) fn database() -> Result<crate::database::Database> {
    crate::database::require_db()
}

pub(crate) async fn all<M>() -> Result<Vec<M>>
where
    M: Model + Sized,
{
    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::find_all::<M, _>(&conn).await
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
    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::count::<M, _>(&conn, None).await
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
    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::exists_any::<M, _>(&conn).await
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

    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::insert_many::<M, _>(&conn, models).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::insert_many::<M, _>(tx.as_ref(), models).await
        }
    }
}

pub(crate) async fn insert_many_returning<M>(models: Vec<M>) -> Result<Vec<M>>
where
    M: Model + Sized,
    <<M as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        crate::internal::IntoActiveModel<<M as crate::internal::InternalModel>::ActiveModel>,
{
    if models.is_empty() {
        return Ok(Vec::new());
    }

    insert_all::<M>(models).await
}

pub(crate) async fn insert_many<M>(models: Vec<M>) -> Result<Vec<M>>
where
    M: Model + Sized,
    <<M as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
        crate::internal::IntoActiveModel<<M as crate::internal::InternalModel>::ActiveModel>,
{
    if models.is_empty() {
        return Ok(Vec::new());
    }

    insert_all::<M>(models).await
}

pub(crate) async fn transaction<M, F, T>(f: F) -> Result<T>
where
    M: Model + Sized,
    F: for<'c> FnOnce(
            &'c crate::database::Transaction,
        ) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'c>>
        + Send,
    T: Send,
{
    crate::database::__current_db()?.transaction(f).await
}

pub(crate) async fn first<M>() -> Result<Option<M>>
where
    M: Model + Sized,
{
    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::first::<M, _>(&conn).await
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
    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::last::<M, _>(&conn).await
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
    let offset = (page.saturating_sub(1)) * per_page;
    let db = crate::database::__current_db()?;
    match db.__get_connection()? {
        crate::database::ConnectionRef::Database(conn) => {
            crate::internal::QueryExecutor::paginate::<M, _>(&conn, per_page, offset).await
        }
        crate::database::ConnectionRef::Transaction(tx) => {
            crate::internal::QueryExecutor::paginate::<M, _>(tx.as_ref(), per_page, offset).await
        }
    }
}

pub(crate) async fn reload<M>(model: &M) -> Result<M>
where
    M: Model + Sized,
{
    M::find_or_fail(model.primary_key()).await
}

pub(crate) fn is_new<M>(model: &M) -> bool
where
    M: Model,
{
    let primary_key = model.primary_key().to_string();

    if primary_key.is_empty() {
        return true;
    }

    if M::primary_key_auto_increment() {
        return primary_key
            .parse::<i128>()
            .map(|value| value == 0)
            .unwrap_or(false);
    }

    false
}