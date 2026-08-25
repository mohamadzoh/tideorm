//! Polymorphic relations: a link expressed as a `(type, id)` column pair rather
//! than a foreign key to one fixed table.
//!
//! [`MorphOne`] and [`MorphMany`] are the owning side; [`MorphTo`] is the
//! inverse, on the table that carries the two columns. The discriminator holds
//! the owner's **table name**, so `"users"`, not `"User"`.
//!
//! Two behaviours differ from the direct wrappers and are worth reading before
//! use. First, `load()` here is **cache-first**: a cached value is returned
//! without consulting the database, including one that arrived by deserializing
//! JSON. Second, [`MorphTo`] has no eager-loading path — its target type varies
//! per row — so `.with("..")` on a `MorphTo` field is a hard error pointing you
//! at the lazy load.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{cached_ref, ensure_relation_configured, preserve_cached_value};
use super::require_scalar_relation_key;

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
/// [`load_as::<T>()`](Self::load_as) for each candidate, optionally collecting
/// the answer into a [`MorphResult`]. Because that target varies per row there
/// is no eager path: `.with(..)` on a `MorphTo` field errors rather than
/// silently returning nothing.
///
/// Serialization is asymmetric here, unlike the other wrappers: the cached owner
/// is written out, but deserializing discards it entirely (the payload is
/// consumed and ignored, since `Morphable` need not be `Deserialize`). A
/// `MorphTo` restored from JSON always starts empty and needs
/// [`refresh_runtime_relations_from`](crate::internal::InternalModel::refresh_runtime_relations_from)
/// to become loadable again.
#[derive(Debug, Clone)]
pub struct MorphTo<Morphable> {
    /// Column on this model holding the owner's table name.
    pub type_column: &'static str,
    /// Column on this model holding the owner's key.
    pub id_column: &'static str,
    type_value: Option<String>,
    id_value: Option<serde_json::Value>,
    cached: Option<Box<Morphable>>,
    _marker: PhantomData<Morphable>,
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
            type_value: None,
            id_value: None,
            cached: None,
            _marker: PhantomData,
        }
    }

    /// Supply the values read off this model's two columns.
    ///
    /// `type_value` is the owner's table name — the same string
    /// `Model::table_name()` returns — and `id_value` its key. Without both, the
    /// load methods have nothing to resolve.
    pub fn with_values(mut self, type_value: String, id_value: serde_json::Value) -> Self {
        self.type_value = Some(type_value);
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
    /// `Ok(None)` for a null or absent id.
    ///
    /// Always queries — the cache is never consulted, since a cached owner has
    /// no type the caller can check against `Related`.
    pub async fn load_as<Related: Model>(&self) -> Result<Option<Related>> {
        self.ensure_configured()?;

        if !self.is_type::<Related>() {
            return Ok(None);
        }

        let id = match &self.id_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "MorphTo::load_as")?,
            _ => return Ok(None),
        };

        Related::query()
            .where_eq(Related::primary_key_name(), id.clone())
            .first()
            .await
    }

    /// The cached owner, if one was set. Never queries and never awaits.
    ///
    /// Always `None` on a wrapper restored from JSON, since deserialization
    /// discards the payload.
    pub fn get_cached(&self) -> Option<&Morphable> {
        cached_ref(&self.cached)
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<Morphable>) {
        self.cached = model.map(Box::new);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self)
    where
        Morphable: Clone,
    {
        let same_relation =
            self.type_column == previous.type_column && self.id_column == previous.id_column;
        if same_relation {
            self.type_value = previous.type_value.clone();
            self.id_value = previous.id_value.clone();
        }

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.type_value.is_none() && previous.id_value.is_none(),
            same_relation,
        );
    }
}

