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
    QuerySource, ensure_relation_configured, owner_is_unsaved, require_scalar_relation_key,
    required_key, where_key,
};
use super::state::{RelationState, relation_serde};

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
    /// The table of the model this relation sits on, which picks the owner's
    /// `MorphOne`/`MorphMany` naming the key its children hold.
    child_table: &'static str,
    type_value: Option<String>,
    id_value: Option<serde_json::Value>,
    source: QuerySource,
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

    /// Record the model this relation sits on. The derive does; without it,
    /// the owner is looked up by the key its first `MorphOne`/`MorphMany`
    /// with this relation's morph name names, else by its primary key.
    #[doc(hidden)]
    pub fn __on<Child: crate::model::ModelMeta>(mut self) -> Self {
        self.child_table = Child::table_name();
        self
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
        let key_column = match Related::__morph_owner_key(self.id_column, self.child_table) {
            Some(column) => column,
            None => match Related::primary_key_names() {
                [column] => column,
                _ => {
                    return Err(Error::query(format!(
                        "MorphTo::load_as: {} has a composite primary key, and no MorphOne or MorphMany of it names the key '{}' holds",
                        Related::table_name(),
                        self.id_column
                    )));
                }
            },
        };

        self.source
            .query::<Related>()
            .where_eq(key_column, id.clone())
            .first()
            .await
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.source.database = Some(database.clone());
    }

    /// The two column values are re-read from the model's own columns, so
    /// only where the relation queries carries over, and only while they are
    /// the same: keeping the previous values would resurrect a discriminator
    /// the columns no longer hold.
    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        #[cfg(feature = "entity-manager")]
        if self.type_value == previous.type_value && self.id_value == previous.id_value {
            self.source.preserve_from(&previous.source);
        }
        #[cfg(not(feature = "entity-manager"))]
        let _ = previous;
    }
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
            child_table: "",
            type_value: None,
            id_value: None,
            source: QuerySource::default(),
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

/// The query for the rows of a `MorphOne` or `MorphMany`: `_type` names the
/// owner's table and `_id` holds its key.
fn morph_query<Related: Model, C>(
    relation: &str,
    morph_name: &str,
    local_key: &str,
    parent_table: Option<&str>,
    state: &RelationState<C>,
    context: &str,
) -> Result<QueryBuilder<Related>> {
    ensure_relation_configured(relation, &[morph_name, local_key])?;
    let pk = required_key(&state.key, "Parent primary key", context)?;
    let table = parent_table.ok_or_else(|| Error::query("Parent table not set for relation"))?;

    Ok(where_key(
        state
            .source
            .query()
            .where_eq(format!("{morph_name}_type"), table),
        format!("{morph_name}_id"),
        pk,
    ))
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
    parent_table: Option<String>,
    state: RelationState<Box<Related>>,
}

impl<Related: Model> MorphOne<Related> {
    fn query(&self, context: &str) -> Result<QueryBuilder<Related>> {
        morph_query(
            "MorphOne",
            self.morph_name,
            self.local_key,
            self.parent_table.as_deref(),
            &self.state,
            context,
        )
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
        self.state.key = Some(pk);
        self.parent_table = Some(table);
        self
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.state.source.database = Some(database.clone());
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        self.state.preserve_from(
            &previous.state,
            self.morph_name == previous.morph_name
                && self.local_key == previous.local_key
                && self.parent_table == previous.parent_table
                && self.state.key == previous.state.key,
            owner_is_unsaved(&previous.state.key),
        );
    }

    /// Fetch the related row, returning `Ok(None)` when there is none.
    ///
    /// Queries whenever a connection is reachable. A cached row — from an eager
    /// load or from deserialization — is only served without one, so a payload
    /// that arrived in a request body is never reported as a stored row; read an
    /// eager-loaded row itself with [`get_cached`](Self::get_cached). An eager
    /// load that found no row is served as `Ok(None)`.
    pub async fn load(&self) -> Result<Option<Related>> {
        let configured = ensure_relation_configured("MorphOne", &[self.morph_name, self.local_key])
            .is_ok()
            && self.parent_table.is_some();
        if let Some(cached) = self.state.served(self.state.can_query(configured)) {
            return Ok(cached.map(|model| (**model).clone()));
        }

        self.query("MorphOne::load")?.first().await
    }

    /// The cached row, if one is present. Never queries and never awaits.
    pub fn get_cached(&self) -> Option<&Related> {
        self.state.cached().map(|model| &**model)
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<Related>) {
        self.state.set_cached(model.map(Box::new));
    }
}

impl<Related: Model> Default for MorphOne<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "",
            parent_table: None,
            state: RelationState::default(),
        }
    }
}

relation_serde!(MorphOne<Related>);

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
    parent_table: Option<String>,
    state: RelationState<Vec<Related>>,
}

impl<Related: Model> MorphMany<Related> {
    fn query(&self, context: &str) -> Result<QueryBuilder<Related>> {
        morph_query(
            "MorphMany",
            self.morph_name,
            self.local_key,
            self.parent_table.as_deref(),
            &self.state,
            context,
        )
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
        self.state.key = Some(pk);
        self.parent_table = Some(table);
        self
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.state.source.database = Some(database.clone());
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        self.state.preserve_from(
            &previous.state,
            self.morph_name == previous.morph_name
                && self.local_key == previous.local_key
                && self.parent_table == previous.parent_table
                && self.state.key == previous.state.key,
            owner_is_unsaved(&previous.state.key),
        );
    }

    /// Fetch every related row, in no particular order.
    ///
    /// Queries whenever a connection is reachable. Cached rows — from an eager
    /// load or from deserialization — are only served without one, so a payload
    /// that arrived in a request body is never reported as stored rows; read
    /// eager-loaded rows themselves with [`get_cached`](Self::get_cached).
    pub async fn load(&self) -> Result<Vec<Related>> {
        let configured =
            ensure_relation_configured("MorphMany", &[self.morph_name, self.local_key]).is_ok()
                && self.parent_table.is_some();
        if let Some(cached) = self.state.served(self.state.can_query(configured)) {
            return Ok(cached.cloned().unwrap_or_default());
        }

        self.query("MorphMany::load")?.get().await
    }

    /// Fetch related rows through a caller-supplied refinement of the query.
    ///
    /// The closure receives the query already filtered on both the type and id
    /// columns, so it should only add constraints. This is the way to order or
    /// page the rows. Never serves the cache.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<Related>>
    where
        F: FnOnce(QueryBuilder<Related>) -> QueryBuilder<Related> + Send,
    {
        constraint_fn(self.query("MorphMany::load_with")?)
            .get()
            .await
    }

    /// Count the related rows. Always queries; the cache is not consulted.
    pub async fn count(&self) -> Result<u64> {
        self.query("MorphMany::count")?.count().await
    }

    /// The cached rows, if any are present. Never queries and never awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded.
    pub fn get_cached(&self) -> Option<&[Related]> {
        self.state.cached().map(Vec::as_slice)
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<Related>) {
        self.state.set_cached(Some(models));
    }
}

impl<Related: Model> Default for MorphMany<Related> {
    fn default() -> Self {
        Self {
            morph_name: "",
            local_key: "",
            parent_table: None,
            state: RelationState::default(),
        }
    }
}

relation_serde!(MorphMany<Related>);
