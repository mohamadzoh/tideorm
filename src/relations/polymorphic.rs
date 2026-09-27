//! Polymorphic relations: a link expressed as a `(type, id)` column pair rather
//! than a foreign key to one fixed table.
//!
//! [`MorphOne`] and [`MorphMany`] are the owning side; [`MorphTo`] is the
//! inverse, on the table that carries the two columns. The discriminator holds
//! the owner's **table name**, so `"users"`, not `"User"`.
//!
//! One behaviour differs from the direct wrappers: [`MorphTo`] has no
//! eager-loading path — its target type varies per row — so `.with("..")` on a
//! `MorphTo` field is a hard error pointing you at the lazy load.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{
    ensure_relation_configured, has_active_database, owner_is_unsaved, preserve_cached_value,
    require_scalar_relation_key, required_key, where_key,
};

/// The inverse side of a polymorphic relation: this model carries the
/// `(type, id)` pair and its owner could be any table.
///
/// ```ignore
/// #[tideorm(morph_name = "commentable")]
/// pub commentable: MorphTo<Post>,
/// ```
///
/// The wrapper type is what selects the relation kind — there is no `morph_to`
/// attribute — and `morph_name` is required. The model must actually declare the
/// `commentable_type` and `commentable_id` columns; the derive fails if either
/// is missing.
///
/// `Morphable` is only the *default* target — the one [`load`](Self::load)
/// resolves without being told. Rows pointing elsewhere are handled by reading
/// [`type_value`](Self::type_value) and calling
/// [`load_as::<T>()`](Self::load_as) for each candidate. Because that target
/// varies per row there is no eager path: `.with(..)` on a `MorphTo` field
/// errors rather than silently returning nothing.
///
/// Unlike the other wrappers it keeps no cache: every load queries, and
/// deserializing discards whatever payload it meets.
#[derive(Debug, Clone)]
pub struct MorphTo<Morphable> {
    /// Column on this model holding the owner's table name.
    pub type_column: &'static str,
    /// Column on this model holding the owner's key.
    pub id_column: &'static str,
    type_value: Option<String>,
    id_value: Option<serde_json::Value>,
    _morphable: PhantomData<Morphable>,
}

impl<Morphable> MorphTo<Morphable> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("MorphTo", &[self.type_column, self.id_column])
    }

    /// Declare which two columns carry the polymorphic link.
    ///
    /// Both must be non-empty; loading a wrapper built by [`Default`] fails with
    /// "MorphTo relation is not configured". Pair with
    /// [`with_values`](Self::with_values) to make it loadable — normally the
    /// derive does both, deriving the names from `morph_name` as
    /// `{morph_name}_type` / `{morph_name}_id`.
    pub fn new(type_column: &'static str, id_column: &'static str) -> Self {
        Self {
            type_column,
            id_column,
            ..Self::default()
        }
    }

    /// Supply the values read off this model's two columns.
    ///
    /// `type_value` is the owner's table name — the same string
    /// `Model::table_name()` returns — and `id_value` its key. Without both, the
    /// load methods have nothing to resolve.
    ///
    /// The type column may be nullable (`Option<String>`): a row with no owner
    /// holds `NULL` in both columns, and `load()` then returns `None`.
    pub fn with_values(
        mut self,
        type_value: impl Into<Option<String>>,
        id_value: serde_json::Value,
    ) -> Self {
        self.type_value = type_value.into();
        self.id_value = Some(id_value);
        self
    }

    /// The stored polymorphic type discriminator, which TideORM writes as the
    /// owner's table name.
    pub fn type_value(&self) -> Option<&str> {
        self.type_value.as_deref()
    }

    /// The stored polymorphic owner key.
    pub fn id_value(&self) -> Option<&serde_json::Value> {
        self.id_value.as_ref()
    }

    /// True when the stored discriminator names `Related`'s table.
    pub fn is_type<Related: Model>(&self) -> bool {
        self.type_value.as_deref() == Some(Related::table_name())
    }

    /// Load the polymorphic owner as `Related`.
    ///
    /// Returns `Ok(None)` when the stored discriminator names a different table,
    /// so a caller can try each type its `morph_type` column may hold. Also
    /// `Ok(None)` for a null or absent id. Always queries.
    pub async fn load_as<Related: Model>(&self) -> Result<Option<Related>> {
        self.ensure_configured()?;

        if !self.is_type::<Related>() {
            return Ok(None);
        }

        let id = match &self.id_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "MorphTo::load_as")?,
            _ => return Ok(None),
        };

        // The owner's `MorphOne`/`MorphMany` names the key its children hold.
        let key_column = match Related::__morph_owner_key(self.id_column) {
            Some(column) => column,
            None => match Related::primary_key_names() {
                [column] => column,
                _ => {
                    return Err(Error::invalid_query(format!(
                        "MorphTo::load_as: {} has a composite primary key, and no MorphOne or MorphMany of it names the key '{}' holds",
                        Related::table_name(),
                        self.id_column
                    )));
                }
            },
        };

        Related::query()
            .where_eq(key_column, id.clone())
            .first()
            .await
    }

    /// `MorphTo` carries no runtime state beyond the two column values, which
    /// the derive re-reads from the model's own columns, so there is nothing to
    /// carry over — keeping the previous values would resurrect a discriminator
    /// the columns no longer hold.
    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, _previous: &Self) {}
}

