//! Self-referencing relations — a table whose rows point at other rows of the
//! same table (parent/children, manager/reports, category trees).
//!
//! [`SelfRef`] walks up one level, [`SelfRefMany`] down one level, and
//! [`SelfRefMany::load_tree`] walks the whole subtree in a single recursive CTE.
//!
//! Like the direct wrappers, `load()` prefers the database whenever a
//! connection is reachable, so a payload that arrived by deserializing JSON
//! cannot pass itself off as the stored rows. Unlike them, neither wrapper has an
//! eager-loading path — `.with("..")` on one is an error pointing at the lazy
//! load — because an arbitrary-depth self-join has no fixed number of levels to
//! batch.

use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::query::QueryBuilder;

use super::helpers::{
    build_self_ref_tree_sql, is_null_key, require_scalar_relation_key, required_key, where_key,
};
use super::state::{RelationState, relation_serde};

/// `new`, `Default` (`parent_id`/`id`) and serde for [`SelfRef`] and
/// [`SelfRefMany`], which name the same two columns.
macro_rules! self_ref_relation {
    ($relation:ident) => {
        impl<E: Model> $relation<E> {
            /// Declare the two columns. Both name columns on the *same* table.
            #[must_use]
            pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
                Self {
                    foreign_key,
                    local_key,
                    state: RelationState::default(),
                }
            }
        }

        impl<E: Model> Default for $relation<E> {
            fn default() -> Self {
                Self::new("parent_id", "id")
            }
        }

        relation_serde!($relation<E>);
    };
}

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
/// [`load`](Self::load) queries whenever a connection is reachable, and this
/// relation has no eager path; see the module documentation.
#[derive(Debug, Clone)]
pub struct SelfRef<E: Model> {
    /// Column on this row holding the target row's key — the "points upward"
    /// column. Defaults to `parent_id`.
    pub foreign_key: &'static str,
    /// Column the foreign key is matched against, on the same table. Defaults to
    /// `id`.
    pub local_key: &'static str,
    state: RelationState<Box<E>>,
}

self_ref_relation!(SelfRef);

impl<E: Model> SelfRef<E> {
    /// Supply this row's [`foreign_key`](Self::foreign_key) value, which is what
    /// the parent is looked up by.
    ///
    /// A NULL is treated as "no parent" and short-circuits every method
    /// to `Ok(None)` / `Ok(false)` without querying — the behaviour that makes a
    /// root row terminate cleanly. Leaving this unset entirely does the same.
    /// Must be a scalar; composite keys are rejected at load time.
    #[doc(hidden)]
    pub fn with_fk_value(mut self, fk: impl Into<Value>) -> Self {
        self.state.key = Some(fk.into());
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        // The cache holds the parent, which a row not given a foreign key yet
        // is the caller's to link.
        self.state.preserve_from(
            &previous.state,
            self.foreign_key == previous.foreign_key && self.local_key == previous.local_key,
            previous.state.key.is_none(),
        );
    }

    /// The query for the parent row, or `None` at a root.
    fn query(&self, context: &str) -> Result<Option<QueryBuilder<E>>> {
        match &self.state.key {
            Some(fk) if !is_null_key(fk) => {
                let fk = require_scalar_relation_key(fk, context)?;
                Ok(Some(
                    self.state
                        .source
                        .query()
                        .where_eq(self.local_key, fk.clone()),
                ))
            }
            _ => Ok(None),
        }
    }

    /// Fetch the row this one points at, or `Ok(None)` at a root.
    ///
    /// Queries whenever a connection is reachable, ignoring any cached row; the
    /// cache is served only when there is no foreign-key value to look the
    /// parent up by, or no connection to look it up through. Loading a whole
    /// ancestor chain costs one query per level — for a subtree use
    /// [`SelfRefMany::load_tree`], which does it in one.
    pub async fn load(&self) -> Result<Option<E>> {
        let query = || self.query("SelfRef::load");
        self.state.load_one(true, query).await
    }

