//! The runtime half of every relation wrapper that keeps a cache.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::helpers::QuerySource;
use crate::error::Result;
use crate::internal::Value;
use crate::model::Model;
use crate::query::QueryBuilder;

/// What a relation wrapper holds besides the names of its keys: the rows it
/// has cached and whether it has loaded, the key it looks rows up by, and
/// where its statements run. Serde carries the cache alone; the rest is
/// rebuilt from the owning model's columns
/// ([`__rebuild_relations`](crate::internal::InternalModel::__rebuild_relations)).
#[derive(Debug, Clone)]
pub(crate) struct RelationState<C> {
    cached: Option<C>,
    /// Whether the cache holds a load, which may have found nothing: a
    /// to-one relation known to have no row is loaded and empty.
    loaded: bool,
    /// The value rows are looked up by: the owner's key, or the foreign key
    /// the row itself holds for `BelongsTo` and `SelfRef`.
    pub(crate) key: Option<Value>,
    /// Whether the owner was not stored yet when the wrapper was built, as
    /// the owner's own `is_new()` tells. The rows cached in such a wrapper
    /// are the caller's, so they are kept when saving the owner rebuilds the
    /// wrapper under its stored key: an entity manager then saves them as its
    /// children. A wrapper given no key counts as one.
    pub(crate) owner_is_new: bool,
    pub(crate) source: QuerySource,
    /// The entity-manager identity keys of the rows this copy of the relation
    /// held when it loaded through an entity manager, or when a save last
    /// synced it. A save diffs the relation against them, so it removes only
    /// what this copy removed, never rows another copy added meanwhile.
    #[cfg(feature = "entity-manager")]
    pub(crate) loaded_keys: Option<Vec<String>>,
}

impl<C> Default for RelationState<C> {
    fn default() -> Self {
        Self {
            cached: None,
            loaded: false,
            key: None,
            owner_is_new: true,
            source: QuerySource::default(),
            #[cfg(feature = "entity-manager")]
            loaded_keys: None,
        }
    }
}

impl<C> RelationState<C> {
    pub(crate) fn cached(&self) -> Option<&C> {
        self.cached.as_ref()
    }

    pub(crate) fn cached_mut(&mut self) -> Option<&mut C> {
        self.cached.as_mut()
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Look rows up by the owner's `key`; `owner_is_new` is the owner's
    /// `is_new()`.
    pub(crate) fn set_owner_key(&mut self, key: Value, owner_is_new: bool) {
        self.key = Some(key);
        self.owner_is_new = owner_is_new;
    }

    /// Hold `value` as what a load found.
    pub(crate) fn set_cached(&mut self, value: Option<C>) {
        self.cached = value;
        self.loaded = true;
    }

    /// Hold `value` as what a load found, and hand it back.
    #[cfg(feature = "entity-manager")]
    pub(crate) fn insert(&mut self, value: C) -> &mut C {
        self.loaded = true;
        self.cached.insert(value)
    }

    /// Take the cached value out, leaving the load flag as it is.
    #[cfg(feature = "entity-manager")]
    pub(crate) fn take(&mut self) -> Option<C> {
        self.cached.take()
    }

    /// Whether `load` would read the database: the relation is `configured`,
    /// has a key to look rows up by, and a connection is reachable that no
    /// entity manager's cache outranks.
    pub(crate) fn can_query(&self, configured: bool) -> bool {
        configured && self.key.is_some() && self.source.prefers_database()
    }

    /// What `load` serves instead of querying: the cache, when it holds a
    /// load and `can_query` is false. `None` means `load` queries.
    pub(crate) fn served(&self, can_query: bool) -> Option<Option<&C>> {
        (self.loaded && !can_query).then_some(self.cached.as_ref())
    }

    /// Carry `previous`'s cache and load over to this state, rebuilt for the
    /// same owner: when it is the same relation (`same_columns`, the
    /// wrapper's column names as its caller compares them, and the same
    /// key), or when `previous` belonged to an owner not stored yet
    /// (`previous_unsaved`, its `owner_is_new` for a relation keyed by the
    /// owner), whose cached rows are the caller's to save with it. The two
    /// travel together, so a rebuilt wrapper never holds rows it counts as
    /// unloaded.
    pub(crate) fn preserve_from(
        &mut self,
        previous: &Self,
        same_columns: bool,
        previous_unsaved: bool,
    ) where
        C: Clone,
    {
        let same_relation = same_columns && self.key == previous.key;
        if same_relation || (previous_unsaved && previous.cached.is_some()) {
            self.cached = previous.cached.clone();
            self.loaded |= previous.loaded;
        }
        if same_relation {
            self.source.preserve_from(&previous.source);
            #[cfg(feature = "entity-manager")]
            if self.loaded_keys.is_none() {
                self.loaded_keys = previous.loaded_keys.clone();
            }
        }
    }
}

impl<E: Model> RelationState<Box<E>> {
    /// A to-one wrapper's `load`: the cache when it is
    /// [`served`](Self::served), otherwise the first row of `query`, which is
    /// `None` for a relation that points at no row.
    pub(crate) async fn load_one(
        &self,
        configured: bool,
        query: impl FnOnce() -> Result<Option<QueryBuilder<E>>>,
    ) -> Result<Option<E>> {
        if let Some(cached) = self.served(self.can_query(configured)) {
            return Ok(cached.map(|model| (**model).clone()));
        }
        match query()? {
            Some(query) => query.first().await,
            None => Ok(None),
        }
    }
}

impl<E: Model> RelationState<Vec<E>> {
    /// A to-many wrapper's `load`: the cache when it is
    /// [`served`](Self::served), otherwise the rows of `query`.
    pub(crate) async fn load_many(
        &self,
        configured: bool,
        query: impl FnOnce() -> Result<QueryBuilder<E>>,
    ) -> Result<Vec<E>> {
        if let Some(cached) = self.served(self.can_query(configured)) {
            return Ok(cached.cloned().unwrap_or_default());
        }
        query()?.get().await
    }
}

impl<C: Serialize> Serialize for RelationState<C> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.cached.serialize(serializer)
    }
}

