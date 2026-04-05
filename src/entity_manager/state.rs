use super::*;

impl EntityManager {
    pub async fn snapshot<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        ids: &[String],
    ) {
        let key = (
            owner_table,
            TypeId::of::<T>(),
            owner_key.to_string(),
            relation,
        );
        let mut snapshots = self.snapshots.write();
        snapshots.insert(key, ids.iter().cloned().collect());
    }

    pub async fn deletions<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        current_ids: &[String],
    ) -> Vec<String> {
        let key = (
            owner_table,
            TypeId::of::<T>(),
            owner_key.to_string(),
            relation,
        );
        let current: HashSet<_> = current_ids.iter().cloned().collect();
        let snapshots = self.snapshots.read();
        let Some(previous) = snapshots.get(&key) else {
            return Vec::new();
        };

        previous
            .iter()
            .filter(|id| !current.contains(id.as_str()))
            .cloned()
            .collect()
    }

    #[doc(hidden)]
    pub async fn additions<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        current_ids: &[String],
    ) -> Vec<String> {
        let key = (
            owner_table,
            TypeId::of::<T>(),
            owner_key.to_string(),
            relation,
        );
        let current: HashSet<_> = current_ids.iter().cloned().collect();
        let snapshots = self.snapshots.read();
        let Some(previous) = snapshots.get(&key) else {
            return current.into_iter().collect();
        };

        current
            .into_iter()
            .filter(|id| !previous.contains(id.as_str()))
            .collect()
    }

    #[doc(hidden)]
    pub fn get<T>(
        &self,
        pk: &<T as crate::model::ModelMeta>::PrimaryKey,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::model::ModelMeta + Clone + Send + Sync + 'static,
    {
        let key = (TypeId::of::<T>(), meta::pk_to_entity_manager_key(pk)?);
        Ok(self.get_by_key(&key))
    }

    #[doc(hidden)]
    pub fn get_by_entity_manager_key<T>(&self, key: &str) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get_by_key(&(TypeId::of::<T>(), key.to_string()))
    }

    #[doc(hidden)]
    pub fn find_by_field<T>(
        &self,
        field: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::internal::InternalModel + Clone + Send + Sync + 'static,
    {
        let map = self.identity_map.read();
        for entry in map.values() {
            let Some(model) = entry.downcast_ref::<T>() else {
                continue;
            };

            if <T as crate::internal::InternalModel>::field_json_value(model, field)?.as_ref()
                == Some(value)
            {
                return Ok(Some(model.clone()));
            }
        }

        Ok(None)
    }

    #[doc(hidden)]
    pub fn put<T>(&self, entity: T)
    where
        T: TideEntityManagerMeta + Clone + Send + Sync + 'static,
    {
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        let key = (TypeId::of::<T>(), entity.tide_pk_key());
        save::record_identity_map_rollback::<T>(self, &key);
        let mut map = self.identity_map.write();
        map.insert(key, Box::new(entity));
    }

    #[doc(hidden)]
    pub fn remove_by_entity_manager_key<T>(&self, key: &str)
    where
        T: Clone + Send + Sync + 'static,
    {
        let key = (TypeId::of::<T>(), key.to_string());
        save::record_identity_map_rollback::<T>(self, &key);
        let mut map = self.identity_map.write();
        map.remove(&key);
    }

    pub(crate) fn get_managed_by_key<T>(&self, key: &str) -> Option<Managed<T>>
    where
        T: Send + Sync + 'static,
    {
        let map = self.managed_identity_map.read();
        let entry = map.get(&(TypeId::of::<T>(), key.to_string()))?.clone();
        let entry = entry.downcast::<managed::ManagedEntry<T>>().ok()?;
        Some(Managed::from_entry(entry))
    }

    pub(crate) fn get_managed_by_model<T>(
        &self,
        entity: &T,
    ) -> crate::error::Result<Option<Managed<T>>>
    where
        T: crate::model::Model + crate::model::ModelMeta + Send + Sync + 'static,
    {
        let Some(key) = meta::model_entity_manager_key(entity)? else {
            return Ok(None);
        };

        Ok(self.get_managed_by_key::<T>(&key))
    }

    pub(crate) fn put_managed_entry<T>(&self, key: &str, entry: Arc<managed::ManagedEntry<T>>)
    where
        T: Send + Sync + 'static,
    {
        let erased: Arc<dyn Any + Send + Sync> = entry;
        let mut map = self.managed_identity_map.write();
        map.insert((TypeId::of::<T>(), key.to_string()), erased);
    }

    pub(crate) fn remove_managed_entry<T>(&self, key: &str)
    where
        T: Send + Sync + 'static,
    {
        let mut map = self.managed_identity_map.write();
        map.remove(&(TypeId::of::<T>(), key.to_string()));
    }

    pub(super) fn remove_managed_ops_entry<T>(&self, managed: &Managed<T>) {
        let target = Arc::as_ptr(&managed.entry).cast::<()>();
        self.managed_entries
            .write()
            .retain(|entry| Arc::as_ptr(entry).cast::<()>() != target);
    }

    pub(super) fn register_managed_entry<T>(&self, entry: Arc<managed::ManagedEntry<T>>)
    where
        T: crate::model::Model
            + TideEntityManagerMeta
            + TideEntityManagerMergePersisted
            + TideEntityManagerSync
            + serde::Serialize
            + Send
            + Sync
            + 'static,
        <<T as crate::internal::InternalModel>::Entity as crate::internal::EntityTrait>::Model:
            PartialEq,
    {
        let ops: Arc<dyn managed::ManagedOps> = entry;
        self.managed_entries.write().push(ops);
    }

    pub(super) fn get_by_key<T>(&self, key: &IdentityKey) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        let map = self.identity_map.read();
        map.get(key)
            .and_then(|value| value.downcast_ref::<T>())
            .cloned()
    }
}
