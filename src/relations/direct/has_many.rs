#[cfg_attr(feature = "entity-manager", allow(dead_code))]
use super::*;

#[derive(Debug, Clone)]
pub struct HasMany<E: Model> {
    pub foreign_key: &'static str,
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

        if can_query && self.ensure_configured().is_ok() {
            if let Some(pk) = self.parent_pk.as_ref() {
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