impl<Morphable: Model> MorphTo<Morphable> {
    /// Load the polymorphic owner.
    ///
    /// `Morphable` is the only target this relation resolves automatically. A row
    /// whose discriminator names another table is an error rather than a silent
    /// `None`; resolve heterogeneous owners with [`MorphTo::type_value`] and
    /// [`MorphTo::load_as`].
    ///
    /// Cache-first: a cached owner is returned without touching the database,
    /// and only an empty cache triggers a query.
    pub async fn load(&self) -> Result<Option<Morphable>> {
        if let Some(cached) = &self.cached {
            return Ok(Some((**cached).clone()));
        }

        self.ensure_configured()?;

        let Some(type_value) = self.type_value.as_deref() else {
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
            cached: None,
            _marker: PhantomData,
        }
    }
}

impl<Morphable: Serialize> Serialize for MorphTo<Morphable> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, Morphable> Deserialize<'de> for MorphTo<Morphable> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // The cached owner is runtime-only state — `Morphable` is not required to
        // be deserializable — but the payload must still be consumed or a
        // streaming deserializer desyncs part way through the surrounding struct.
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
/// Unlike [`HasOne`](crate::relations::HasOne), [`load`](Self::load) is
/// **cache-first** — a cached row wins over the database, including one that
/// arrived by deserializing JSON. Eager loading is supported and issues one
/// `WHERE .. IN (..)` per level.
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
    _marker: PhantomData<Related>,
}

impl<Related: Model> MorphOne<Related> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("MorphOne", &[self.morph_name, self.local_key])
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
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
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
            previous.parent_pk.is_none() && previous.parent_table.is_none(),
            self.morph_name == previous.morph_name
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk
                && self.parent_table == previous.parent_table,
        );
    }

    /// Fetch the related row, returning `Ok(None)` when there is none.
    ///
    /// Cache-first: if a row is cached — from an eager load or from
    /// deserialization — it is cloned out and no query runs, even with a live
    /// connection. Call it on a freshly loaded model, or go through
    /// `Related::query()` directly, when you need a guaranteed round trip.
    pub async fn load(&self) -> Result<Option<Related>> {
        if let Some(cached) = &self.cached {
            return Ok(Some((**cached).clone()));
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphOne::load")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .first()
            .await
    }

    /// The cached row, if one is present. Never queries and never awaits — and
    /// since [`load`](Self::load) is cache-first, a `Some` here is exactly what
    /// `load()` would hand back.
    pub fn get_cached(&self) -> Option<&Related> {
        cached_ref(&self.cached)
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
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize> Serialize for MorphOne<Related> {
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
/// this model's table name — and the same cache-first
/// [`load`](Self::load) behaviour.
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
    _marker: PhantomData<Related>,
}

impl<Related: Model> MorphMany<Related> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("MorphMany", &[self.morph_name, self.local_key])
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
            cached: None,
            parent_pk: None,
            parent_table: None,
            _marker: PhantomData,
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
            previous.parent_pk.is_none() && previous.parent_table.is_none(),
            self.morph_name == previous.morph_name
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk
                && self.parent_table == previous.parent_table,
        );
    }

    /// Fetch all related rows, in no particular order.
    ///
    /// Cache-first: if rows are cached — from an eager load or from
    /// deserialization — they are cloned out and no query runs, even with a live
    /// connection. [`load_with`](Self::load_with) and [`count`](Self::count)
    /// always query, so reach for those when a round trip must happen.
    pub async fn load(&self) -> Result<Vec<Related>> {
        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphMany::load")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .get()
            .await
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
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphMany::load_with")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        let query = Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone());

        constraint_fn(query).get().await
    }

    /// Count related rows in the database without materializing them.
    ///
    /// Always queries, so this can legitimately disagree with
    /// `get_cached().len()` when the cache is stale.
    pub async fn count(&self) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "MorphMany::count")?;
        let table = self
            .parent_table
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent table not set for relation")))?;

        let type_column = format!("{}_type", self.morph_name);
        let id_column = format!("{}_id", self.morph_name);

        Related::query()
            .where_eq(&type_column, table.clone())
            .where_eq(&id_column, pk.clone())
            .count()
            .await
    }

    /// The cached rows, if this relation was populated. Never queries and never
    /// awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded — and it is the only state in which [`load`](Self::load) will
    /// query.
    pub fn get_cached(&self) -> Option<&[Related]> {
        cached_ref(&self.cached)
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
            _marker: PhantomData,
        }
    }
}

