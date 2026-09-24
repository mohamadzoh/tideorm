//! Eager loading: resolve a model's relations in a bounded number of queries
//! instead of one per parent row.
//!
//! The entry point is `Model::query().with("posts")`, which hands the
//! [`QueryBuilder`] to an [`EagerQueryBuilder`]. Terminals return
//! [`WithRelations`] rather than `M`, because the resolved payloads are
//! recorded alongside the model as well as pushed into its relation wrappers.
//!
//! Relation names are dotted paths (`"posts.comments"`), parsed by
//! [`RelationPath`] and merged into a [`RelationTree`] so that overlapping
//! requests share one load. Each level is loaded across *all* parents at once —
//! nested levels included — so depth costs queries, breadth does not.
//!
//! Two things are worth knowing up front: eager loading applies the related
//! model's own soft-delete scope, so trashed rows stay hidden exactly as
//! `HasMany::load()` hides them; and `MorphTo` has no eager path at all, since
//! its target table varies per row. Asking for one is an error naming the
//! limitation, not a silent empty result.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::columns::IntoColumnName;
use crate::error::{Error, Result};
use crate::internal::InternalModel;
use crate::model::Model;
use crate::query::{Order, QueryBuilder};

fn apply_primary_key_filter<M: Model>(
    mut query: QueryBuilder<M>,
    primary_key: &M::PrimaryKey,
) -> Result<QueryBuilder<M>> {
    let values = match serde_json::to_value(primary_key)
        .map_err(|e| Error::conversion(format!("Failed to serialize primary key: {}", e)))?
    {
        serde_json::Value::Array(values) => values,
        value => vec![value],
    };

    let columns = M::primary_key_names();
    if values.len() != columns.len() {
        return Err(Error::invalid_query(format!(
            "Primary key value for {} did not match declared key columns",
            M::table_name()
        )));
    }

    for (column, value) in columns.iter().zip(values) {
        query = query.where_eq(*column, value);
    }

    Ok(query)
}

/// A model plus the relation payloads an eager load resolved for it.
///
/// This is what every [`EagerQueryBuilder`] terminal returns. The relations are
/// recorded twice on purpose: pushed into the model's own wrappers (so
/// `entry.posts.get_cached()` works) *and* kept here as JSON keyed by relation
/// name, which is what makes a relation loadable-and-inspectable without knowing
/// its Rust type. [`get_relation`](Self::get_relation) reads the JSON copy.
///
/// Only relations actually named in the `.with(..)` calls appear; a declared but
/// unrequested relation is simply absent, so [`has_relation`](Self::has_relation)
/// answers "was this eager loaded", not "does this model declare it".
///
/// It derefs to `M`, so model methods and fields are reachable directly. Use
/// [`into_inner`](Self::into_inner) when you want the plain model back — the
/// wrappers keep their cached contents, so nothing is lost but the JSON map.
/// Serialization flattens the model, and the `relations` key disappears entirely
/// when empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithRelations<M> {
    /// The model itself. Also reachable through `Deref`/`DerefMut`.
    #[serde(flatten)]
    pub model: M,
    // `default` pairs with `skip_serializing_if`: without it the type cannot
    // deserialize its own output, because an eager load that resolved nothing
    // omits the key entirely.
    /// Resolved relation payloads, keyed by the relation's field name. Only the
    /// relations the eager load was asked for are present.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub relations: HashMap<String, serde_json::Value>,
}

impl<M: Model> WithRelations<M> {
    /// Wrap a model with no relations resolved yet.
    ///
    /// The eager loader calls this before running, then fills the map in. Calling
    /// it yourself gives a `WithRelations` whose `relations` is empty — it does
    /// not trigger any loading.
    pub fn new(model: M) -> Self {
        Self {
            model,
            relations: HashMap::new(),
        }
    }

