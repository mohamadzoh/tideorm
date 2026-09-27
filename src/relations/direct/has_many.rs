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
    pub(crate) cached: Option<Vec<E>>,
    parent_pk: Option<serde_json::Value>,
    source: QuerySource,
}

impl<E: Model> HasMany<E> {
    fn ensure_configured(&self) -> Result<()> {
        ensure_relation_configured("HasMany", &[self.foreign_key, self.local_key])
    }

    /// The query for the related rows.
    pub(crate) fn query(&self, context: &str) -> Result<QueryBuilder<E>> {
        self.ensure_configured()?;
        let pk = required_key(&self.parent_pk, "Parent primary key", context)?;
        Ok(where_key(self.source.query(), self.foreign_key, pk))
    }

    /// Whether `other` describes the same relation of the same owner.
    pub(crate) fn same_relation(&self, other: &Self) -> bool {
        self.foreign_key == other.foreign_key
            && self.local_key == other.local_key
            && self.parent_pk == other.parent_pk
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
        self.parent_pk = Some(pk);
        self
    }

    #[doc(hidden)]
    pub fn set_cached(&mut self, models: Vec<E>) {
        self.cached = Some(models);
    }

    #[doc(hidden)]
    pub fn preserve_runtime_state_from(&mut self, previous: &Self) {
        let same_relation = self.same_relation(previous);

        preserve_cached_value(
            &mut self.cached,
            &previous.cached,
            owner_is_unsaved(&previous.parent_pk),
            same_relation,
        );

        #[cfg(feature = "entity-manager")]
        if same_relation {
            self.source.preserve_from(&previous.source);
        }
    }

    #[cfg(feature = "entity-manager")]
    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.source.database = Some(database.clone());
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
        let can_query = self.source.prefers_database()
            && self.parent_pk.is_some()
            && self.ensure_configured().is_ok();
        if let Some(cached) = &self.cached
            && !can_query
        {
            return Ok(cached.clone());
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

    /// The eagerly-loaded rows, if this relation was populated. Never queries
    /// and never awaits.
    ///
    /// `Some(&[])` means "loaded, and there are none"; `None` means nothing was
    /// ever loaded. The cache is filled by `.with("posts")` eager loading or by
    /// deserializing a payload that contained the key — not by
    /// [`load`](Self::load), which does not write its result back.
    pub fn get_cached(&self) -> Option<&[E]> {
        self.cached.as_deref()
    }
}

impl<E: Model> Default for HasMany<E> {
    fn default() -> Self {
        Self {
            foreign_key: "",
            local_key: "",
            cached: None,
            parent_pk: None,
            source: QuerySource::default(),
        }
    }
}

impl<E: Model> Serialize for HasMany<E> {
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
