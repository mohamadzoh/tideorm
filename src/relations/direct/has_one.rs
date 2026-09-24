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
/// [`refresh_runtime_relations_from`](crate::internal::InternalModel::refresh_runtime_relations_from)
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
    /// Name of the model field this relation was declared on, used as the
    /// entity manager's relation-snapshot key.
    #[cfg(feature = "entity-manager")]
    pub relation_name: &'static str,
    /// Table of the model owning the relation.
    #[cfg(feature = "entity-manager")]
    pub owner_table: &'static str,
    cached: Option<Box<E>>,
    loaded: bool,
    parent_pk: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    owner_key: Option<String>,
    source: QuerySource,
}

impl<E: Model> HasOne<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("HasOne", &[self.foreign_key, self.local_key])
    }

    fn parent_key(&self, context: &str) -> Result<&serde_json::Value> {
        self.ensure_configured()?;
        required_key(&self.parent_pk, "Parent primary key", context)
    }

    /// The query for the related row.
    fn query(&self, context: &str) -> Result<QueryBuilder<E>> {
        let pk = self.parent_key(context)?;
        Ok(self.source.query().where_eq(self.foreign_key, pk.clone()))
    }

    /// Declare the relation's key pair.
    ///
    /// Both names must be non-empty; every loading method rejects a wrapper
    /// built by [`Default`] (which leaves them `""`) with "HasOne relation is
    /// not configured". Pair with [`with_parent_pk`](Self::with_parent_pk) to
    /// make it loadable — normally the derive does both for you.
    pub fn new(foreign_key: &'static str, local_key: &'static str) -> Self {
        Self {
            foreign_key,
            local_key,
            ..Self::default()
        }
    }

    /// Supply the owning model's [`local_key`](Self::local_key) value, which is
    /// what the relation is looked up by.
    ///
    /// Must be a scalar: composite primary keys are rejected at load time
    /// because a single-column `WHERE` cannot express them. Without this the
    /// wrapper is inert — every load method errors with "Parent primary key not
    /// set for relation".
    pub fn with_parent_pk(mut self, pk: serde_json::Value) -> Self {
        self.parent_pk = Some(pk);
        self
    }

    /// Record the names the entity manager keys its relation snapshots by.
    ///
    /// Only meaningful together with [`with_owner_key`](Self::with_owner_key);
    /// without both, [`load_in_entity_manager`](Self::load_in_entity_manager)
    /// cannot record which related rows this owner held and errors out. The
    /// related table the derive passes third is not used.
    #[cfg(feature = "entity-manager")]
    pub fn with_metadata(mut self, relation_name: &'static str, owner_table: &'static str) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self
    }

    /// Record the owning model's entity-manager identity key.
    ///
    /// This is the string form of the owner's primary key that the manager's
    /// identity map uses; the derive computes it so that a relation snapshot and
    /// the owner's own map entry agree on one spelling.
    #[cfg(feature = "entity-manager")]
    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.owner_key = Some(owner_key);
        self
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.source.database = Some(database.clone());
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<E>) {
        self.cached = model.map(Box::new);
        self.loaded = true;
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk;
        #[cfg(feature = "entity-manager")]
        let same_relation = same_relation
            && self.relation_name == previous.relation_name
            && self.owner_table == previous.owner_table;

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.parent_pk.is_none(),
            same_relation,
        );

        if (same_relation || previous.parent_pk.is_none()) && !self.loaded {
            self.loaded = previous.loaded;
        }

        #[cfg(feature = "entity-manager")]
        if same_relation {
            self.source.preserve_from(&previous.source);
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
        }
    }

    /// Fetch the related row, returning `Ok(None)` when there is none.
    ///
    /// Queries the database whenever a connection is reachable, ignoring any
    /// cached value; see the type-level note on `load()` versus `get_cached()`.
    /// The result is *not* stored back, so this is a fresh read every call —
    /// keep the value if you need it twice. Fails when the wrapper carries no
    /// parent key (a bare `Default` or a deserialized model that was never
    /// refreshed).
    pub async fn load(&self) -> Result<Option<E>> {
        let can_query = self.source.prefers_database()
            && self.parent_pk.is_some()
            && self.ensure_configured().is_ok();
        if self.loaded && !can_query {
            return Ok(self.cached.as_deref().cloned());
        }

        self.query("HasOne::load")?.first().await
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

    /// Mutable access to the cached related model, if one is cached.
    ///
    /// Edits are local to the cache: nothing is written back, and
    /// [`load`](Self::load) will not see them. Persist by calling `save()` on
    /// the related model itself.
    pub fn as_mut(&mut self) -> Option<&mut E> {
        self.cached.as_deref_mut()
    }

    /// Whether the cache has been populated — by an eager load, by
    /// [`clear`](Self::clear), or by deserializing a non-`null` payload.
    ///
    /// Note the asymmetry with [`get_cached`](Self::get_cached): after `clear()`
    /// this is `true` while `get_cached()` is `None`, because "known to have no
    /// related row" is a loaded state. Deserializing a `null` payload is the
    /// opposite case and leaves this `false`.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Drop the cached row and mark the relation as *loaded and empty*.
    ///
    /// This is not "forget what we loaded" — [`is_loaded`](Self::is_loaded)
    /// stays `true` afterwards, and with no reachable connection
    /// [`load`](Self::load) will report `Ok(None)` from the cache rather than
    /// querying. Use it to record a deliberate detach, not to invalidate.
    pub fn clear(&mut self) {
        self.cached = None;
        self.loaded = true;
    }

    /// The eagerly-loaded row, if one is cached. Never queries and never awaits.
    ///
    /// Returns `None` both when nothing was ever loaded and when the relation is
    /// known to be empty; [`is_loaded`](Self::is_loaded) distinguishes them.
    pub fn get_cached(&self) -> Option<&E> {
        self.cached.as_deref()
    }

    /// Load the relation into `entity_manager`'s identity map and cache it here.
    ///
    /// Unlike [`load`](Self::load) this is `&mut self` and memoizing: the row is
    /// resolved from the manager's map when it is already there, registered into
    /// it when it is not, and a repeat call returns the cached instance rather
    /// than re-querying. An already cached row gives way to the instance the
    /// manager tracks. It also records a relation snapshot, which is how the
    /// manager later detects that the relation was reassigned — so it requires
    /// [`with_metadata`](Self::with_metadata) and
    /// [`with_owner_key`](Self::with_owner_key) to have been supplied.
    #[cfg(feature = "entity-manager")]
    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<EntityManager>,
    ) -> Result<Option<&E>>
    where
        E: TideEntityManagerMeta,
    {
        const CONTEXT: &str = "HasOne::load_in_entity_manager";

        let owner = SnapshotOwner::new(self.owner_table, &self.owner_key, self.relation_name)?;
        self.source.entity_manager = Some(entity_manager.clone());

        if !self.loaded {
            let tracked =
                entity_manager.find_by_field::<E>(self.foreign_key, self.parent_key(CONTEXT)?)?;
            let related = match tracked {
                Some(related) => Some(related),
                None => self.query(CONTEXT)?.first().await?,
            };
            self.cached = related.map(Box::new);
            self.loaded = true;
        }

        register_loaded(entity_manager, self.cached.as_deref_mut(), Some(owner)).await?;
        Ok(self.cached.as_deref())
    }
}

impl<E: Model> Default for HasOne<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            local_key: "",
            #[cfg(feature = "entity-manager")]
            relation_name: "",
            #[cfg(feature = "entity-manager")]
            owner_table: "",
            cached: None,
            loaded: false,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            owner_key: None,
            source: QuerySource::default(),
        }
    }
}

impl<E: Model> Serialize for HasOne<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasOne<E> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<E>::deserialize(deserializer)?;
        let loaded = cached.is_some();
        Ok(Self {
            cached: cached.map(Box::new),
            loaded,
            ..Self::default()
        })
    }
}

#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for HasOne<E>
where
    E: Model + TideEntityManagerMeta,
{
    type Output<'a>
        = Option<&'a E>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Result<Self::Output<'a>> {
        self.load_in_entity_manager(entity_manager).await
    }
}
