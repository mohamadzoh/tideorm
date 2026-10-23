use std::sync::Arc;

use super::*;
use crate::entity_manager::{EntityManager, EntityManagerLoad, TideEntityManagerMeta};
use crate::relations::helpers::{identity_keys, load_many_in_entity_manager};

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    /// Record the relation's field name and its owner's table; see
    /// [`HasOne::with_metadata`](crate::relations::HasOne::with_metadata).
    pub fn with_metadata(mut self, relation_name: &'static str, owner_table: &'static str) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self
    }

    /// The keys this copy held when it loaded through an entity manager, or
    /// when a save last synced it.
    pub(crate) fn loaded_keys(&self) -> Option<&[String]> {
        self.state.loaded_keys.as_deref()
    }

    pub(crate) fn set_loaded_keys(&mut self, keys: Vec<String>) {
        self.state.loaded_keys = Some(keys);
    }

    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.state.source.database = Some(database.clone());
    }

    /// The entity-manager identity keys of the cached rows, skipping those not
    /// stored yet.
    #[doc(hidden)]
    pub fn current_keys(&self) -> Result<Vec<String>> {
        identity_keys(self.state.cached())
    }
}

/// Loading through [`EntityManager::load`](crate::entity_manager::EntityManager::load)
/// keeps the rows cached already, or reads them, hands each to the manager's
/// identity map and records the keys it holds, which a later save compares
/// against to detach the rows it no longer holds.
impl<Related, Pivot> EntityManagerLoad for HasManyThrough<Related, Pivot>
where
    Related: Model + TideEntityManagerMeta,
    Pivot: Model,
{
    type Output<'a>
        = &'a Vec<Related>
    where
        Self: 'a;

    async fn load_with_entity_manager<'a>(
        &'a mut self,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Result<Self::Output<'a>> {
        self.state.source.entity_manager = Some(entity_manager.clone());
        let query = self.load_query("HasManyThrough::load");
        load_many_in_entity_manager(&mut self.state, entity_manager, query).await
    }
}
