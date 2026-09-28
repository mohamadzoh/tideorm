//! The runtime half of every relation wrapper that keeps a cache.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::helpers::QuerySource;

/// What a relation wrapper holds besides the names of its keys: the rows it
/// has cached and whether it has loaded, the key it looks rows up by, and
/// where its statements run. Serde carries the cache alone; the rest is
/// rebuilt from the owning model's columns
/// ([`refresh_runtime_relations_from`](crate::internal::InternalModel::refresh_runtime_relations_from)).
#[derive(Debug, Clone)]
pub(crate) struct RelationState<C> {
    cached: Option<C>,
    /// Whether the cache holds a load, which may have found nothing: a
    /// to-one relation known to have no row is loaded and empty.
    loaded: bool,
    /// The value rows are looked up by: the owner's key, or the foreign key
    /// the row itself holds for `BelongsTo` and `SelfRef`.
    pub(crate) key: Option<serde_json::Value>,
    pub(crate) source: QuerySource,
    /// The owning row's entity-manager identity key, which the relation's
    /// snapshots are recorded under.
    #[cfg(feature = "entity-manager")]
    pub(crate) owner_key: Option<String>,
}

impl<C> Default for RelationState<C> {
    fn default() -> Self {
        Self {
            cached: None,
            loaded: false,
            key: None,
            source: QuerySource::default(),
            #[cfg(feature = "entity-manager")]
            owner_key: None,
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
    /// same owner: when it is the same relation, or when `previous` belonged
    /// to an owner not stored yet (`previous_unsaved`), whose cached rows are
    /// the caller's to save with it. The two travel together, so a rebuilt
    /// wrapper never holds rows it counts as unloaded.
    pub(crate) fn preserve_from(
        &mut self,
        previous: &Self,
        same_relation: bool,
        previous_unsaved: bool,
    ) where
        C: Clone,
    {
        if same_relation || (previous_unsaved && previous.cached.is_some()) {
            self.cached = previous.cached.clone();
            self.loaded |= previous.loaded;
        }
        #[cfg(feature = "entity-manager")]
        if same_relation {
            self.source.preserve_from(&previous.source);
            if self.owner_key.is_none() {
                self.owner_key = previous.owner_key.clone();
            }
        }
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