impl<Morphable: Model> MorphTo<Morphable> {
    /// Load the polymorphic owner.
    ///
    /// `Morphable` is the only target this relation resolves automatically. A row
    /// whose discriminator names another table is an error rather than a silent
    /// `None`; resolve heterogeneous owners with [`MorphTo::type_value`] and
    /// [`MorphTo::load_as`].
    pub async fn load(&self) -> Result<Option<Morphable>> {
        self.ensure_configured()?;

        let Some(type_value) = self.type_value.as_deref() else {
            // A NULL type column is a row with no owner; only a wrapper that
            // never received the columns has no id either.
            if self.id_value.is_some() {
                return Ok(None);
            }
            return Err(Error::query(format!(
                "MorphTo column '{}' holds no type value; rebuild the model through TideORM",
                self.type_column
            )));
        };

        if type_value != Morphable::table_name() {
            return Err(Error::query(format!(
                "MorphTo target type '{}' is not '{}'; use load_as::<T>() for heterogeneous owners",
                type_value,
                Morphable::table_name()
            )));
        }

        self.load_as::<Morphable>().await
    }
}

impl<Morphable> Default for MorphTo<Morphable> {
    fn default() -> Self {
        Self {
            type_column: "",
            id_column: "",
            type_value: None,
            id_value: None,
            _morphable: PhantomData,
        }
    }
}

impl<Morphable> Serialize for MorphTo<Morphable> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_none()
    }
}

impl<'de, Morphable> Deserialize<'de> for MorphTo<Morphable> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // The payload must still be consumed, or a streaming deserializer
        // desyncs part way through the surrounding struct.
        serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(Self::default())
    }
}

/// The owning side of a polymorphic one-to-one relation: at most one row of
/// `Related` whose `(type, id)` pair points back at this model.
///
/// ```ignore
/// #[tideorm(morph_name = "imageable")]
/// pub image: MorphOne<Image>,
/// ```
///
/// The wrapper type selects the relation kind; only `morph_name` is required
/// (plus `local_key`, which defaults to `"id"`).
///
/// The column names are derived from [`morph_name`](Self::morph_name), not
/// stored: `{morph_name}_type` and `{morph_name}_id` on `Related`'s table. The
/// type column is matched against this model's table name.
///
/// Like [`HasOne`](crate::relations::HasOne), [`load`](Self::load) prefers the
/// database over a cached row. Eager loading issues one `WHERE .. IN (..)` per
/// level.
#[derive(Debug, Clone)]
pub struct MorphOne<Related: Model> {
    /// Prefix the two polymorphic columns on `Related` are named after:
    /// `{morph_name}_type` and `{morph_name}_id`.
    pub morph_name: &'static str,
    /// Column on this model whose value the `_id` column holds; normally its
    /// primary key.
    pub local_key: &'static str,
    cached: Option<Box<Related>>,
    parent_pk: Option<serde_json::Value>,
    parent_table: Option<String>,
}

