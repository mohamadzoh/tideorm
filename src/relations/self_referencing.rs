//! Self-referencing relations — a table whose rows point at other rows of the
//! same table (parent/children, manager/reports, category trees).
//!
//! [`SelfRef`] walks up one level, [`SelfRefMany`] down one level, and
//! [`SelfRefMany::load_tree`] walks the whole subtree in a single recursive CTE.
//!
//! Two behaviours differ from the direct wrappers. `load()` here is
//! **cache-first**: a cached value is returned without consulting the database,
//! including one that arrived by deserializing JSON. And neither wrapper has an
//! eager-loading path — `.with("..")` on one is an error pointing at the lazy
//! load — because an arbitrary-depth self-join has no fixed number of levels to
//! batch.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::marker::PhantomData;

use crate::error::{Error, Result};
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{build_self_ref_tree_sql, cached_ref, preserve_cached_value};
use super::require_scalar_relation_key;

/// The upward half of a self-referencing relation: the single row of the same
/// table that this row points at, such as a node's parent.
///
/// ```ignore
/// #[tideorm(foreign_key = "parent_id")]
/// pub parent: SelfRef<Category>,
/// ```
///
/// The wrapper type is what selects the relation kind — there is no `self_ref`
/// attribute — and both key attributes are optional, defaulting to
/// `parent_id`/`id`.
///
/// A root row — one whose foreign key is `NULL` — yields `Ok(None)` from every
/// method rather than an error, so walking upward terminates naturally.
/// [`Default`] uses `parent_id`/`id`, which is why (unlike the direct wrappers)
/// this type has no "not configured" error.
///
/// [`load`](Self::load) is cache-first, and this relation has no eager path; see
/// the module documentation.
#[derive(Debug, Clone)]
pub struct SelfRef<E: Model> {
    /// Column on this row holding the target row's key — the "points upward"
    /// column. Defaults to `parent_id`.
    pub foreign_key: &'static str,
    /// Column the foreign key is matched against, on the same table. Defaults to
    /// `id`.
    pub local_key: &'static str,
    cached: Option<Box<E>>,
    fk_value: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> SelfRef<E> {
    /// Declare the two columns. Both name columns on the *same* table.
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }

    /// Supply this row's [`foreign_key`](Self::foreign_key) value, which is what
    /// the parent is looked up by.
    ///
    /// A JSON `null` is treated as "no parent" and short-circuits every method
    /// to `Ok(None)` / `Ok(false)` without querying — the behaviour that makes a
    /// root row terminate cleanly. Leaving this unset entirely does the same.
    /// Must be a scalar; composite keys are rejected at load time.
    pub fn with_fk_value(mut self, fk: serde_json::Value) -> Self {
        self.fk_value = Some(fk);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.fk_value.is_none(),
            self.foreign_key == previous.foreign_key
                && self.local_key == previous.local_key
                && self.fk_value == previous.fk_value,
        );
    }

    /// Fetch the row this one points at, or `Ok(None)` at a root.
    ///
    /// Cache-first: a cached row is cloned out without touching the database,
    /// even with a live connection. Loading a whole ancestor chain therefore
    /// costs one query per level — for a subtree use
    /// [`SelfRefMany::load_tree`], which does it in one.
    pub async fn load(&self) -> Result<Option<E>> {
        if let Some(cached) = &self.cached {
            return Ok(Some((**cached).clone()));
        }

        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "SelfRef::load")?,
            _ => return Ok(None),
        };

        E::query()
            .where_eq(self.local_key, fk.clone())
            .first()
            .await
    }

    /// Fetch the parent through a caller-supplied refinement of the query.
    ///
    /// Never serves the cache, so this is also the way to force a fresh read.
    /// Still short-circuits to `Ok(None)` at a root without calling the closure.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "SelfRef::load_with")?,
            _ => return Ok(None),
        };

        let query = E::query().where_eq(self.local_key, fk.clone());
        constraint_fn(query).first().await
    }

    /// Whether the referenced row exists. `Ok(false)` at a root, without a
    /// query.
    ///
    /// Always queries otherwise; the cache is not consulted. A non-null foreign
    /// key with `Ok(false)` here means the reference dangles.
    pub async fn exists(&self) -> Result<bool> {
        let fk = match &self.fk_value {
            Some(v) if !v.is_null() => require_scalar_relation_key(v, "SelfRef::exists")?,
            _ => return Ok(false),
        };

        E::query()
            .where_eq(self.local_key, fk.clone())
            .exists()
            .await
    }

    /// The cached parent, if one is present. Never queries and never awaits —
    /// and since [`load`](Self::load) is cache-first, a `Some` here is exactly
    /// what `load()` would hand back.
    pub fn get_cached(&self) -> Option<&E> {
        cached_ref(&self.cached)
    }
}

