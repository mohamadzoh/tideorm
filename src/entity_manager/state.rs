use super::*;

/// The address of a managed entry, the same through every `Arc` of it.
fn entry_address<T: ?Sized>(entry: &Arc<T>) -> *const () {
    Arc::as_ptr(entry).cast()
}

/// The key an entity of `T` with entity-manager key `key` is filed under.
pub(super) fn identity_key<T: 'static>(key: impl Into<String>) -> IdentityKey {
    (TypeId::of::<T>(), key.into())
}

impl EntityManager {
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

    /// A tracked entity of `T` whose `field` holds `value`, both read as
    /// keys of the field's column; the caller skips a NULL value, which
    /// matches none, as SQL's comparison does.
    pub(crate) fn find_by_field<T>(&self, field: &str, value: &crate::internal::Value) -> Option<T>
    where
        T: Model,
    {
        let kind = crate::internal::column_type_of::<T>(field);
        let key = |value: &crate::internal::Value| {
            crate::orm::sql_builder::ValueKey::new(crate::internal::column_value(
                value,
                kind.as_ref(),
            ))
        };
        let wanted = key(value);
        let map = self.identity_map.read();
        map.values()
            .filter_map(|entry| entry.downcast_ref::<T>())
            .find(|model| {
                model
                    .field_value(field)
                    .is_some_and(|held| key(&held) == wanted)
            })
            .cloned()
    }

    /// File a copy of `entity` in the identity map, attaching its relations to
    /// this manager's database first, so the caller's copy queries it too.
    pub(crate) fn put<T>(&self, entity: &mut T)
    where
        T: Model,
    {
        entity.__attach_query_database(self.database());

        // Same collision as `register`: an unsaved entity's default primary key
        // is not an identity, and filing several under it makes them alias.
        if entity.is_new() {
            return;
        }

        let key = identity_key::<T>(model_identity_key(entity));
        // A copy the map hands out carries only the relations its caller
        // loads: one filed with this entity's would serve those rows long
        // after other saves changed them, and saving it would write them back.
        let mut stored = entity.clone();
        stored.__unload_relations();
        stored.__attach_query_database(self.database());
        let mut map = self.identity_map.write();
        save::record_identity_map_rollback(self, &key, &map);
        map.insert(key, Arc::new(stored));
    }

    pub(crate) fn remove_by_entity_manager_key<T>(&self, key: &str)
    where
        T: Clone + Send + Sync + 'static,
    {
        let key = identity_key::<T>(key);
        let mut map = self.identity_map.write();
        save::record_identity_map_rollback(self, &key, &map);
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
        let target = entry_address(&managed.entry);
        self.managed_entries
            .write()
            .retain(|entry| entry_address(entry) != target);
    }

    pub(super) fn register_managed_entry(&self, entry: Arc<dyn managed::ManagedOps>) {
        self.managed_entries.write().push(entry);
    }

    /// Register `entry` again, unless it still is.
    pub(super) fn reregister_managed_entry(&self, entry: Arc<dyn managed::ManagedOps>) {
        let target = entry_address(&entry);
        let mut entries = self.managed_entries.write();
        if !entries
            .iter()
            .any(|registered| entry_address(registered) == target)
        {
            entries.push(entry);
        }
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