impl<Related: Model> MorphOne<Related> {
    /// The query for the related row: `_type` names this model's table, `_id`
    /// holds its key.
    fn query(&self, context: &str) -> Result<QueryBuilder<Related>> {
        ensure_relation_configured("MorphOne", &[self.morph_name, self.local_key])?;
        let pk = required_key(&self.parent_pk, "Parent primary key", context)?;
        let table = self
            .parent_table
            .as_deref()
            .ok_or_else(|| Error::query("Parent table not set for relation"))?;

        Ok(where_key(
            Related::query().where_eq(format!("{}_type", self.morph_name), table),
            format!("{}_id", self.morph_name),
            pk,
        ))
    }

    /// Declare the morph prefix and the local key column.
    ///
    /// Both must be non-empty; loading a wrapper built by [`Default`] fails with
    /// "MorphOne relation is not configured". Pair with
    /// [`with_parent`](Self::with_parent) to make it loadable.
    pub fn new(morph_name: &'static str, local_key: &'static str) -> Self {
        Self {
            morph_name,
            local_key,
            ..Self::default()
        }
    }

    /// Supply both halves of the polymorphic key: this model's
    /// [`local_key`](Self::local_key) value and its table name, which is what
    /// the `_type` column is matched against.
    ///
    /// The pk must be a scalar. Without this the wrapper is inert — loading
    /// errors with "Parent primary key not set for relation".
    pub fn with_parent(mut self, pk: serde_json::Value, table: String) -> Self {
        self.parent_pk = Some(pk);
        self.parent_table = Some(table);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            owner_is_unsaved(&previous.parent_pk),
            self.morph_name == previous.morph_name
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk
                && self.parent_table == previous.parent_table,
        );
    }

    /// Fetch the related row, returning `Ok(None)` when there is none.
    ///
    /// Queries whenever a connection is reachable. A cached row — from an eager
    /// load or from deserialization — is only served without one, so a payload
    /// that arrived in a request body is never reported as a stored row; read an
    /// eager-loaded row itself with [`get_cached`](Self::get_cached).
    pub async fn load(&self) -> Result<Option<Related>> {
        let can_query =
            self.parent_pk.is_some() && self.parent_table.is_some() && has_active_database();
        if let Some(cached) = &self.cached
            && !can_query
        {
            return Ok(Some((**cached).clone()));
        }

        self.query("MorphOne::load")?.first().await
    }

    /// The cached row, if one is present. Never queries and never awaits.
    pub fn get_cached(&self) -> Option<&Related> {
        self.cached.as_deref()
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<Related>) {
        self.cached = model.map(Box::new);
    }
}

impl<Related: Model> Default for MorphOne<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            parent_table: None,
        }
    }
}

impl<Related: Model> Serialize for MorphOne<Related> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model> Deserialize<'de> for MorphOne<Related> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Related>::deserialize(deserializer)?;
        Ok(Self {
            cached: cached.map(Box::new),
            ..Self::default()
        })
    }
}

/// The owning side of a polymorphic one-to-many relation: every row of
/// `Related` whose `(type, id)` pair points back at this model.
///
/// ```ignore
/// #[tideorm(morph_name = "commentable")]
/// pub comments: MorphMany<Comment>,
/// ```
///
/// Same column convention as [`MorphOne`] — `{morph_name}_type` and
/// `{morph_name}_id` on `Related`'s table, with the type column matched against
/// this model's table name — and the same [`load`](Self::load) behaviour.
#[derive(Debug, Clone)]
pub struct MorphMany<Related: Model> {
    /// Prefix the two polymorphic columns on `Related` are named after:
    /// `{morph_name}_type` and `{morph_name}_id`.
    pub morph_name: &'static str,
    /// Column on this model whose value the `_id` column holds; normally its
    /// primary key.
    pub local_key: &'static str,
    cached: Option<Vec<Related>>,
    parent_pk: Option<serde_json::Value>,
    parent_table: Option<String>,
}

