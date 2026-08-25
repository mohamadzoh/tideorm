use super::*;

// `HasMany` is re-exported from `crate::relations` rather than `super`: with the
// `entity-manager` feature on, `direct::HasMany` is crate-private and the public
// name resolves to `TrackedHasMany`.
/// The inverse of [`HasOne`](super::HasOne)/[`HasMany`](crate::relations::HasMany): the
/// foreign key lives on *this* model and points at a row of `E`.
///
/// Declared as a struct field; the derive reads the foreign-key column off the
/// model and stores its value in the wrapper:
///
/// ```ignore
/// #[tideorm(belongs_to = "User", foreign_key = "user_id")]
/// pub author: BelongsTo<User>,
/// ```
///
/// Which side you reach for is decided by the schema, not by cardinality: use
/// `BelongsTo` on the table that physically carries the column.
///
/// # Runtime-only state
///
/// The foreign-key value and any scoped connection are runtime state that serde
/// does not carry: serializing yields only the cached owner (or `null`), and a
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
/// a value cached, so a deserialized payload cannot pass itself off as database
/// state. It serves the cache only when there is nothing to query through. Under
/// the `entity-manager` feature an attached manager is the exception: it owns
/// the instance (identity map), so its cache wins.
/// [`get_cached`](Self::get_cached) never queries and never awaits.
#[derive(Debug, Clone)]
pub struct BelongsTo<E: Model> {
    /// Column on *this* model holding the owner's key.
    pub foreign_key: &'static str,
    /// Column on `E`'s table that the foreign key points at. Set from the
    /// field's `owner_key` attribute, defaulting to `"id"`. Note the asymmetry
    /// with [`HasOne`](super::HasOne), whose second key names a column on the
    /// *declaring* model.
    pub owner_key: &'static str,
    cached: Option<Box<E>>,
    loaded: bool,
    fk_value: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    entity_manager: Option<Arc<crate::entity_manager::EntityManager>>,
    #[cfg(feature = "entity-manager")]
    query_db: Option<crate::database::Database>,
    _marker: PhantomData<E>,
}

