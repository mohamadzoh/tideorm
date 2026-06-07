use super::*;

#[cfg(feature = "entity-manager")]
impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    pub fn with_metadata(
        mut self,
        relation_name: &'static str,
        owner_table: &'static str,
        related_table: &'static str,
    ) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self.related_table = related_table;
        self
    }

    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.owner_key = Some(owner_key);
        self
    }

    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.query_db = Some(database.clone());
    }

    pub(super) fn scoped_database(&self) -> Result<crate::database::Database> {
        if matches!(
            crate::database::__current_connection(),
            Ok(crate::database::ConnectionRef::Transaction(_))
        ) {
            return crate::database::__current_db();
        }

        if let Some(entity_manager) = &self.entity_manager {
            return Ok(entity_manager.database().clone());
        }

        if let Some(db) = &self.query_db {
            return Ok(db.clone());
        }

        crate::database::require_db()
    }

    fn scoped_database_for_entity_manager(
        entity_manager: &std::sync::Arc<crate::entity_manager::EntityManager>,
    ) -> Result<crate::database::Database> {
        if matches!(
            crate::database::__current_connection(),
            Ok(crate::database::ConnectionRef::Transaction(_))
        ) {
            return crate::database::__current_db();
        }

        Ok(entity_manager.database().clone())
    }

    pub fn current_keys(&self) -> Result<Vec<String>>
    where
        Related: crate::model::ModelMeta,
    {
        let mut keys = Vec::new();
        for item in self.cached.as_deref().unwrap_or(&[]) {
            if let Some(key) = crate::entity_manager::__model_entity_manager_key(item)? {
                keys.push(key);
            }
        }

        Ok(keys)
    }

    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &std::sync::Arc<crate::entity_manager::EntityManager>,
    ) -> Result<&Vec<Related>>
    where
        Related: crate::internal::InternalModel
            + crate::entity_manager::TideEntityManagerMeta
            + Clone
            + Send
            + Sync
            + 'static,
    {
        if let Some(items) = self.cached.as_ref() {
            for entity in items.iter().cloned() {
                entity_manager.put(entity);
            }

            self.entity_manager = Some(entity_manager.clone());
            let owner_key = self.owner_key.as_deref().ok_or_else(|| {
                Error::query(format!(
                    "entity manager owner key not set for relation '{}'",
                    self.relation_name
                ))
            })?;
            let ids = self.current_keys()?;
            entity_manager
                .snapshot::<Related>(self.owner_table, owner_key, self.relation_name, &ids)
                .await;
            return self.cached.as_ref().ok_or_else(|| {
                Error::internal(format!(
                    "relation cache missing for '{}' after entity manager snapshot",
                    self.relation_name
                ))
            });
        }

        self.ensure_configured()?;

        let pk = self
            .parent_pk
            .as_ref()
            .ok_or_else(|| Error::query(String::from("Parent primary key not set for relation")))?;
        let pk = require_scalar_relation_key(pk, "HasManyThrough::load_in_entity_manager")?;
        let owner_key = self.owner_key.as_deref().ok_or_else(|| {
            Error::query(format!(
                "entity manager owner key not set for relation '{}'",
                self.relation_name
            ))
        })?;
        let pivot_related_column = format!("{}.{}", self.pivot_table, self.related_key);
        let related_local_column = format!("{}.{}", Related::table_name(), self.related_local_key);

        let db = Self::scoped_database_for_entity_manager(entity_manager)?;
        let loaded = Related::query_with(&db)
            .inner_join(
                self.pivot_table,
                &pivot_related_column,
                &related_local_column,
            )
            .where_eq(
                format!("{}.{}", self.pivot_table, self.foreign_key),
                pk.clone(),
            )
            .get()
            .await?;

        let mut registered = Vec::with_capacity(loaded.len());
        for entity in loaded {
            registered.push(entity_manager.register(entity).await);
        }

        self.cached = Some(registered);
        self.entity_manager = Some(entity_manager.clone());

        let ids = self.current_keys()?;
        entity_manager
            .snapshot::<Related>(self.owner_table, owner_key, self.relation_name, &ids)
            .await;

        self.cached.as_ref().ok_or_else(|| {
            Error::internal(format!(
                "relation cache missing for '{}' after load",
                self.relation_name
            ))
        })
    }
}

#[cfg(feature = "entity-manager")]
impl<Related, Pivot> crate::entity_manager::EntityManagerLoad for HasManyThrough<Related, Pivot>
where
    Related: crate::internal::InternalModel
        + crate::entity_manager::TideEntityManagerMeta
        + Model
        + Clone
        + Send
        + Sync
        + 'static,
    Pivot: Model,
{
    type Output<'a>
        = &'a Vec<Related>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a std::sync::Arc<crate::entity_manager::EntityManager>,
    ) -> Result<Self::Output<'a>> {
        self.load_in_entity_manager(entity_manager).await
    }
}
