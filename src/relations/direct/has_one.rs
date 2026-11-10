use super::*;

/// A one-to-one relation: at most one row of `E` carries a foreign key pointing
/// back at this model.
///
/// Declared as a struct field; the derive fills in the keys and the parent
/// primary key:
///
/// ```ignore
/// #[tideorm(has_one = "Profile", foreign_key = "user_id")]
/// pub profile: HasOne<Profile>,
/// ```
///
/// Use [`BelongsTo`](super::BelongsTo) on the other side, where the foreign key
/// physically lives.
///
/// # Runtime-only state
///
/// Everything that makes the wrapper loadable — the parent primary key, the
/// entity-manager handle, the scoped connection — is runtime state that serde
/// does not carry. Serializing yields only the cached related model (or `null`);
/// deserializing produces a wrapper that knows that payload and nothing else, so
/// `load()` on it fails with "Parent primary key not set for relation". A model
/// rebuilt from JSON only works again after
/// [`__rebuild_relations`](crate::internal::InternalModel::__rebuild_relations)
/// re-derives the wrappers from the fresh column values, which is what
/// TideORM's own JSON round-trips do. Any new path that reconstructs a model
/// from JSON must call it too, or its relations are silently dead.
///
/// # `load()` versus `get_cached()`
///
/// [`load`](Self::load) re-queries whenever a connection is reachable, *even if
/// a value is cached* — that is deliberate, so a deserialized payload cannot
/// masquerade as database state. It serves the cache only when there is nothing
/// to query through. Under the `entity-manager` feature an attached manager is
/// the exception: it owns the instance (identity map), so its cache wins.
/// [`get_cached`](Self::get_cached) never queries and never awaits.
#[derive(Debug, Clone)]
pub struct HasOne<E: Model> {
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
    pub(crate) state: RelationState<Box<E>>,
}

key_pair_relation!(HasOne);

impl<E: Model> HasOne<E> {
    /// Fetch the related row, returning `Ok(None)` when there is none.
    ///
    /// Queries the database whenever a connection is reachable, ignoring any
    /// cached value; see the type-level note on `load()` versus `get_cached()`.
    /// The result is *not* stored back, so this is a fresh read every call —
    /// keep the value if you need it twice. Fails when the wrapper carries no
    /// parent key (a bare `Default` or a deserialized model that was never
    /// refreshed).
    pub async fn load(&self) -> Result<Option<E>> {
        let configured = self.ensure_configured().is_ok();
        let query = || self.query("HasOne::load").map(Some);
        self.state.load_one(configured, query).await
    }

    /// Fetch the related row through a caller-supplied refinement of the query.
    ///
    /// The closure receives the query already filtered to this relation, so it
    /// should only add constraints — ordering, extra `where_*`, `with_trashed()`.
    /// Unlike [`load`](Self::load) there is no cache path at all: it always
    /// queries, and errors if no connection is available.
    ///
    /// ```ignore
    /// let profile = user.profile.load_with(|q| q.with_trashed()).await?;
    /// ```
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        constraint_fn(self.query("HasOne::load_with")?)
            .first()
            .await
    }

    /// Whether a related row exists, without materializing it.
    ///
    /// Always hits the database — the cache is not consulted, so this is the
    /// honest answer rather than "did something get eagerly loaded".
    pub async fn exists(&self) -> Result<bool> {
        self.query("HasOne::exists")?.exists().await
    }

    crate::relations::state::one_row_cache!(
        E,
        "Under an entity manager, the owner's next save deletes a child the relation no longer holds."
    );
}

/// Loading through [`EntityManager::load`](crate::entity_manager::EntityManager::load)
/// resolves the row from the manager's identity map when it is there,
/// registers it when it is not, and records the key it holds, which a later
/// save compares against to tell that the relation was reassigned. A repeat
/// load serves the cached instance.
#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for HasOne<E>
where
    E: Model,
{
    type Output<'a>
        = Option<&'a E>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Result<Self::Output<'a>> {
        const CONTEXT: &str = "HasOne::load";

        self.state.source.entity_manager = Some(entity_manager.clone());
        let lookup = self
            .parent_key(CONTEXT)
            .cloned()
            .and_then(|key| Ok((self.foreign_key, key, self.query(CONTEXT)?)));
        load_one_in_entity_manager(&mut self.state, entity_manager, lookup, true).await
    }
}
