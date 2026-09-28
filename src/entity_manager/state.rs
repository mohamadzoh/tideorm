use super::*;

/// The key a relation snapshot of `owner_key`'s `relation`, holding rows of `T`,
/// is filed under.
fn snapshot_key<T: 'static>(
    owner_table: &'static str,
    owner_key: &str,
    relation: &'static str,
) -> SnapshotKey {
    (
        owner_table,
        TypeId::of::<T>(),
        owner_key.to_string(),
        relation,
    )
}

/// The key an entity of `T` with entity-manager key `key` is filed under.
pub(super) fn identity_key<T: 'static>(key: impl Into<String>) -> IdentityKey {
    (TypeId::of::<T>(), key.into())
}

impl EntityManager {
    pub(crate) fn snapshot<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        ids: &[String],
    ) {
        let key = snapshot_key::<T>(owner_table, owner_key, relation);
        let mut snapshots = self.snapshots.write();
        snapshots.insert(key, ids.iter().cloned().collect());
    }

    /// The keys recorded for a relation, or `None` when none were.
    fn recorded<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
    ) -> Option<HashSet<String>> {
        let key = snapshot_key::<T>(owner_table, owner_key, relation);
        self.snapshots.read().get(&key).cloned()
    }

    /// The recorded keys `current_ids` no longer holds; none without a
    /// snapshot, since nothing can have left a set that was never recorded.
    pub(crate) fn deletions<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        current_ids: &[String],
    ) -> Vec<String> {
        let Some(previous) = self.recorded::<T>(owner_table, owner_key, relation) else {
            return Vec::new();
        };
        let current: HashSet<&String> = current_ids.iter().collect();
        previous
            .into_iter()
            .filter(|id| !current.contains(id))
            .collect()
    }

    /// The keys of `current_ids` not recorded, each once.
    pub(crate) fn additions<T: 'static>(
        &self,
        owner_table: &'static str,
        owner_key: &str,
        relation: &'static str,
        current_ids: &[String],
    ) -> Vec<String> {
        let current: HashSet<String> = current_ids.iter().cloned().collect();
        // No snapshot means no baseline, and that is NOT symmetric with `deletions`:
        // nothing can have been removed from a set that was never recorded, but
        // everything currently present is new. Returning empty here loses the
        // associations permanently — the flush arm attaches `additions()` and then
        // calls `snapshot()` with the current keys, so the next flush sees them as
        // already recorded and never inserts the pivot rows.
        //
        // Reporting everything used to duplicate pivot rows on a re-flush. It no
        // longer can: `HasManyThrough::attach` checks for the row first and is a
        // no-op when the pair is already attached.
        match self.recorded::<T>(owner_table, owner_key, relation) {
            Some(previous) => current
                .into_iter()
                .filter(|id| !previous.contains(id))
                .collect(),
            None => current.into_iter().collect(),
        }
    }

    pub(crate) fn get<T>(
        &self,
        pk: &<T as crate::model::ModelMeta>::PrimaryKey,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::model::ModelMeta,
    {
        let key = identity_key::<T>(meta::pk_to_entity_manager_key(pk)?);
        Ok(self.get_by_key(&key))
    }

    pub(crate) fn get_by_entity_manager_key<T>(&self, key: &str) -> Option<T>
    where
        T: Clone + Send + Sync + 'static,
    {
        self.get_by_key(&identity_key::<T>(key))
    }

    pub(crate) fn find_by_field<T>(
        &self,
        field: &str,
        value: &serde_json::Value,
    ) -> crate::error::Result<Option<T>>
    where
        T: crate::internal::InternalModel,
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

    pub(crate) fn put<T>(&self, entity: T)
    where
        T: TideEntityManagerMeta + Clone + Send + Sync + 'static,
    {
        let mut entity = entity;
        entity.tide_attach_entity_manager_database(self.database());

        // Same collision as `register`: an unsaved entity's default primary key
        // is not an identity, and filing several under it makes them alias.
        if entity.tide_pk_is_new() {
            return;
        }

        let key = identity_key::<T>(entity.tide_pk_key());
        save::record_identity_map_rollback::<T>(self, &key);
        let mut map = self.identity_map.write();
        map.insert(key, Box::new(entity));
    }

    pub(crate) fn remove_by_entity_manager_key<T>(&self, key: &str)
    where
        T: Clone + Send + Sync + 'static,
    {
        let key = identity_key::<T>(key);
        save::record_identity_map_rollback::<T>(self, &key);
        let mut map = self.identity_map.write();
        map.remove(&key);
    }

    pub(crate) fn get_managed_by_key<T>(&self, key: &str) -> Option<Managed<T>>
    where
        T: Send + Sync + 'static,
    {
        let map = self.managed_identity_map.read();
        let entry = map.get(&identity_key::<T>(key))?.clone();
        let entry = entry.downcast::<managed::ManagedEntry<T>>().ok()?;
        Some(Managed::from_entry(entry))
    }

    pub(crate) fn put_managed_entry<T>(&self, key: &str, entry: Arc<managed::ManagedEntry<T>>)
    where
        T: Send + Sync + 'static,
    {
        let erased: Arc<dyn Any + Send + Sync> = entry;
        let mut map = self.managed_identity_map.write();
        map.insert(identity_key::<T>(key), erased);
    }

    pub(crate) fn remove_managed_entry<T>(&self, key: &str)
    where
        T: Send + Sync + 'static,
    {
        let mut map = self.managed_identity_map.write();
        map.remove(&identity_key::<T>(key));
    }

    pub(super) fn remove_managed_ops_entry<T>(&self, managed: &Managed<T>) {
        let target = Arc::as_ptr(&managed.entry).cast::<()>();
        self.managed_entries
            .write()
            .retain(|entry| Arc::as_ptr(entry).cast::<()>() != target);
    }

    pub(super) fn register_managed_entry<T>(&self, entry: Arc<managed::ManagedEntry<T>>)
    where
        T: TideEntityManagerMergePersisted + TideEntityManagerSync,
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