impl<Related: Model + Serialize> Serialize for MorphMany<Related> {
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

/// A resolved [`MorphTo`] owner narrowed to one of two candidate types.
///
/// Nothing builds this for you: `MorphTo` resolves one type at a time through
/// [`load_as`](MorphTo::load_as), and this is the container to fold those
/// attempts into when a column is known to hold one of a small, fixed set of
/// tables. It exists so callers can pass a resolved owner around as a single
/// value with a `match` at the far end, instead of a tuple of `Option`s.
///
/// See [`MorphResult3`] and [`MorphResult4`] for three and four candidates.
#[derive(Debug, Clone)]
pub enum MorphResult<A, B> {
    /// The owner resolved as the first candidate type.
    TypeA(A),
    /// The owner resolved as the second candidate type.
    TypeB(B),
    /// The discriminator matched neither candidate. Carries whatever the caller
    /// chose to keep — typically the raw type/id values — so an unexpected table
    /// name can be reported rather than lost.
    Unknown(serde_json::Value),
}

impl<A, B> MorphResult<A, B> {
    /// Whether this resolved as the first candidate type.
    pub fn is_type_a(&self) -> bool {
        matches!(self, MorphResult::TypeA(_))
    }

    /// Whether this resolved as the second candidate type.
    pub fn is_type_b(&self) -> bool {
        matches!(self, MorphResult::TypeB(_))
    }

    /// Whether the discriminator matched neither candidate.
    pub fn is_unknown(&self) -> bool {
        matches!(self, MorphResult::Unknown(_))
    }

    /// Borrow the first candidate, or `None` for any other variant.
    pub fn as_type_a(&self) -> Option<&A> {
        match self {
            MorphResult::TypeA(a) => Some(a),
            _ => None,
        }
    }

    /// Borrow the second candidate, or `None` for any other variant.
    pub fn as_type_b(&self) -> Option<&B> {
        match self {
            MorphResult::TypeB(b) => Some(b),
            _ => None,
        }
    }

    /// Take the first candidate by value, discarding any other variant.
    pub fn into_type_a(self) -> Option<A> {
        match self {
            MorphResult::TypeA(a) => Some(a),
            _ => None,
        }
    }

    /// Take the second candidate by value, discarding any other variant.
    pub fn into_type_b(self) -> Option<B> {
        match self {
            MorphResult::TypeB(b) => Some(b),
            _ => None,
        }
    }
}

/// [`MorphResult`] widened to three candidate types.
///
/// Deliberately bare: it carries no accessor helpers, so consume it with a
/// `match`.
#[derive(Debug, Clone)]
pub enum MorphResult3<A, B, C> {
    /// The owner resolved as the first candidate type.
    TypeA(A),
    /// The owner resolved as the second candidate type.
    TypeB(B),
    /// The owner resolved as the third candidate type.
    TypeC(C),
    /// The discriminator matched none of the candidates.
    Unknown(serde_json::Value),
}

/// [`MorphResult`] widened to four candidate types.
///
/// Deliberately bare: it carries no accessor helpers, so consume it with a
/// `match`.
#[derive(Debug, Clone)]
pub enum MorphResult4<A, B, C, D> {
    /// The owner resolved as the first candidate type.
    TypeA(A),
    /// The owner resolved as the second candidate type.
    TypeB(B),
    /// The owner resolved as the third candidate type.
    TypeC(C),
    /// The owner resolved as the fourth candidate type.
    TypeD(D),
    /// The discriminator matched none of the candidates.
    Unknown(serde_json::Value),
}
