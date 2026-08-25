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

    /// Mark the record as deleted by setting the soft-delete timestamp.
    ///
    /// The stamp is `Utc::now()` and never the database's `CURRENT_TIMESTAMP`.
    /// `deleted_at` is a `DateTime<Utc>`, so a session-local database clock —
    /// which is what `CURRENT_TIMESTAMP` returns on MySQL/MariaDB — would store
    /// an offset instant that is later read back as if it were UTC. The
    /// query-level `QueryBuilder::soft_delete` renders the same UTC instant for
    /// exactly this reason; keep the two in agreement.
    async fn soft_delete(mut self) -> Result<Self>
    where
        Self: Sized,
    {
        self.set_deleted_at(Some(Utc::now()));
        self.update().await
    }

    /// Clear the soft-delete timestamp and persist the restored record.
    async fn restore(mut self) -> Result<Self>
    where
        Self: Sized,
    {
        self.set_deleted_at(None);
        self.update().await
    }

    /// Bypass soft deletion and remove the record permanently.
    async fn force_delete(self) -> Result<u64>
    where
        Self: Sized,
    {
        <Self as Model>::delete(self).await
    }
}
