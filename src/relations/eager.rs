//! Eager loading: resolve a model's relations in a bounded number of queries
//! instead of one per parent row.
//!
//! The entry point is `Model::query().with("posts")`, which hands the
//! [`QueryBuilder`] to an [`EagerQueryBuilder`]. Terminals return the models,
//! each requested relation's rows in its wrapper (`user.posts.get_cached()`).
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

use crate::error::{Error, Result};
use crate::internal::InternalModel;
use crate::model::Model;
use crate::query::QueryBuilder;

pub(super) mod keys;
mod paths;

pub use paths::{RelationPath, RelationTree};

/// `model`'s value of the field or column `name`: the generated accessor's,
/// or for a hand-written model without one, the serialized model's.
fn key_value<M: Model>(model: &M, name: &str) -> Result<serde_json::Value> {
    if let Some(value) = model.field_json_value(name)? {
        return Ok(value);
    }
    serde_json::to_value(model)
        .map_err(|error| Error::query(format!("Failed to serialize model: {error}")))?
        .get(name)
        .cloned()
        .ok_or_else(|| Error::query(format!("Field '{name}' not found on model")))
}

/// A [`QueryBuilder`] paired with the relations to resolve once it has run.
///
/// Reached from `Model::query().with("posts")` or `.with_many(..)`, which keep
/// the filters, ordering and paging already on the query. Shape the root query
/// before `.with()`, or afterwards through [`query`](Self::query).
///
/// Terminals ([`get`](Self::get), [`first`](Self::first), [`find`](Self::find),
/// [`paginate`](Self::paginate) and the `_or_fail` forms) yield the models,
/// each requested relation's rows in its wrapper, and require
/// `M: EagerLoadModel`, which the derive implements. Nothing is loaded until a
/// terminal runs.
#[must_use = "an eager query loads nothing until a terminal such as `get` is awaited"]
pub struct EagerQueryBuilder<M: Model> {
    query: QueryBuilder<M>,
    pub(super) relation_tree: RelationTree,
}

/// Macro-generated eager-loading machinery. Implemented for every model by
/// `#[derive(Model)]`; not something to implement or call by hand.
#[async_trait]
#[doc(hidden)]
pub trait EagerLoadModel: Model + InternalModel {
    /// Check requested paths before querying, including when there are no rows.
    fn __validate_relations(_relation_tree: &RelationTree) -> Result<()> {
        Ok(())
    }

    /// Resolve `relation_tree` across every model of `models` in place, writing
    /// each relation's rows into the model's wrapper.
    ///
    /// One pass per tree level over all parents at once, recursing into nested
    /// levels the same way. Related rows are fetched with the related model's
    /// soft-delete scope applied. A name with no eager path (`MorphTo`, the
    /// self-referencing wrappers) is an error naming that limitation; an
    /// undeclared name is an "Unknown relation" error.
    async fn __eager_load(models: &mut [Self], relation_tree: &RelationTree) -> Result<()>
    where
        Self: Sized;
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
    let mut flattened: Vec<R> = grouped.into_iter().flatten().collect();
    R::__eager_load(&mut flattened, nested).await?;

    let mut restored = flattened.into_iter();
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
    let mut flattened: Vec<R> = related.into_iter().flatten().collect();
    R::__eager_load(&mut flattened, nested).await?;

    let mut restored = flattened.into_iter();
    Ok(slots
        .into_iter()
        .map(|filled| if filled { restored.next() } else { None })
        .collect())
}

impl<M: Model> EagerQueryBuilder<M> {
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
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// let query = User::query().with(User::POSTS).with(User::POSTS);
    /// # }
    /// ```
    pub fn with(mut self, relation: &str) -> Self {
        let path = if relation.split('.').any(str::is_empty) {
            RelationPath {
                segments: vec![relation.to_owned()],
            }
        } else {
            RelationPath::parse(relation)
        };
        self.relation_tree.add_path(&path);
        self
    }

    /// Request several relations at once; equivalent to chained
    /// [`with`](Self::with) calls.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// let query = User::query().with(User::POSTS).with_many([User::POSTS]);
    /// # }
    /// ```
    pub fn with_many(mut self, relations: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        for relation in relations {
            self = self.with(relation.as_ref());
        }
        self
    }

    /// Shape the *root* query with any [`QueryBuilder`] method after `.with()`:
    ///
    /// ```ignore
    /// let users = User::query()
    ///     .with("posts")
    ///     .query(|q| q.where_gt("age", 18).where_null("banned_at").order_desc("id"))
    ///     .get()
    ///     .await?;
    /// ```
    ///
    /// Relation queries are unaffected; constrain those by loading them lazily
    /// with `load_with` instead.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// let query = User::query().with(User::POSTS).query(|q| q.where_eq(User::ID, 1));
    /// # }
    /// ```
    pub fn query(mut self, shape: impl FnOnce(QueryBuilder<M>) -> QueryBuilder<M>) -> Self {
        self.query = shape(self.query);
        self
    }

