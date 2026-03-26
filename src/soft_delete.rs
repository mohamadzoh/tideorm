//! Soft Delete support for TideORM models
//!
//! Soft deletes allow you to mark records as deleted without actually removing them
//! from the database. This is useful for maintaining data history and implementing
//! features like a trash/recycle bin.
//!
//! ## Usage
//!
//! ```no_run
//! use tideorm::prelude::*;
//! use tideorm::SoftDelete;
//!
//! #[tideorm::model(table = "posts", soft_delete)]
//! pub struct Post {
//!     #[tideorm(primary_key, auto_increment)]
//!     pub id: i64,
//!     pub title: String,
//!     pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
//! }
//!
//! // `#[tideorm(soft_delete)]` generates the `SoftDelete` impl automatically.
//! // Use `deleted_at_column = "archived_on"` when the field/column name is not `deleted_at`.
//!
//! # async fn demo(post: Post) -> tideorm::Result<()> {
//! // Soft delete a post
//! post.clone().soft_delete().await?;
//!
//! // Restore a soft-deleted post
//! post.clone().restore().await?;
//!
//! // Query including soft-deleted records
//! let all_posts = Post::query().with_trashed().get().await?;
//!
//! // Query only soft-deleted records
//! let trashed = Post::query().only_trashed().get().await?;
//! # let _ = (all_posts, trashed);
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::Result;
use crate::model::Model;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SoftDeleteScope {
    Disabled,
    ActiveOnly,
    WithTrashed,
    OnlyTrashed,
}

pub(crate) fn query_scope_for<M: Model>(
    include_trashed: bool,
    only_trashed: bool,
) -> SoftDeleteScope {
    if !M::soft_delete_enabled() {
        SoftDeleteScope::Disabled
    } else if only_trashed {
        SoftDeleteScope::OnlyTrashed
    } else if include_trashed {
        SoftDeleteScope::WithTrashed
    } else {
        SoftDeleteScope::ActiveOnly
    }
}

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
    /// ```no_run
    /// # use tideorm::prelude::*;
    /// # #[tideorm::model(table = "posts", soft_delete)]
    /// # struct Post {
    /// #     #[tideorm(primary_key, auto_increment)]
    /// #     id: i64,
    /// #     title: String,
    /// #     deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// # }
    /// # async fn demo() -> tideorm::Result<()> {
    /// let post = Post::find_or_fail(1).await?;
    /// post.soft_delete().await?;
    /// # Ok(())
    /// # }
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
    /// ```no_run
    /// # use tideorm::prelude::*;
    /// # #[tideorm::model(table = "posts", soft_delete)]
    /// # struct Post {
    /// #     #[tideorm(primary_key, auto_increment)]
    /// #     id: i64,
    /// #     title: String,
    /// #     deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// # }
    /// # async fn demo() -> tideorm::Result<()> {
    /// let post = Post::query().only_trashed().first().await?.unwrap();
    /// post.restore().await?;
    /// # Ok(())
    /// # }
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
    /// ```no_run
    /// # use tideorm::prelude::*;
    /// # #[tideorm::model(table = "posts", soft_delete)]
    /// # struct Post {
    /// #     #[tideorm(primary_key, auto_increment)]
    /// #     id: i64,
    /// #     title: String,
    /// #     deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// # }
    /// # async fn demo(post: Post) -> tideorm::Result<()> {
    /// post.force_delete().await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn force_delete(self) -> Result<u64>
    where
        Self: Sized,
    {
        <Self as Model>::delete(self).await
    }
}