impl<E: Model> BelongsTo<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("BelongsTo", &[self.foreign_key, self.owner_key])
    }

    /// Declare the relation's key pair.
    ///
    /// Both names must be non-empty; every method rejects a wrapper built by
    /// [`Default`] (which leaves them `""`) with "BelongsTo relation is not
    /// configured". Pair with [`with_fk_value`](Self::with_fk_value) to make it
    /// loadable — normally the derive does both for you.
    pub fn new(foreign_key: &'static str, owner_key: &'static str) -> Self {
        Self {
            foreign_key,
            owner_key,
            cached: None,
            loaded: false,
            fk_value: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
            _marker: PhantomData,
        }
    }

    /// Supply this model's [`foreign_key`](Self::foreign_key) value, which is
    /// what the owner is looked up by.
    ///
    /// Must be a scalar; composite keys are rejected at load time. Without this
    /// the wrapper is inert — every query method errors with "Foreign key value
    /// not set for relation". A nullable foreign key is fine: a JSON `null` is
    /// not short-circuited but renders as `WHERE owner_key IS NULL`, which for a
    /// primary key matches nothing and yields `Ok(None)`.
    pub fn with_fk_value(mut self, fk: serde_json::Value) -> Self {
        self.fk_value = Some(fk);
        self
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn with_entity_manager(
        mut self,
        entity_manager: Arc<crate::entity_manager::EntityManager>,
    ) -> Self {
        self.entity_manager = Some(entity_manager);
        self
    }

    #[cfg(feature = "entity-manager")]
    fn query_builder(&self) -> QueryBuilder<E> {
        if let Some(entity_manager) = &self.entity_manager {
            E::query_with(entity_manager.database())
        } else if let Some(db) = &self.query_db {
            E::query_with(db)
        } else {
            E::query()
        }
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.query_db = Some(database.clone());
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, model: Option<E>) {
        self.cached = model.map(Box::new);
        self.loaded = true;
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.foreign_key == previous.foreign_key
            && self.owner_key == previous.owner_key
            && self.fk_value == previous.fk_value;

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            previous.fk_value.is_none(),
            same_relation,
        );

        if previous.fk_value.is_none() && !self.loaded {
            self.loaded = previous.loaded;
        }

        #[cfg(feature = "entity-manager")]
        if same_relation {
            if self.entity_manager.is_none() {
                self.entity_manager = previous.entity_manager.clone();
            }
            if self.query_db.is_none() {
                self.query_db = previous.query_db.clone();
            }
        }

        if same_relation && !self.loaded {
            self.loaded = previous.loaded;
        }
    }

    /// Fetch the owning row, returning `Ok(None)` when the foreign key matches
    /// nothing.
    ///
    /// Queries the database whenever a connection is reachable, ignoring any
    /// cached value; see the type-level note on `load()` versus `get_cached()`.
    /// The result is not stored back, so every call is a fresh read. Errors when
    /// the wrapper carries no foreign-key value (a bare `Default`, or a
    /// deserialized model that was never refreshed).
    pub async fn load(&self) -> Result<Option<E>> {
        #[cfg(feature = "entity-manager")]
        if self.loaded && self.entity_manager.is_some() {
            return Ok(self.cached.as_deref().cloned());
        }

        let can_query = {
            #[cfg(feature = "entity-manager")]
            {
                has_active_database() || self.entity_manager.is_some() || self.query_db.is_some()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                has_active_database()
            }
        };

        if can_query
            && self.ensure_configured().is_ok()
            && let Some(fk) = self.fk_value.as_ref()
        {
            let fk = require_scalar_relation_key(fk, "BelongsTo::load")?;

            let query = {
                #[cfg(feature = "entity-manager")]
                {
                    self.query_builder()
                }
                #[cfg(not(feature = "entity-manager"))]
                {
                    E::query()
                }
            };

            return query.where_eq(self.owner_key, fk.clone()).first().await;
        }

        if self.loaded {
            return Ok(self.cached.as_deref().cloned());
        }

        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::load")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                self.query_builder()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        };

        query.where_eq(self.owner_key, fk.clone()).first().await
    }

    /// Fetch the owning row through a caller-supplied refinement of the query.
    ///
    /// The closure receives the query already filtered to the owner key, so it
    /// should only add constraints — `with_trashed()` to reach a soft-deleted
    /// owner is the common one. Unlike [`load`](Self::load) there is no cache
    /// path: it always queries.
    pub async fn load_with<F>(&self, constraint_fn: F) -> Result<Option<E>>
    where
        F: FnOnce(QueryBuilder<E>) -> QueryBuilder<E> + Send,
    {
        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::load_with")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                self.query_builder()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        }
        .where_eq(self.owner_key, fk.clone());
        constraint_fn(query).first().await
    }

    /// Whether the owning row exists, without materializing it.
    ///
    /// Always queries; the cache is not consulted. Useful for spotting a dangling
    /// foreign key without paying to decode the owner.
    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::exists")?;

        let query = {
            #[cfg(feature = "entity-manager")]
            {
                self.query_builder()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                E::query()
            }
        };

        query.where_eq(self.owner_key, fk.clone()).exists().await
    }

    /// Mutable access to the cached owner, if one is cached.
    ///
    /// Edits are local to the cache: nothing is written back, and
    /// [`load`](Self::load) will not see them. Persist by calling `save()` on the
    /// owner itself.
    pub fn as_mut(&mut self) -> Option<&mut E> {
        self.cached.as_deref_mut()
    }

    /// Whether the cache has been populated — by an eager load, by `set_cached`,
    /// or by deserializing a non-`null` payload. Deserializing `null` leaves this
    /// `false`.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// The eagerly-loaded owner, if one is cached. Never queries and never
    /// awaits.
    ///
    /// Returns `None` both when nothing was ever loaded and when the owner is
    /// known to be absent; [`is_loaded`](Self::is_loaded) distinguishes them.
    pub fn get_cached(&self) -> Option<&E> {
        cached_ref(&self.cached)
    }

    /// Load the owner into `entity_manager`'s identity map and cache it here.
    ///
    /// Unlike [`load`](Self::load) this is `&mut self` and memoizing: the owner
    /// is resolved from the manager's map when it is already there, registered
    /// into it when it is not, and a repeat call returns the cached instance
    /// rather than re-querying. Two models pointing at the same owner therefore
    /// end up sharing one instance.
    #[cfg(feature = "entity-manager")]
    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<crate::entity_manager::EntityManager>,
    ) -> Result<Option<&E>>
    where
        E: crate::internal::InternalModel
            + crate::entity_manager::TideEntityManagerMeta
            + Clone
            + Send
            + Sync
            + 'static,
    {
        if self.loaded {
            if let Some(cached) = self.cached.as_deref() {
                entity_manager.put(cached.clone());
            }

            self.entity_manager = Some(entity_manager.clone());
            return Ok(self.cached.as_deref());
        }

        self.ensure_configured()?;

        let fk = self
            .fk_value
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Foreign key value not set for relation")))?;
        let fk = require_scalar_relation_key(fk, "BelongsTo::load_in_entity_manager")?;

        let loaded = if let Some(cached) = entity_manager.find_by_field::<E>(self.owner_key, fk)? {
            Some(cached)
        } else {
            E::query_with(entity_manager.database())
                .where_eq(self.owner_key, fk.clone())
                .first()
                .await?
        };

        self.cached = match loaded {
            Some(entity) => Some(Box::new(entity_manager.register(entity).await)),
            None => None,
        };
        self.loaded = true;
        self.entity_manager = Some(entity_manager.clone());

        Ok(self.cached.as_deref())
    }
}

impl<E: Model> Default for BelongsTo<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            owner_key: "",
            cached: None,
            loaded: false,
            fk_value: None,
            #[cfg(feature = "entity-manager")]
            entity_manager: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for BelongsTo<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for BelongsTo<E> {
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

#[cfg(all(test, feature = "entity-manager"))]
#[path = "../../../tests/unit/direct_entity_manager_relation_tests.rs"]
mod entity_manager_tests;

#[cfg(feature = "entity-manager")]
impl<E> crate::entity_manager::EntityManagerLoad for BelongsTo<E>
where
    E: crate::internal::InternalModel
        + crate::entity_manager::TideEntityManagerMeta
        + Model
        + Clone
        + Send
        + Sync
        + 'static,
{
    type Output<'a>
        = Option<&'a E>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<crate::entity_manager::EntityManager>,
    ) -> Result<Self::Output<'a>> {
        self.load_in_entity_manager(entity_manager).await
    }
}
