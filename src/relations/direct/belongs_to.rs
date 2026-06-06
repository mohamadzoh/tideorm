use super::*;

#[derive(Debug, Clone)]
pub struct BelongsTo<E: Model> {
    pub foreign_key: &'static str,
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

        if can_query && self.ensure_configured().is_ok() {
            if let Some(fk) = self.fk_value.as_ref() {
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

    pub fn as_mut(&mut self) -> Option<&mut E> {
        self.cached.as_deref_mut()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn get_cached(&self) -> Option<&E> {
        cached_ref(&self.cached)
    }

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
