//! Soft Delete support for TideORM models
//!
//! Soft delete keeps a record in the table while marking it as deleted through a
//! timestamp column.
//!
//! Use it when records should disappear from normal reads without being removed
//! permanently. If soft-deleted rows still appear unexpectedly, check whether the
//! query opted into `with_trashed()` or `only_trashed()`.
//!
//! `#[tideorm(soft_delete)]` usually generates the implementation for you.
//! Use `deleted_at_column = "..."` when the soft-delete timestamp column is not
//! named `deleted_at`.

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
/// `deleted_at_column = "..."` override on the model. The column name itself is
/// [`ModelMeta::deleted_at_column`](crate::model::ModelMeta::deleted_at_column).
#[async_trait]
pub trait SoftDelete: Model {
    /// Get the deleted_at timestamp
    fn deleted_at(&self) -> Option<DateTime<Utc>>;

    /// Set the deleted_at timestamp
    fn set_deleted_at(&mut self, timestamp: Option<DateTime<Utc>>);

    /// Check if this record is soft deleted
    fn is_deleted(&self) -> bool {
        self.deleted_at().is_some()
    }

    /// Mark the record as deleted, and return it as stored.
    ///
    /// On a `#[tideorm(soft_delete)]` model this is `delete()`: the delete
    /// callbacks run, so a `before_delete` that refuses stops it, and a record
    /// already deleted keeps its stamp, which a retention purge reads.
    ///
    /// The stamp is `Utc::now()` and never the database's `CURRENT_TIMESTAMP`.
    /// `deleted_at` is a `DateTime<Utc>`, so a session-local database clock —
    /// which is what `CURRENT_TIMESTAMP` returns on MySQL/MariaDB — would store
    /// an offset instant that is later read back as if it were UTC. The
    /// query-level `QueryBuilder::soft_delete` renders the same UTC instant for
    /// exactly this reason; keep the two in agreement.
    async fn soft_delete(mut self) -> Result<Self> {
        // `delete()` removes the row of a model without the flag, so a hand
        // written implementation on one only sets the stamp.
        if !Self::soft_delete_enabled() {
            self.set_deleted_at(Some(Utc::now()));
            return self.update().await;
        }
        let primary_key = self.primary_key();
        <Self as Model>::delete(self).await?;
        crate::model::reload_by_key::<Self>(primary_key).await
    }

    /// Clear the soft-delete timestamp and persist the restored record.
    async fn restore(mut self) -> Result<Self> {
        self.set_deleted_at(None);
        self.update().await
    }

    /// Bypass soft deletion and remove the record permanently.
    async fn force_delete(self) -> Result<u64> {
        <Self as Model>::__force_delete(self).await
    }
}
