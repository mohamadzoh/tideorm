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
/// User::on_conflict(vec!["email"])
///     .update_all_except(vec!["role"])
///     .insert(user)
///     .await?;
/// ```
///
/// The fields are public because macro-generated model code constructs and
/// reads this type; they are not part of the supported API.
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
    #[must_use]
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
    /// Mutually exclusive with
    /// [`OnConflictBuilder::update_all_except`]; when both are set, this one wins.
    #[must_use]
    pub fn update_columns(mut self, columns: Vec<&str>) -> Self {
        self.update_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Overwrite every column on conflict except these, and except a managed
    /// `created_at`.
    ///
    /// The usual reason to reach for this is to protect insert-only columns
    /// while letting new values win everywhere else.
    #[must_use]
    pub fn update_all_except(mut self, columns: Vec<&str>) -> Self {
        self.exclude_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
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