    /// Run the root query, then resolve every requested relation across all
    /// returned rows.
    ///
    /// Relation loading is skipped entirely when the root query returns nothing,
    /// so an empty result costs exactly one query.
    ///
    /// Run with the `sqlite` feature.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User, Post)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id) VALUES (1)").await?;
    /// let result = User::query().with(User::POSTS).get().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// # }
    /// ```
    pub async fn get(self) -> Result<Vec<M>>
    where
        M: EagerLoadModel,
    {
        self.ensure_relations_allowed()?;
        let database = self.query.named_database();
        let models = self.query.get().await?;
        Self::load_relations(&self.relation_tree, database, models).await
    }

    /// Refuse relations the model does not declare, or more of them, or
    /// nested deeper, than the project rules allow.
    fn ensure_relations_allowed(&self) -> Result<()>
    where
        M: EagerLoadModel,
    {
        M::__validate_relations(&self.relation_tree)?;
        crate::rules::ensure_eager_load_allowed::<M>(
            self.relation_tree.depth(),
            self.relation_tree.relations(),
        )
    }

    /// `models` with `relation_tree` resolved across them, read from
    /// `database` when the root query named one.
    async fn load_relations(
        relation_tree: &RelationTree,
        database: Option<crate::database::Database>,
        mut models: Vec<M>,
    ) -> Result<Vec<M>>
    where
        M: EagerLoadModel,
    {
        let load = M::__eager_load(&mut models, relation_tree);
        match database {
            // A `query_with(db)` reads the relations from `db` too: the
            // loaders run their queries on the scope's connection.
            Some(database) => database.scope(load).await?,
            None => load.await?,
        }
        Ok(models)
    }

    /// One page of the root query, as [`QueryBuilder::paginate`] reads it,
    /// with the requested relations resolved for that page's rows only.
    ///
    /// ```ignore
    /// let page = User::query().with(User::POSTS).paginate(2, 20).await?;
    /// ```
    pub async fn paginate(self, page: u64, per_page: u64) -> Result<crate::query::Paginated<M>>
    where
        M: EagerLoadModel,
    {
        self.ensure_relations_allowed()?;
        let database = self.query.named_database();
        let found = self.query.paginate(page, per_page).await?;
        Ok(crate::query::Paginated {
            items: Self::load_relations(&self.relation_tree, database, found.items).await?,
            total: found.total,
            page: found.page,
            per_page: found.per_page,
        })
    }

    /// Count the rows the root query matches; the relations play no part.
    pub async fn count(self) -> Result<u64> {
        self.query.count().await
    }

    /// Like [`get`](Self::get) but stops at the first row, applying `LIMIT 1` to
    /// the root query so the relation load covers only that row.
    ///
    /// Without an `order_by` "first" is whatever the backend returns first.
    ///
    /// Run with the `sqlite` feature.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User, Post)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id) VALUES (1)").await?;
    /// let result = User::query().with(User::POSTS).first().await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// # }
    /// ```
    pub async fn first(mut self) -> Result<Option<M>>
    where
        M: EagerLoadModel,
    {
        self.query = self.query.limit(1);
        let results = self.get().await?;
        Ok(results.into_iter().next())
    }

    /// [`first`](Self::first), failing with [`Error::NotFound`](crate::Error::NotFound)
    /// when no row matches.
    pub async fn first_or_fail(self) -> Result<M>
    where
        M: EagerLoadModel,
    {
        self.first()
            .await?
            .ok_or_else(|| Error::not_found(format!("No {} found matching query", M::table_name())))
    }

    /// Fetch one row by primary key together with its requested relations.
    ///
    /// This is the eager counterpart of `Model::find(id)`. Like it, it leaves a
    /// trashed row out unless the chain opted into it. Composite keys are
    /// supported: the key serializes to an array and is matched column by
    /// column.
    ///
    /// Run with the `sqlite` feature.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users")]
    /// # struct User { #[tideorm(primary_key)] id: i64, #[tideorm(has_many = "Post", foreign_key = "user_id")] posts: HasMany<Post> }
    /// # #[tideorm::model(table = "posts")]
    /// # struct Post { #[tideorm(primary_key)] id: i64, user_id: i64 }
    /// # fn main() {
    /// # #[cfg(feature = "sqlite")]
    /// # {
    /// # tokio::runtime::Runtime::new().unwrap().block_on(async {
    /// # TideConfig::init().database("sqlite::memory:").max_connections(1).models::<(User, Post)>().sync(true).connect().await?;
    /// # Database::execute("INSERT INTO users (id) VALUES (1)").await?;
    /// let result = User::query().with(User::POSTS).find(1).await?;
    /// # Ok::<(), tideorm::Error>(())
    /// # }).unwrap();
    /// # }
    /// # }
    /// ```
    pub async fn find(mut self, id: M::PrimaryKey) -> Result<Option<M>>
    where
        M: EagerLoadModel,
    {
        self.query.ensure_key_lookup_is_unpaged()?;
        self.query = self.query.where_primary_key(&id)?.limit(1);
        self.first().await
    }

    /// [`find`](Self::find), failing with [`Error::NotFound`](crate::Error::NotFound)
    /// when no row has that key.
    pub async fn find_or_fail(self, id: M::PrimaryKey) -> Result<M>
    where
        M: EagerLoadModel,
    {
        self.find(id).await?.ok_or_else(|| {
            Error::not_found(format!(
                "No {} found with that key matching the query",
                M::table_name()
            ))
        })
    }
}
