use crate::columns::{IntoColumnNames, column_names};
use crate::error::Result;

use super::Model;

/// Builder for on-conflict (upsert) operations.
///
/// Returned by [`Model::on_conflict`](crate::model::Model::on_conflict). The
/// conflict columns name a unique constraint or unique index; when the insert
/// collides with it the row is updated instead of failing.
///
/// By default every non-conflict column is overwritten, except a managed
/// `created_at`, which keeps the stored row's creation time. Narrow that with
/// [`OnConflictBuilder::update_columns`] or [`OnConflictBuilder::update_all_except`].
///
/// ```ignore
/// User::on_conflict(["email"])
///     .update_all_except(["role"])
///     .insert(user)
///     .await?;
///
/// // Typed columns work everywhere a column is named.
/// User::on_conflict([User::columns.email]).do_nothing().insert(user).await?;
/// ```
///
/// The fields are public because macro-generated model code constructs and
/// reads this type; they are not part of the supported API.
#[must_use = "an upsert builder does nothing until `insert` is awaited"]
pub struct OnConflictBuilder<M: Model> {
    #[doc(hidden)]
    pub _marker: std::marker::PhantomData<M>,
    #[doc(hidden)]
    pub conflict_columns: Vec<String>,
    #[doc(hidden)]
    pub update_columns: Option<Vec<String>>,
    #[doc(hidden)]
    pub exclude_columns: Option<Vec<String>>,
}

impl<M: Model> OnConflictBuilder<M> {
    /// Start a builder that treats a collision on `conflict_columns` as an update.
    ///
    /// Prefer [`Model::on_conflict`](crate::model::Model::on_conflict), which
    /// calls this for you.
    pub fn new(conflict_columns: Vec<String>) -> Self {
        Self {
            _marker: std::marker::PhantomData,
            conflict_columns,
            update_columns: None,
            exclude_columns: None,
        }
    }

    /// Overwrite only these columns on conflict.
    ///
    /// Everything else keeps the value already stored; name `updated_at` here
    /// to refresh it, and `created_at` only to overwrite the creation time.
    /// MySQL and MariaDB reject primary-key columns in this list before writing.
    /// Mutually exclusive with
    /// [`OnConflictBuilder::update_all_except`]; when both are set, this one wins.
    ///
    /// Columns are names or typed columns (`User::columns.name`); to update
    /// nothing, use [`OnConflictBuilder::do_nothing`].
    pub fn update_columns(mut self, columns: impl IntoColumnNames) -> Self {
        self.update_columns = Some(column_names(&columns));
        self
    }

    /// Overwrite every column on conflict except these, and except a managed
    /// `created_at`.
    ///
    /// The usual reason to reach for this is to protect insert-only columns
    /// while letting new values win everywhere else.
    pub fn update_all_except(mut self, columns: impl IntoColumnNames) -> Self {
        self.exclude_columns = Some(column_names(&columns));
        self
    }

    /// Keep the stored row unchanged on conflict.
    ///
    /// [`OnConflictBuilder::insert`] then returns the row already stored, or
    /// the new one when nothing collided, so the call reads "insert unless it
    /// exists" and always yields the row.
    pub fn do_nothing(mut self) -> Self {
        self.update_columns = Some(Vec::new());
        self
    }

    /// Run the upsert and return the stored row.
    ///
    /// The model is checked against its validation rules first; no callbacks
    /// run. The returned model reflects what the database ended up with, so it
    /// carries the existing primary key when the insert turned into an update.
    pub async fn insert(self, model: M) -> Result<M> {
        M::__insert_with_conflict(model, self).await
    }
}
