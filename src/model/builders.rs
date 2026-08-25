//! Field-at-a-time builders for writes.
//!
//! These complement the struct-literal path (`User { .. }.save()`): reach for a
//! builder when the fields to write are decided at runtime — from a form, a
//! patch document, or a config map — so field names arrive as strings rather
//! than as struct fields.

use crate::error::{Error, Result};

use super::{Model, ModelMeta, serialization};

/// Builder for creating new model instances.
///
/// Accumulates field values by name and inserts them with
/// [`CreateBuilder::save`]. Use this when the set of fields is only known at
/// runtime; prefer `M::create(model)` with a struct literal when it is not,
/// because that form is checked by the compiler.
///
/// Every field the model requires must be set before `save()` — the builder
/// starts empty and does not fill in defaults for you.
pub struct CreateBuilder<M: ModelMeta> {
    _marker: std::marker::PhantomData<M>,
    values: std::collections::HashMap<String, serde_json::Value>,
}

impl<M: ModelMeta> CreateBuilder<M> {
    /// Start an empty builder for `M`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            values: std::collections::HashMap::new(),
        }
    }

    /// Stage a value for one field.
    ///
    /// `field` accepts either the Rust field name or the database column name.
    /// Neither the name nor the value is checked here: an unknown name or a
    /// value of the wrong shape surfaces as an error from
    /// [`CreateBuilder::save`], not from this call. Setting the same field twice
    /// keeps the last value.
    #[must_use]
    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.values.insert(field.to_string(), value.into());
        self
    }
}

impl<M: Model> CreateBuilder<M> {
    /// Build the model from the accumulated values and insert it.
    ///
    /// Field names accept either the Rust field name or the database column
    /// name. Returns an error when a name is unknown to the model or when the
    /// collected values do not describe a complete model.
    pub async fn save(self) -> Result<M> {
        let model =
            serialization::model_from_values::<M>(self.values).map_err(Error::conversion)?;
        M::create(model).await
    }
}

impl<M: ModelMeta> Default for CreateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for updating model instances.
///
/// Wraps one already-loaded model and applies named changes to it before
/// writing. This is the single-row counterpart to
/// [`BatchUpdateBuilder`](crate::model::BatchUpdateBuilder): it needs the row in
/// hand, runs the model's callbacks and validations, and issues an `UPDATE` for
/// that one primary key.
pub struct UpdateBuilder<M: Model> {
    model: M,
    changes: std::collections::HashMap<String, serde_json::Value>,
}

impl<M: Model> UpdateBuilder<M> {
    /// Start a builder that will update `model`.
    #[must_use]
    pub fn new(model: M) -> Self {
        Self {
            model,
            changes: std::collections::HashMap::new(),
        }
    }

    /// Stage a change to one field.
    ///
    /// `field` accepts either the Rust field name or the database column name.
    /// The name is resolved when [`UpdateBuilder::save`] runs, not here.
    /// Setting the same field twice keeps the last value.
    #[must_use]
    pub fn set(mut self, field: &str, value: impl Into<serde_json::Value>) -> Self {
        self.changes.insert(field.to_string(), value.into());
        self
    }

    /// Apply the accumulated changes to the model and update the record.
    ///
    /// Field names accept either the Rust field name or the database column
    /// name. Returns an error when a name is unknown to the model.
    pub async fn save(self) -> Result<M> {
        let Self { mut model, changes } = self;
        serialization::apply_changes(&mut model, changes).map_err(Error::conversion)?;
        model.update().await
    }
}

/// Builder for on-conflict (upsert) operations.
///
/// Returned by [`Model::on_conflict`](crate::model::Model::on_conflict). The
/// conflict columns name a unique constraint or unique index; when the insert
/// collides with it the row is updated instead of failing.
///
/// By default every non-conflict column is overwritten. Narrow that with
/// [`OnConflictBuilder::update_columns`] or [`OnConflictBuilder::update_all_except`].
///
/// ```ignore
/// User::on_conflict(vec!["email"])
///     .update_all_except(vec!["created_at"])
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
    /// Everything else keeps the value already stored. Mutually exclusive with
    /// [`OnConflictBuilder::update_all_except`]; when both are set, this one wins.
    #[must_use]
    pub fn update_columns(mut self, columns: Vec<&str>) -> Self {
        self.update_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Overwrite every column on conflict except these.
    ///
    /// The usual reason to reach for this is to protect insert-only columns such
    /// as `created_at` while letting new values win everywhere else.
    #[must_use]
    pub fn update_all_except(mut self, columns: Vec<&str>) -> Self {
        self.exclude_columns = Some(columns.into_iter().map(|s| s.to_string()).collect());
        self
    }

    /// Run the upsert and return the stored row.
    ///
    /// The returned model reflects what the database ended up with, so it
    /// carries the existing primary key when the insert turned into an update.
    pub async fn insert(self, model: M) -> Result<M>
    where
        M: Sized,
    {
        M::__insert_with_conflict(model, self).await
    }
}
