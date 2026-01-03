//! Internal SeaORM adapter layer
//!
//! ⚠️ **WARNING: This module is internal and should NEVER be exposed to end users.**
//!
//! This module serves as the adapter between TideORM's public API and SeaORM's internals.
//! All SeaORM interactions happen through this layer, ensuring:
//!
//! 1. SeaORM types never leak to the public API
//! 2. We can swap the underlying ORM engine if needed
//! 3. Error translation happens in one place
//! 4. Query translation is centralized

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::error::{Error, Result};

// Re-export SeaORM internally (but this module itself is #[doc(hidden)])
pub use sea_orm::{
    entity::prelude::*,
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ColumnTrait, ColumnType, Condition,
    ConnectionTrait, Database as SeaDatabase, DatabaseConnection, DatabaseTransaction,
    DbBackend, DbErr, EntityTrait, FromQueryResult, Iden, IntoActiveModel, ModelTrait, Iterable,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    Statement, TransactionTrait, Value, ExecResult, TryGetable,
    DeriveEntityModel, EnumIter, DeriveRelation, DeleteMany,
    sea_query::{Expr, Asterisk, Alias},
    ConnectOptions,
};

/// Internal trait that maps TideORM models to SeaORM entities
/// This is implemented by the `#[derive(Model)]` macro
/// 
/// ⚠️ Users should NEVER implement or interact with this trait directly
#[doc(hidden)]
pub trait InternalModel: Sized + Send + Sync + Clone {
    type Entity: EntityTrait;
    type ActiveModel: ActiveModelTrait<Entity = Self::Entity> + ActiveModelBehavior + Send;
    
    /// Convert TideORM model to SeaORM ActiveModel
    fn into_active_model(self) -> Self::ActiveModel;
    
    /// Convert SeaORM Model to TideORM model
    fn from_sea_model(model: <Self::Entity as EntityTrait>::Model) -> Self;
    
    /// Get SeaORM primary key column (optional, for find_by_id)
    fn primary_key_column() -> Option<<Self::Entity as EntityTrait>::Column> {
        None
    }
}

/// Internal connection wrapper
#[doc(hidden)]
pub struct InternalConnection {
    pub(crate) conn: DatabaseConnection,
}

impl InternalConnection {
    pub async fn connect(url: &str) -> Result<Self> {
        let conn = SeaDatabase::connect(url)
            .await
            .map_err(|e| Error::connection(e.to_string()))?;
        Ok(Self { conn })
    }
    
    pub fn connection(&self) -> &DatabaseConnection {
        &self.conn
    }
}

/// Translate SeaORM DbErr to TideORM Error
pub(crate) fn translate_error(err: DbErr) -> Error {
    match err {
        DbErr::RecordNotFound(msg) => Error::not_found(msg),
        DbErr::ConnectionAcquire(e) => Error::connection(e.to_string()),
        DbErr::Conn(e) => Error::connection(e.to_string()),
        DbErr::Exec(e) => Error::query(e.to_string()),
        DbErr::Query(e) => Error::query(e.to_string()),
        DbErr::ConvertFromU64(msg) => Error::conversion(msg),
        DbErr::UnpackInsertId => Error::query("Failed to get insert ID".to_string()),
        DbErr::UpdateGetPrimaryKey => Error::query("Failed to get primary key after update".to_string()),
        DbErr::Custom(msg) => Error::internal(msg),
        _ => Error::internal(err.to_string()),
    }
}

/// Internal query executor
#[doc(hidden)]
pub struct QueryExecutor;

impl QueryExecutor {
    /// Find all records
    pub async fn find_all<M>(conn: &DatabaseConnection) -> Result<Vec<M>>
    where
        M: InternalModel,
    {
        let results = M::Entity::find()
            .all(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(results.into_iter().map(M::from_sea_model).collect())
    }
    
    /// Get first record
    pub async fn first<M>(conn: &DatabaseConnection) -> Result<Option<M>>
    where
        M: InternalModel,
    {
        let result = M::Entity::find()
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.map(M::from_sea_model))
    }
    
    /// Get last record (by primary key descending)
    pub async fn last<M>(conn: &DatabaseConnection) -> Result<Option<M>>
    where
        M: InternalModel,
    {
        // SeaORM doesn't have a simple "last" - we just get first for now
        // In real implementation, we'd order by PK DESC
        let result = M::Entity::find()
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.map(M::from_sea_model))
    }
    
    /// Count records
    pub async fn count<M>(conn: &DatabaseConnection, _condition: Option<Condition>) -> Result<u64>
    where
        M: InternalModel,
    {
        #[derive(Debug, FromQueryResult)]
        struct CountResult {
            count: i64,
        }
        
        let result: Option<CountResult> = M::Entity::find()
            .select_only()
            .column_as(Expr::col(Asterisk).count(), "count")
            .into_model::<CountResult>()
            .one(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(result.map(|r| r.count as u64).unwrap_or(0))
    }
    
    /// Paginate records
    pub async fn paginate<M>(conn: &DatabaseConnection, limit: u64, offset: u64) -> Result<Vec<M>>
    where
        M: InternalModel,
    {
        let results = M::Entity::find()
            .offset(offset)
            .limit(limit)
            .all(conn)
            .await
            .map_err(translate_error)?;
        
        Ok(results.into_iter().map(M::from_sea_model).collect())
    }
    
    /// Delete a record
    pub async fn delete<M>(conn: &DatabaseConnection, model: M) -> Result<u64>
    where
        M: InternalModel,
    {
        let active = model.into_active_model();
        let result = active.delete(conn).await.map_err(translate_error)?;
        Ok(result.rows_affected)
    }
}