/// A deserialized payload is loaded when it holds something: `null` says
/// nothing about the rows.
impl<'de, C: Deserialize<'de>> Deserialize<'de> for RelationState<C> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let cached = Option::<C>::deserialize(deserializer)?;
        Ok(Self {
            loaded: cached.is_some(),
            cached,
            ..Self::default()
        })
    }
}

/// `Serialize` and `Deserialize` for relation wrappers, which carry their
/// cache alone and take everything else from `Default`.
macro_rules! relation_serde {
    ($($wrapper:ident < $($param:ident),+ >),+ $(,)?) => {$(
        impl<$($param: $crate::model::Model),+> ::serde::Serialize for $wrapper<$($param),+> {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                self.state.serialize(serializer)
            }
        }

        impl<'de, $($param: $crate::model::Model),+> ::serde::Deserialize<'de>
            for $wrapper<$($param),+>
        {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                Ok(Self {
                    state: $crate::relations::state::RelationState::deserialize(deserializer)?,
                    ..Self::default()
                })
            }
        }
    )+};
}

pub(crate) use relation_serde;

/// What every caching wrapper has besides its cache accessors: where it
/// queries, and the load flag. `$rows` names what it caches.
macro_rules! cache_plumbing {
    ($rows:literal) => {
        #[doc(hidden)]
        pub fn attach_query_database(&mut self, database: &$crate::database::Database) {
            self.state.source.database = Some(database.clone());
        }

        /// Whether the cache holds a load: an eager load, a load through an
        /// entity manager, [`clear`](Self::clear), or a deserialized
        /// non-`null` payload.
        #[doc = concat!("A loaded relation may hold no ", $rows, ".")]
        pub fn is_loaded(&self) -> bool {
            self.state.is_loaded()
        }
    };
}

pub(crate) use cache_plumbing;

/// `get_cached`, `as_mut`, `is_loaded` and `clear` for a wrapper that caches
/// at most one `$model` in its `state`. `$note` adds what editing the cache
/// does for this relation.
macro_rules! one_row_cache {
    ($model:ident $(, $note:literal)?) => {
        $crate::relations::state::cache_plumbing!("row");

        #[doc(hidden)]
        pub fn set_cached(&mut self, model: Option<$model>) {
            self.state.set_cached(model.map(Box::new));
        }

        /// The cached row, if one is cached: eager-loaded, loaded through an
        /// entity manager, or deserialized. Never queries and never awaits.
        ///
        /// `None` both when nothing was loaded and when the relation is known
        /// to have no row; [`is_loaded`](Self::is_loaded) tells them apart.
        pub fn get_cached(&self) -> Option<&$model> {
            self.state.cached().map(|model| &**model)
        }

        /// Mutable access to the cached row, if one is cached.
        ///
        /// Edits stay in the cache, and [`load`](Self::load) does not see
        /// them: save the row itself to persist them.
        $(#[doc = ""] #[doc = $note])?
        pub fn as_mut(&mut self) -> Option<&mut $model> {
            self.state.cached_mut().map(|model| &mut **model)
        }

        /// Drop the cached row and mark the relation *loaded and empty*.
        ///
        /// [`is_loaded`](Self::is_loaded) stays `true`, and without a
        /// reachable connection [`load`](Self::load) answers `Ok(None)` from
        /// the cache: this records a detach, it does not invalidate.
        $(#[doc = ""] #[doc = $note])?
        pub fn clear(&mut self) {
            self.state.set_cached(None);
        }
    };
}

pub(crate) use one_row_cache;

/// `get_cached`, `as_mut`, `is_loaded` and `clear` for a wrapper that caches
/// a list of `$model` in its `state`, as [`one_row_cache!`] does for one row.
macro_rules! many_rows_cache {
    ($model:ident $(, $note:literal)?) => {
        $crate::relations::state::cache_plumbing!("rows");

        #[doc(hidden)]
        pub fn set_cached(&mut self, models: Vec<$model>) {
            self.state.set_cached(Some(models));
        }

        /// The cached rows, if the relation was loaded: eager-loaded, loaded
        /// through an entity manager, or deserialized. Never queries and never
        /// awaits; [`load`](Self::load) does not fill it.
        ///
        /// `Some(&[])` means loaded, and there are none; `None` means nothing
        /// was loaded.
        pub fn get_cached(&self) -> Option<&[$model]> {
            self.state.cached().map(Vec::as_slice)
        }

        /// Mutable access to the cached rows, if the relation was loaded.
        ///
        /// Edits stay in the cache, and [`load`](Self::load) does not see
        /// them: save the rows themselves to persist them.
        $(#[doc = ""] #[doc = $note])?
        pub fn as_mut(&mut self) -> Option<&mut Vec<$model>> {
            self.state.cached_mut()
        }

        /// Drop the cached rows and mark the relation *loaded and empty*.
        ///
        /// [`is_loaded`](Self::is_loaded) stays `true`, and without a
        /// reachable connection [`load`](Self::load) answers with no rows from
        /// the cache: this records a detach, it does not invalidate.
        $(#[doc = ""] #[doc = $note])?
        pub fn clear(&mut self) {
            self.state.set_cached(Some(Vec::new()));
        }
    };
}

pub(crate) use many_rows_cache;
