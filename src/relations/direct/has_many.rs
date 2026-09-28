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
/// [`refresh_runtime_relations_from`](crate::internal::InternalModel::refresh_runtime_relations_from)
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
    /// Name of the model field this relation was declared on, used as the
    /// entity manager's relation-snapshot key.
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    /// Table of the model owning the relation.
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    state: RelationState<Vec<E>>,
}

impl<E: Model> HasMany<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("HasMany", &[self.foreign_key, self.local_key])
    }

    /// The query for the related rows.
    fn query(&self, context: &str) -> Result<QueryBuilder<E>> {
        self.ensure_configured()?;
        let pk = required_key(&self.state.key, "Parent primary key", context)?;
        Ok(where_key(self.state.source.query(), self.foreign_key, pk))
    }

    /// Declare the relation's key pair.
    ///
    /// Both names must be non-empty; every method rejects a wrapper built by
    /// [`Default`] (which leaves them `""`) with "HasMany relation is not
    /// configured". Pair with [`with_parent_pk`](Self::with_parent_pk) to make it
    /// loadable — normally the derive does both for you.
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            ..Self::default()
        }
    }

    /// Supply the owning model's [`local_key`](Self::local_key) value, which is
    /// what the related rows are looked up by.
    ///
    /// Must be a scalar: composite primary keys are rejected at load time
    /// because a single-column `WHERE` cannot express them. Without this the
    /// wrapper is inert — every query method errors with "Parent primary key not
    /// set for relation".
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.state.key = Some(pk);
        self
    }

    /// Record the names the entity manager keys its relation snapshots by;
    /// see [`HasOne::with_metadata`](super::HasOne::with_metadata).
    #[cfg(feature = "entity-manager")]
    pub fn with_metadata(mut self, relation_name: &'static str, owner_table: &'static str) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self
    }

    /// Record the owning model's entity-manager identity key; see
    /// [`HasOne::with_owner_key`](super::HasOne::with_owner_key).
    #[cfg(feature = "entity-manager")]
    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.state.owner_key = Some(owner_key);
        self
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.state.source.database = Some(database.clone());
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<E>) {
        self.state.set_cached(Some(models));
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.state.key == previous.state.key;
        #[cfg(feature = "entity-manager")]
        let same_relation = same_relation
            && self.relation_name == previous.relation_name
            && self.owner_table == previous.owner_table;

        self.state.preserve_from(
            &previous.state,
            same_relation,
            owner_is_unsaved(&previous.state.key),
        );
    }

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
        let can_query = self.state.can_query(self.ensure_configured().is_ok());
        if let Some(cached) = self.state.served(can_query) {
            return Ok(cached.cloned().unwrap_or_default());
        }

        self.query("HasMany::load")?.get().await
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

    /// Mutable access to the cached rows, if any are cached. Edits are local
    /// to the cache; persist them by saving the models.
    pub fn as_mut(&mut self) -> Option<&mut Vec<E>> {
        self.state.cached_mut()
    }

    /// Whether the cache holds a load: an eager load, a load through an
    /// entity manager, or a deserialized non-`null` payload.
    pub fn is_loaded(&self) -> bool {
        self.state.is_loaded()
    }

    /// The eagerly-loaded rows, if this relation was populated. Never queries
    /// and never awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded. The cache is filled by `.with("posts")` eager loading or by
    /// deserializing a payload that contained the key — not by
    /// [`load`](Self::load), which does not write its result back.
    pub fn get_cached(&self) -> Option<&[E]> {
        self.state.cached().map(Vec::as_slice)
    }

    /// The entity-manager identity keys of the cached rows, skipping those not
    /// stored yet.
    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn current_keys(&self) -> Result<Vec<String>> {
        identity_keys(self.state.cached())
    }
}

impl<E: Model> Default for HasMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            local_key: "",
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            state: RelationState::default(),
        }
    }
}

relation_serde!(HasMany<E>);

/// Loading through [`EntityManager::load`](crate::entity_manager::EntityManager::load)
/// keeps the rows cached already, or reads them, hands each to the manager's
/// identity map and records a relation snapshot.
#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for HasMany<E>
where
    E: Model + TideEntityManagerMeta,
{
    type Output<'a>
        = &'a Vec<E>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Result<Self::Output<'a>> {
        let owner =
            SnapshotOwner::new(self.owner_table, &self.state.owner_key, self.relation_name)?;
        self.state.source.entity_manager = Some(entity_manager.clone());
        let query = self.query("HasMany::load");
        load_many_in_entity_manager(&mut self.state, entity_manager, query, owner).await
    }
}