    /// Record an eagerly-loaded relation payload under `name`.
    ///
    /// The derive's eager loader calls this for every relation it resolves, so
    /// [`has_relation`](Self::has_relation) and [`get_relation`](Self::get_relation)
    /// reflect what a completed eager load actually fetched.
    pub fn set_relation<T: Serialize>(&mut self, name: &str, value: &T) -> Result<()> {
        let value = serde_json::to_value(value)
            .map_err(|e| Error::conversion(format!("Failed to serialize relation: {}", e)))?;
        self.relations.insert(name.to_string(), value);
        Ok(())
    }

    /// Decode a recorded relation payload into `R`.
    ///
    /// `R` must match the relation's cardinality: `Vec<Post>` for a has-many,
    /// `Option<Profile>` for a has-one or belongs-to. A missing key and a payload
    /// that fails to deserialize both yield `None`, so a type mismatch looks like
    /// an absent relation — reach for `entry.posts.get_cached()` instead when you
    /// want the typed value without that ambiguity.
    pub fn get_relation<R: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<R> {
        self.relations
            .get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Whether `name` was resolved by the eager load.
    ///
    /// `true` even when the relation resolved to nothing — an empty list or a
    /// `null` still counts as loaded.
    pub fn has_relation(&self, name: &str) -> bool {
        self.relations.contains_key(name)
    }

    /// Drop the JSON relation map and return the model.
    ///
    /// The model's own relation wrappers keep their eagerly-loaded contents, so
    /// this discards only the untyped copy.
    pub fn into_inner(self) -> M {
        self.model
    }
}

impl<M> std::ops::Deref for WithRelations<M> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl<M> std::ops::DerefMut for WithRelations<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model
    }
}

/// A dotted relation path such as `"posts.comments.author"`, split into the
/// hops an eager load walks.
///
/// Purely syntactic: nothing here checks that the names exist. An unknown
/// segment surfaces later, from the generated eager loader, as
/// "Unknown relation '..' on ..".
#[derive(Debug, Clone)]
pub struct RelationPath {
    /// The dot-separated hops, outermost first.
    pub segments: Vec<String>,
}