    /// Fetch the parent through a caller-supplied refinement of the query.
    ///
    /// Never serves the cache, so this is also the way to force a fresh read.
    /// Still short-circuits to `Ok(None)` at a root without calling the closure.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        match self.query("SelfRef::load_with")? {
            Some(query) => constraint_fn(query).first().await,
            None => Ok(None),
        }
    }

    /// Whether the referenced row exists. `Ok(false)` at a root, without a
    /// query.
    ///
    /// Always queries otherwise; the cache is not consulted. A non-null foreign
    /// key with `Ok(false)` here means the reference dangles.
    pub async fn exists(&self) -> Result<bool> {
        match self.query("SelfRef::exists")? {
            Some(query) => query.exists().await,
            None => Ok(false),
        }
    }

    crate::relations::state::one_row_cache!(E);
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
/// `load()` queries whenever a connection is reachable, and this relation has no
/// eager path; see the module documentation.
#[derive(Debug, Clone)]
pub struct SelfRefMany<E: Model> {
    /// Column on the child rows holding this row's key. Defaults to
    /// `parent_id`.
    pub foreign_key: &'static str,
    /// Column on this row that the children's foreign key matches. Defaults to
    /// `id`.
    pub local_key: &'static str,
    state: RelationState<Vec<E>>,
}

self_ref_relation!(SelfRefMany);

impl<E: Model> SelfRefMany<E> {
    /// Supply this row's [`local_key`](Self::local_key) value, which the
    /// children's foreign key is matched against, and whether the row is new
    /// (its `is_new()`), whose cached children carry over when saving it
    /// rebuilds the relation.
    ///
    /// Must be a scalar. Unlike `SelfRef::with_fk_value` there is no
    /// null-means-root short circuit here: leaving it unset makes every method
    /// error with "Parent primary key not set for relation".
    #[doc(hidden)]
    pub fn with_parent_pk(mut self, pk: impl Into<Value>, owner_is_new: bool) -> Self {
        self.state.set_owner_key(pk.into(), owner_is_new);
        self
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        self.state.preserve_from(
            &previous.state,
            self.foreign_key == previous.foreign_key && self.local_key == previous.local_key,
            previous.state.owner_is_new,
        );
    }

    fn parent_key(&self, context: &str) -> Result<&Value> {
        required_key(&self.state.key, "Parent primary key", context)
    }

    /// The query for the direct children.
    fn query(&self, context: &str) -> Result<QueryBuilder<E>> {
        let pk = self.parent_key(context)?;
        Ok(where_key(self.state.source.query(), self.foreign_key, pk))
    }

    /// Fetch the direct children — one level only, in no particular order.
    ///
    /// Queries whenever a connection is reachable, ignoring any cached rows; the
    /// cache is served only when there is nothing to query through. Recursing
    /// with this is one query per node; use [`load_tree`](Self::load_tree)
    /// instead.
    pub async fn load(&self) -> Result<Vec<E>> {
        let query = || self.query("SelfRefMany::load");
        self.state.load_many(true, query).await
    }

    /// Fetch the direct children through a caller-supplied refinement of the
    /// query — the way to order or page them. Never serves the cache.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        constraint_fn(self.query("SelfRefMany::load_with")?)
            .get()
            .await
    }

    /// Count the direct children. Always queries; one level only, so this is not
    /// the size of the subtree.
    pub async fn count(&self) -> Result<u64> {
        self.query("SelfRefMany::count")?.count().await
    }

    /// Whether this row has at least one direct child — i.e. whether it is a
    /// leaf. Cheaper than [`count`](Self::count).
    pub async fn exists(&self) -> Result<bool> {
        self.query("SelfRefMany::exists")?.exists().await
    }

    crate::relations::state::many_rows_cache!(E);

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

        let pk = self.parent_key("SelfRefMany::load_tree")?;
        let db = self.state.source.database()?;
        let (sql, params) = build_self_ref_tree_sql::<E>(
            self.foreign_key,
            self.local_key,
            pk,
            max_depth,
            db.backend(),
        )?;

        db.raw_models::<E>(&sql, params, crate::logging::EngineLog::On)
            .await
    }
}
