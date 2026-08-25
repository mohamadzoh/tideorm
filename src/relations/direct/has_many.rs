#[cfg_attr(feature = "entity-manager", allow(dead_code))]
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
/// Enabling the `entity-manager` feature re-points the `HasMany` name exported
/// from this crate at `TrackedHasMany`, which adds change tracking on top of the
/// same key-based loading.
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
/// through. [`get_cached`](Self::get_cached) never queries and never awaits.
#[derive(Debug, Clone)]
pub struct HasMany<E: Model> {
    /// Column on `E`'s table holding this model's key.
    pub foreign_key: &'static str,
    /// Column on *this* model whose value the foreign key matches — the primary
    /// key unless `local_key = ".."` overrides it.
    pub local_key: &'static str,
    cached: Option<Vec<E>>,
    parent_pk: Option<serde_json::Value>,
    #[cfg(feature = "entity-manager")]
    query_db: Option<crate::database::Database>,
    _marker: PhantomData<E>,
}

#[cfg_attr(feature = "entity-manager", allow(dead_code))]
impl<E: Model> HasMany<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("HasMany", &[self.foreign_key, self.local_key])
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
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
            _marker: PhantomData,
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
        self.parent_pk = Some(pk);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<E>) {
        self.cached = Some(models);
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

        #[cfg(feature = "entity-manager")]
        if self.foreign_key == previous.foreign_key
            && self.local_key == previous.local_key
            && self.parent_pk == previous.parent_pk
            && self.query_db.is_none()
        {
            self.query_db = previous.query_db.clone();
        }
    }

    #[cfg(feature = "entity-manager")]
    fn query_builder(&self) -> QueryBuilder<E> {
        if let Some(db) = &self.query_db {
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
        let can_query = {
            #[cfg(feature = "entity-manager")]
            {
                self.query_db.is_some() || has_active_database()
            }
            #[cfg(not(feature = "entity-manager"))]
            {
                has_active_database()
            }
        };

        if can_query
            && self.ensure_configured().is_ok()
            && let Some(pk) = self.parent_pk.as_ref()
        {
            let pk = require_scalar_relation_key(pk, "HasMany::load")?;

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

            return query.where_eq(self.foreign_key, pk.clone()).get().await;
        }

        if let Some(cached) = &self.cached {
            return Ok(cached.clone());
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::load")?;

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

        query.where_eq(self.foreign_key, pk.clone()).get().await
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
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::load_with")?;

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
        .where_eq(self.foreign_key, pk.clone());
        constraint_fn(query).get().await
    }

    /// Count related rows in the database without materializing them.
    ///
    /// Always queries; cached rows are not consulted, so this can legitimately
    /// disagree with `get_cached().len()` when the cache is stale or was
    /// constrained by an eager load.
    pub async fn count(&self) -> Result<u64> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::count")?;

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

        query.where_eq(self.foreign_key, pk.clone()).count().await
    }

    /// Whether at least one related row exists. Cheaper than
    /// [`count`](Self::count) when you only need the yes/no answer.
    pub async fn exists(&self) -> Result<bool> {
        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasMany::exists")?;

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

        query.where_eq(self.foreign_key, pk.clone()).exists().await
    }

    /// The eagerly-loaded rows, if this relation was populated. Never queries
    /// and never awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded. The cache is filled by `.with("posts")` eager loading or by
    /// deserializing a payload that contained the key — not by
    /// [`load`](Self::load), which does not write its result back.
    pub fn get_cached(&self) -> Option<&[E]> {
        cached_ref(&self.cached)
    }
}

impl<E: Model> Default for HasMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            #[cfg(feature = "entity-manager")]
            query_db: None,
            _marker: PhantomData,
        }
    }
}

impl<E: Model + Serialize> Serialize for HasMany<E> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

impl<'de, E: Model> Deserialize<'de> for HasMany<E> {
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