impl RelationPath {
    /// Split a dotted path into segments, dropping empty ones: `""` parses to
    /// no segments and `"posts..comments"` to two.
    pub fn parse(path: &str) -> Self {
        Self {
            segments: path
                .split('.')
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// The first hop — the relation to resolve on the model at hand. Empty
    /// string for a path with no segments.
    pub fn root(&self) -> &str {
        self.segments.first().map(|s| s.as_str()).unwrap_or("")
    }

    /// The path with [`root`](Self::root) stripped, to continue with on the
    /// related model. `None` once a single hop is left.
    pub fn nested(&self) -> Option<RelationPath> {
        if self.segments.len() > 1 {
            Some(RelationPath {
                segments: self.segments[1..].to_vec(),
            })
        } else {
            None
        }
    }
}

/// The merged set of relation paths one eager load will resolve.
///
/// Several [`RelationPath`]s are folded into a trie, so `.with("posts.comments")`
/// and `.with("posts.author")` share a single load of `posts` and then branch.
/// The loader walks it level by level, batching all parents at each level, which
/// is why breadth is free and only depth costs extra round trips.
#[derive(Debug, Clone, Default)]
pub struct RelationTree {
    children: HashMap<String, RelationTree>,
}

impl RelationTree {
    /// An empty tree — an eager load against it resolves nothing and skips the
    /// loader entirely.
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }

    /// Merge a path into the tree, sharing every prefix it has in common with
    /// paths already added. Re-adding the same path is a no-op; an empty path is
    /// ignored.
    pub fn add_path(&mut self, path: &RelationPath) {
        if path.segments.is_empty() {
            return;
        }

        let root = path.root().to_string();
        let child = self.children.entry(root).or_default();

        if let Some(nested) = path.nested() {
            child.add_path(&nested);
        }
    }

    /// The relations to resolve at this level.
    ///
    /// Backed by a `HashMap`, so the order is unspecified and varies between
    /// runs; relations at one level are independent, but do not depend on the
    /// order for anything observable.
    pub fn roots(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    /// The subtree to resolve on `name`'s related models, or `None` if `name` is
    /// not in this level at all.
    ///
    /// A leaf yields `Some` of an empty tree, which the loader treats as "stop
    /// here".
    pub fn get_nested(&self, name: &str) -> Option<&RelationTree> {
        self.children.get(name)
    }

    /// Whether this level has nothing to resolve.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}

/// A [`QueryBuilder`] paired with the relations to resolve once it has run.
///
/// Reached from `Model::query().with("posts")`, or from
/// [`EagerLoadExt::eager`] for a query with no filters. It exposes a deliberately
/// small slice of the query builder — enough to filter, order and page the root
/// query. For anything richer, build the `QueryBuilder` first and call `.with()`
/// last, since that keeps the full builder available up to that point.
///
/// Terminals ([`get`](Self::get), [`first`](Self::first), [`find`](Self::find))
/// yield [`WithRelations`] and require `M: EagerLoadModel`, which the derive
/// implements. Nothing is loaded until a terminal runs.
pub struct EagerQueryBuilder<M: Model> {
    query: QueryBuilder<M>,
    pub(super) relation_tree: RelationTree,
}

/// Macro-generated eager-loading machinery. Implemented for every model by
/// `#[derive(Model)]`; not something to implement or call by hand.
#[async_trait]
#[doc(hidden)]
pub trait EagerLoadModel: Model + InternalModel {
    /// Resolve `relation_tree` across every entry of `models` in place, writing
    /// each payload into both the [`WithRelations`] map and the model's own
    /// relation wrapper.
    ///
    /// One pass per tree level over all parents at once, recursing into nested
    /// levels the same way. Related rows are fetched with the related model's
    /// soft-delete scope applied. A name with no eager path (`MorphTo`, the
    /// self-referencing wrappers) is an error naming that limitation; an
    /// undeclared name is an "Unknown relation" error.
    async fn __eager_load(
        models: &mut [WithRelations<Self>],
        relation_tree: &RelationTree,
    ) -> Result<()>
    where
        Self: Sized;

    /// Fetch every row whose `foreign_key` is one of `keys`, grouped by that key.
    ///
    /// SeaORM's `LoaderTrait` can only express relations it has an entity-level
    /// `Related` impl for, which leaves polymorphic relations without an eager
    /// path. This covers them with a single `WHERE .. IN (..)` so eager loading
    /// stays one query instead of N+1. `morph_type` adds the polymorphic type
    /// discriminator as `(column, value)`.
    ///
    /// Groups are keyed by the JSON rendering of the foreign key
    /// (`serde_json::Value::to_string`), which is what callers must look up with.
    async fn __load_grouped_by_key(
        keys: &[serde_json::Value],
        foreign_key: &'static str,
        morph_type: Option<(&'static str, &'static str)>,
    ) -> Result<HashMap<String, Vec<Self>>>
    where
        Self: Sized,
    {
        let mut grouped: HashMap<String, Vec<Self>> = HashMap::new();

        let mut lookup_keys: Vec<serde_json::Value> = Vec::new();
        for key in keys {
            if key.is_null() || lookup_keys.contains(key) {
                continue;
            }
            lookup_keys.push(key.clone());
        }

        if lookup_keys.is_empty() {
            return Ok(grouped);
        }

        // A chunk per query keeps each IN list under the backends'
        // bind-parameter limits however many parents there are.
        for chunk in lookup_keys.chunks(5_000) {
            let mut query = Self::query().where_in(foreign_key, chunk.to_vec());
            if let Some((type_column, type_value)) = morph_type {
                query = query.where_eq(type_column, type_value);
            }

            for row in query.get().await? {
                let key = row.get_field_value(foreign_key)?;
                grouped.entry(key.to_string()).or_default().push(row);
            }
        }

        Ok(grouped)
    }
}

/// Resolve the next level of a to-many relation for every parent at once.
///
/// Recursing per parent would be N+1, so the groups are flattened, loaded in one
/// pass and split back apart by their recorded sizes.
#[doc(hidden)]
pub async fn __eager_load_nested_many<R: EagerLoadModel>(
    grouped: Vec<Vec<R>>,
    nested: Option<&RelationTree>,
) -> Result<Vec<Vec<R>>> {
    let Some(nested) = nested.filter(|tree| !tree.is_empty()) else {
        return Ok(grouped);
    };

    let group_sizes: Vec<usize> = grouped.iter().map(Vec::len).collect();
    let mut flattened: Vec<WithRelations<R>> = grouped
        .into_iter()
        .flatten()
        .map(WithRelations::new)
        .collect();
    R::__eager_load(&mut flattened, nested).await?;

    let mut restored = flattened.into_iter().map(WithRelations::into_inner);
    Ok(group_sizes
        .into_iter()
        .map(|size| restored.by_ref().take(size).collect())
        .collect())
}

/// The to-one counterpart of [`__eager_load_nested_many`]: resolve the next
/// level for the present children of every parent at once, keeping each in its
/// slot.
#[doc(hidden)]
pub async fn __eager_load_nested_one<R: EagerLoadModel>(
    related: Vec<Option<R>>,
    nested: Option<&RelationTree>,
) -> Result<Vec<Option<R>>> {
    let Some(nested) = nested.filter(|tree| !tree.is_empty()) else {
        return Ok(related);
    };

    let slots: Vec<bool> = related.iter().map(Option::is_some).collect();
    let mut flattened: Vec<WithRelations<R>> = related
        .into_iter()
        .flatten()
        .map(WithRelations::new)
        .collect();
    R::__eager_load(&mut flattened, nested).await?;

    let mut restored = flattened.into_iter().map(WithRelations::into_inner);
    Ok(slots
        .into_iter()
        .map(|filled| if filled { restored.next() } else { None })
        .collect())
}

impl<M: Model> EagerQueryBuilder<M> {
    /// An unfiltered query over `M` with no relations requested yet.
    ///
    /// Equivalent to [`EagerLoadExt::eager`]. Add relations with
    /// [`with`](Self::with) — without any, the terminals still work and simply
    /// skip the loader.
    pub fn new() -> Self {
        Self {
            query: QueryBuilder::new(),
            relation_tree: RelationTree::new(),
        }
    }

    #[must_use]
    pub(crate) fn from_query(query: QueryBuilder<M>) -> Self {
        Self {
            query,
            relation_tree: RelationTree::new(),
        }
    }

    /// Request a relation, optionally nested: `"posts"` or `"posts.comments"`.
    ///
    /// Calls accumulate and merge, so shared prefixes are loaded once. The name
    /// is not validated here — a typo surfaces as an "Unknown relation" error
    /// when a terminal runs.
    pub fn with(mut self, relation: &str) -> Self {
        let path = RelationPath::parse(relation);
        self.relation_tree.add_path(&path);
        self
    }

    /// Request several relations at once; equivalent to chained
    /// [`with`](Self::with) calls.
    pub fn with_many(mut self, relations: &[&str]) -> Self {
        for relation in relations {
            self = self.with(relation);
        }
        self
    }

    /// Filter the *root* query. Relation queries are unaffected — constrain
    /// those by loading them lazily with `load_with` instead.
    pub fn where_eq<V: serde::Serialize>(mut self, column: impl IntoColumnName, value: V) -> Self {
        self.query = self.query.where_eq(column, value);
        self
    }

    /// Restrict the root query to rows whose `column` is one of `values`.
    pub fn where_in<V: serde::Serialize>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.query = self.query.where_in(column, values);
        self
    }

    /// Add a raw SQL predicate to the root query.
    ///
    /// The trusted-SQL escape hatch: the fragment is not parameterized, so never
    /// build it from user input. Validation is deferred — a rejected fragment
    /// surfaces when a terminal runs, not here.
    pub fn where_raw(mut self, sql: &str) -> Self {
        self.query = self.query.where_raw(sql);
        self
    }

    /// Order the root query by a validated column reference. Related rows keep
    /// whatever order the batched relation query returned them in.
    pub fn order_by(mut self, column: impl IntoColumnName, order: Order) -> Self {
        self.query = self.query.order_by(column, order);
        self
    }

    /// Cap the number of *root* rows. Relations are then loaded for exactly
    /// those rows, so this bounds the eager load too.
    pub fn limit(mut self, n: u64) -> Self {
        self.query = self.query.limit(n);
        self
    }

    /// Skip `n` root rows. Pair with [`order_by`](Self::order_by), since row
    /// order is otherwise unspecified.
    pub fn offset(mut self, n: u64) -> Self {
        self.query = self.query.offset(n);
        self
    }

    /// Run the root query, then resolve every requested relation across all
    /// returned rows.
    ///
    /// Relation loading is skipped entirely when the root query returns nothing,
    /// so an empty result costs exactly one query.
    pub async fn get(self) -> Result<Vec<WithRelations<M>>>
    where
        M: EagerLoadModel,
    {
        let models = self.query.get().await?;
        let mut results: Vec<WithRelations<M>> =
            models.into_iter().map(WithRelations::new).collect();
        M::__eager_load(&mut results, &self.relation_tree).await?;
        Ok(results)
    }

    /// Like [`get`](Self::get) but stops at the first row, applying `LIMIT 1` to
    /// the root query so the relation load covers only that row.
    ///
    /// Without an [`order_by`](Self::order_by) "first" is whatever the backend
    /// returns first.
    pub async fn first(mut self) -> Result<Option<WithRelations<M>>>
    where
        M: EagerLoadModel,
    {
        self.query = self.query.limit(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }

    /// Fetch one row by primary key together with its requested relations.
    ///
    /// This is the eager counterpart of `Model::find(id)`, and unlike that
    /// macro-generated method it goes through the query builder — so the model's
    /// soft-delete scope applies and a trashed row is *not* returned unless the
    /// chain opted into it. Composite keys are supported: the key serializes to
    /// an array and is matched column by column.
    pub async fn find(mut self, id: M::PrimaryKey) -> Result<Option<WithRelations<M>>>
    where
        M: EagerLoadModel,
    {
        self.query = apply_primary_key_filter(self.query, &id)?.limit(1);
        self.first().await
    }
}

impl<M: Model> Default for EagerQueryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Blanket-implemented entry points for starting an eager load from the model
/// type rather than from a query.
///
/// `User::eager().with("posts")` and `User::query().with("posts")` build the
/// same thing; these read better when there is nothing to filter, and are worse
/// when there is, because they start from an unconstrained query.
///
/// Implemented for every [`Model`], so it only needs to be in scope — it is in
/// the prelude and at the crate root.
pub trait EagerLoadExt: Model {
    /// Start an eager query with no relations requested yet. Chain
    /// [`EagerQueryBuilder::with`] to add them.
    fn eager() -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new()
    }

    /// Start an eager query for several relations at once.
    fn with_relations(relations: &[&str]) -> EagerQueryBuilder<Self>
    where
        Self: Sized,
    {
        EagerQueryBuilder::new().with_many(relations)
    }
}

impl<T: Model> EagerLoadExt for T {}

/// Read one field of a model as JSON, by name.
///
/// This is how relation loading gets at a key column without knowing the model's
/// concrete field types — grouping eagerly-loaded rows by their foreign key, for
/// instance. Blanket-implemented for every [`Model`].
pub trait RelationExt: Model {
    /// The value of `field`, rendered as JSON.
    ///
    /// Tries the typed accessor first and falls back to serializing the whole
    /// model, so relation fields and other serde-only keys resolve too — at the
    /// cost of a full serialization. `field` is the Rust field name as serde
    /// spells it, not necessarily the database column. Errors when no such key
    /// exists.
    fn get_field_value(&self, field: &str) -> Result<serde_json::Value> {
        if let Some(value) = <Self as InternalModel>::field_json_value(self, field)? {
            return Ok(value);
        }

        let json = serde_json::to_value(self)
            .map_err(|e| Error::query(format!("Failed to serialize model: {}", e)))?;

        json.get(field)
            .cloned()
            .ok_or_else(|| Error::query(format!("Field '{}' not found on model", field)))
    }
}

impl<T: Model> RelationExt for T {}