impl<E: Model> Default for SelfRef<E> {
    fn default() -> Self {
        Self {
            foreign_key: "parent_id",
            local_key: "id",
            cached: None,
            fk_value: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for SelfRef<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for SelfRef<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<E>::deserialize(deserializer)?;
        Ok(Self {
            cached: cached.map(Box::new),
            ..Self::default()
        })
    }
}

/// The downward half of a self-referencing relation: every row of the same table
/// that points at this one, such as a node's direct children.
///
/// ```ignore
/// #[tideorm(foreign_key = "parent_id")]
/// pub children: SelfRefMany<Category>,
/// ```
///
/// The inverse of [`SelfRef`], sharing the same two column names read in the
/// other direction, and defaulting to `parent_id`/`id` likewise.
/// [`load`](Self::load) returns *one* level; [`load_tree`](Self::load_tree)
/// returns the whole subtree in a single query.
///
/// `load()` is cache-first, and this relation has no eager path; see the module
/// documentation.
#[derive(Debug, Clone)]
pub struct SelfRefMany<E: Model> {
    /// Column on the child rows holding this row's key. Defaults to
    /// `parent_id`.
    pub foreign_key: &'static str,
    /// Column on this row that the children's foreign key matches. Defaults to
    /// `id`.
    pub local_key: &'static str,
    cached: Option<Vec<E>>,
    parent_pk: Option<serde_json::Value>,
    _marker: PhantomData<E>,
}

impl<E: Model> SelfRefMany<E> {
    /// Declare the two columns. Both name columns on the *same* table.
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }

    /// Supply this row's [`local_key`](Self::local_key) value, which the
    /// children's foreign key is matched against.
    ///
    /// Must be a scalar. Unlike [`SelfRef::with_fk_value`] there is no
    /// null-means-root short circuit here: leaving it unset makes every method
    /// error with "Parent primary key not set for self-reference".
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            self.foreign_key == previous.foreign_key
                && self.local_key == previous.local_key
                && self.parent_pk == previous.parent_pk,
        );
    }

    /// Fetch the direct children — one level only, in no particular order.
    ///
    /// Cache-first: cached rows are cloned out without touching the database.
    /// Recursing with this is one query per node; use
    /// [`load_tree`](Self::load_tree) instead.
    pub async fn load(&self) -> Result<Vec<E>> {
        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::load")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .get()
            .await
    }

    /// Fetch the direct children through a caller-supplied refinement of the
    /// query — the way to order or page them. Never serves the cache.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::load_with")?;

        let query = E::query().where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).get().await
    }

    /// Count the direct children. Always queries; one level only, so this is not
    /// the size of the subtree.
    pub async fn count(&self) -> Result<u64> {
        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::count")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .count()
            .await
    }

    /// Whether this row has at least one direct child — i.e. whether it is a
    /// leaf. Cheaper than [`count`](Self::count).
    pub async fn exists(&self) -> Result<bool> {
        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::exists")?;

        E::query()
            .where_eq(self.foreign_key, pk.clone())
            .exists()
            .await
    }

    /// The cached direct children, if populated. Never queries and never awaits.
    ///
    /// `Some(&[])` means "loaded, and this is a leaf"; `None` means nothing was
    /// ever loaded — and it is the only state in which [`load`](Self::load) will
    /// query.
    pub fn get_cached(&self) -> Option<&[E]> {
        cached_ref(&self.cached)
    }

    /// Fetch the whole subtree below this row in one recursive-CTE query,
    /// stopping after `max_depth` levels.
    ///
    /// Rows come back ordered by depth — direct children first — but flat: the
    /// parent/child structure is not reconstructed, so rebuild it from the rows'
    /// own foreign keys. `max_depth == 0` returns an empty `Vec` without
    /// querying, and depth `1` is equivalent to [`load`](Self::load).
    ///
    /// Each node appears exactly once even when the foreign keys contain a cycle
    /// (`a -> b -> a`), kept at the shallowest depth it was reached at. Soft
    /// deletes are honoured at every level, so a trashed node also prunes its
    /// descendants. Unlike the other methods this needs a live connection
    /// unconditionally — there is no cache path — and it bypasses the query
    /// builder, so a nonexistent `foreign_key`/`local_key` is rejected here
    /// rather than silently rendered.
    pub async fn load_tree(&self, max_depth: usize) -> Result<Vec<E>> {
        if max_depth == 0 {
            return Ok(Vec::new());
        }

        let pk = self.parent_pk.as_ref().ok_or_else(|| {
            Error::query(String::from(
                "Parent primary key not set for self-reference",
            ))
        })?;
        let pk = require_scalar_relation_key(pk, "SelfRefMany::load_tree")?;

        let db = crate::database::__current_db()?;
        let (sql, params) = build_self_ref_tree_sql::<E>(
            self.foreign_key,
            self.local_key,
            pk,
            max_depth,
            db.backend(),
        )?;

        db.__raw_with_params::<E>(&sql, params).await
    }
}

impl<E: Model> Default for SelfRefMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "parent_id",
            local_key: "id",
            cached: None,
            parent_pk: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for SelfRefMany<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for SelfRefMany<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<Vec<E>>::deserialize(deserializer)?;
        Ok(Self {
            cached,
            ..Self::default()
        })
    }
}
