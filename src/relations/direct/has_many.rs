use super::*;

/// A one-to-many relation: every row of `E` whose foreign key points back at
/// this model.
///
/// Declared as a struct field; the derive fills in the keys and the parent
/// primary key:
///
/// ```ignore
/// #[tideorm(has_many = "Post", foreign_key = "user_id")]
/// pub posts: HasMany<Post>,
/// ```
///
/// The plural counterpart of [`HasOne`](super::HasOne), and the inverse of
/// [`BelongsTo`](super::BelongsTo). For a relation reached through a join table
/// use [`HasManyThrough`](crate::relations::HasManyThrough) instead.
///
/// # Runtime-only state
///
/// The parent primary key and any scoped connection are runtime state that serde
/// does not carry: serializing yields just the cached `Vec<E>`, and a
/// deserialized wrapper can no longer query. A model rebuilt from JSON works
/// again only after
/// [`__rebuild_relations`](crate::internal::InternalModel::__rebuild_relations)
/// re-derives the wrappers from its fresh column values. Any new path that
/// reconstructs a model from JSON must call it, or the relation is silently
/// dead.
///
/// # `load()` versus `get_cached()`
///
/// [`load`](Self::load) re-queries whenever a connection is reachable, even with
/// rows cached — deliberately, so a deserialized payload cannot pass itself off
/// as database state. It serves the cache only when there is nothing to query
/// through. Under the `entity-manager` feature an attached manager is the
/// exception: it owns the instances (identity map), so its cache wins.
/// [`get_cached`](Self::get_cached) never queries and never awaits.
#[derive(Debug, Clone)]
pub struct HasMany<E: Model> {
    /// Column on `E`'s table holding this model's key.
    pub foreign_key: &'static str,
    /// Column on *this* model whose value the foreign key matches — the primary
    /// key unless `local_key = ".."` overrides it.
    pub local_key: &'static str,
    /// Name of the model field this relation was declared on: with
    /// `owner_table`, what tells it apart from another relation when a
    /// rebuilt model takes over a previous copy's state.
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    /// Table of the model owning the relation.
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    pub(crate) state: RelationState<Vec<E>>,
}

key_pair_relation!(HasMany);

impl<E: Model> HasMany<E> {
    /// Fetch all related rows, in no particular order — add one with
    /// [`load_with`](Self::load_with) if order matters.
    ///
    /// Queries the database whenever a connection is reachable, ignoring any
    /// cached rows; see the type-level note on `load()` versus `get_cached()`.
    /// The result is not stored back, so every call is a fresh read. Returns an
    /// empty `Vec` when there are no related rows, and errors when the wrapper
    /// carries no parent key (a bare `Default`, or a deserialized model that was
    /// never refreshed).
    pub async fn load(&self) -> Result<Vec<E>> {
        let configured = self.ensure_configured().is_ok();
        let query = || self.query("HasMany::load");
        self.state.load_many(configured, query).await
    }

    /// Fetch related rows through a caller-supplied refinement of the query.
    ///
    /// The closure receives the query already filtered to this relation, so it
    /// should only add constraints — ordering, paging, extra `where_*`. This is
    /// the normal way to order or limit a relation, since [`load`](Self::load)
    /// takes no arguments. Unlike `load` there is no cache path: it always
    /// queries.
    ///
    /// ```ignore
    /// let recent = user.posts.load_with(|q| q.order_desc("created_at").limit(5)).await?;
    /// ```
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Vec<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        constraint_fn(self.query("HasMany::load_with")?).get().await
    }

    /// Count related rows in the database without materializing them.
    ///
    /// Always queries; cached rows are not consulted, so this can legitimately
    /// disagree with `get_cached().len()` when the cache is stale or was
    /// constrained by an eager load.
    pub async fn count(&self) -> Result<u64> {
        self.query("HasMany::count")?.count().await
    }

    /// Whether at least one related row exists. Cheaper than
    /// [`count`](Self::count) when you only need the yes/no answer.
    pub async fn exists(&self) -> Result<bool> {
        self.query("HasMany::exists")?.exists().await
    }

    crate::relations::state::many_rows_cache!(
        E,
        "Under an entity manager, the owner's next save deletes a child the relation no longer holds."
    );

    /// The entity-manager identity keys of the cached rows, skipping those not
    /// stored yet.
    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn current_keys(&self) -> Result<Vec<String>> {
        identity_keys(self.state.cached())
    }
}

/// Loading through [`EntityManager::load`](crate::entity_manager::EntityManager::load)
/// keeps the rows cached already, or reads them, hands each to the manager's
/// identity map and records the keys it holds, which a later save compares
/// against to delete or detach the rows it no longer holds.
#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for HasMany<E>
where
    E: Model,
{
    type Output<'a>
        = &'a Vec<E>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Result<Self::Output<'a>> {
        self.state.source.entity_manager = Some(entity_manager.clone());
        let query = self.query("HasMany::load");
        load_many_in_entity_manager(&mut self.state, entity_manager, query).await
    }
}
