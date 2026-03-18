//! Soft Delete support for TideORM models
//!
//! Soft deletes allow you to mark records as deleted without actually removing them
//! from the database. This is useful for maintaining data history and implementing
//! features like a trash/recycle bin.
//!
//! ## Usage
//!
//! ```ignore
//! use tideorm::prelude::*;
//! use tideorm::SoftDelete;
//!
//! #[derive(Model)]
//! #[tideorm(table = "posts", soft_delete)]
//! pub struct Post {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub title: String,
//!     pub deleted_at: Option<DateTime<Utc>>,
//! }
//!
//! // `#[tideorm(soft_delete)]` generates the `SoftDelete` impl automatically.
//! // Use `deleted_at_column = "archived_on"` when the field/column name is not `deleted_at`.
//!
//! // Soft delete a post
//! post.soft_delete(&db).await?;
//!
//! // Restore a soft-deleted post
//! post.restore(&db).await?;
//!
//! // Query including soft-deleted records
//! let all_posts = Post::with_trashed(&db).await?;
//!
//! // Query only soft-deleted records
//! let trashed = Post::only_trashed(&db).await?;
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::Model;

/// Trait for models that support soft deletion
///
/// `#[tideorm(soft_delete)]` models receive an implementation automatically as long
/// as they expose a `deleted_at` field/column, or declare a custom
/// `deleted_at_column = "..."` override on the model.
#[async_trait]
pub trait SoftDelete: Model {
    /// The name of the deleted_at column
    fn deleted_at_column() -> &'static str {
        "deleted_at"
    }

    /// Get the deleted_at timestamp
    fn deleted_at(&self) -> Option<DateTime<Utc>>;

    /// Set the deleted_at timestamp
    fn set_deleted_at(&mut self, timestamp: Option<DateTime<Utc>>);

    /// Check if this record is soft deleted
    fn is_deleted(&self) -> bool {
        self.deleted_at().is_some()
    }

    /// Soft delete this record (sets deleted_at to now)
    ///
    /// # Example
    /// ```ignore
    /// let post = Post::find_or_fail(1).await?;
    /// post.soft_delete().await?;
    /// ```
    async fn soft_delete(mut self) -> Result<Self>
    where
        Self: Sized,
    {
        self.set_deleted_at(Some(Utc::now()));
        self.update().await
    }

    /// Restore a soft-deleted record (sets deleted_at to None)
    ///
    /// # Example
    /// ```ignore
    /// let post = Post::only_trashed().await?.into_iter().next().unwrap();
    /// post.restore().await?;
    /// ```
    async fn restore(mut self) -> Result<Self>
    where
        Self: Sized,
    {
        self.set_deleted_at(None);
        self.update().await
    }

    /// Permanently delete this record from the database
    ///
    /// # Example
    /// ```ignore
    /// post.force_delete().await?;
    /// ```
    async fn force_delete(self) -> Result<u64>
    where
        Self: Sized,
    {
        <Self as Model>::delete(self).await
    }

    // Note: Static methods like with_trashed(), only_trashed() would need
    // to be implemented via query builder extensions or custom methods
    // on each model since Rust doesn't support static methods in traits
    // that return Self collections easily.
}

/// Extension methods for querying soft-deleted records
///
/// These methods can be added to the QueryBuilder to support soft delete queries
pub mod query_extensions {
    /// Query scope that excludes soft-deleted records
    pub const WITHOUT_TRASHED: &str = "deleted_at IS NULL";

    /// Query scope that includes only soft-deleted records  
    pub const ONLY_TRASHED: &str = "deleted_at IS NOT NULL";
}