impl<Related: Model> MorphMany<Related> {
    /// The query for the related rows: `_type` names this model's table, `_id`
    /// holds its key.
    fn query(&self, context: &str) -> Result<QueryBuilder<Related>> {
        ensure_relation_configured("MorphMany", &[self.morph_name, self.local_key])?;
        let pk = required_key(&self.parent_pk, "Parent primary key", context)?;
        let table = self
            .parent_table
            .as_deref()
            .ok_or_else(|| Error::query("Parent table not set for relation"))?;

        Ok(where_key(
            Related::query().where_eq(format!("{}_type", self.morph_name), table),
            format!("{}_id", self.morph_name),
            pk,
        ))
    }

    /// Declare the morph prefix and the local key column.
    ///
    /// Both must be non-empty; every method rejects a wrapper built by
    /// [`Default`] with "MorphMany relation is not configured". Pair with
    /// [`with_parent`](Self::with_parent) to make it loadable.
    pub fn new(morph_name: &'static str, local_key: &'static str) -> Self {
        Self {
            morph_name,
            local_key,
            ..Self::default()
        }
    }

    /// Supply both halves of the polymorphic key: this model's
    /// [`local_key`](Self::local_key) value and its table name, which is what
    /// the `_type` column is matched against.
    ///
    /// The pk must be a scalar. Without this the wrapper is inert — every query
    /// method errors with "Parent primary key not set for relation".
    pub fn with_parent(mut self, pk: serde_json::Value, table: String) -> Self {
        self.parent_pk = Some(pk);
        self.parent_table = Some(table);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            owner_is_unsaved(&previous.parent_pk),
            self.morph_name == previous.morph_name
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk
                && self.parent_table == previous.parent_table,
        );
    }

    /// Fetch every related row, in no particular order.
    ///
    /// Queries whenever a connection is reachable. Cached rows — from an eager
    /// load or from deserialization — are only served without one, so a payload
    /// that arrived in a request body is never reported as stored rows; read
    /// eager-loaded rows themselves with [`get_cached`](Self::get_cached).
    pub async fn load(&self) -> Result<Vec<Related>> {
        let can_query =
            self.parent_pk.is_some() && self.parent_table.is_some() && has_active_database();
        if let Some(cached) = &self.cached
            && !can_query
        {
            return Ok(cached.clone());
        }

        self.query("MorphMany::load")?.get().await
    }

    /// Fetch related rows through a caller-supplied refinement of the query.
    ///
    /// The closure receives the query already filtered on both the type and id
    /// columns, so it should only add constraints. This is the way to order or
    /// page a morph relation, and unlike [`load`](Self::load) it never serves the
    /// cache.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        constraint_fn(self.query("MorphMany::load_with")?)
            .get()
            .await
    }

    /// Count related rows in the database without materializing them.
    ///
    /// Always queries, so this can legitimately disagree with
    /// `get_cached().len()` when the cache is stale.
    pub async fn count(&self) -> Result<u64> {
        self.query("MorphMany::count")?.count().await
    }

    /// The cached rows, if this relation was populated. Never queries and never
    /// awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded — and it is the only state in which [`load`](Self::load) will
    /// query.
    pub fn get_cached(&self) -> Option<&[Related]> {
        self.cached.as_deref()
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<Related>) {
        self.cached = Some(models);
    }
}

impl<Related: Model> Default for MorphMany<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            parent_table: None,
        }
    }
}

impl<Related: Model> Serialize for MorphMany<Related> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Related: Model> Deserialize<'de> for MorphMany<Related> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Vec<Related>>::deserialize(deserializer)?;
        Ok(Self {
            cached,
            ..Self::default()
        })
    }
}
