use std::sync::Arc;

use super::*;
use crate::entity_manager::{EntityManager, EntityManagerLoad, TideEntityManagerMeta};
use crate::relations::helpers::{SnapshotOwner, identity_keys, load_many_in_entity_manager};

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    /// Record the names the entity manager keys its relation snapshots by;
    /// see [`HasOne::with_metadata`](crate::relations::HasOne::with_metadata).
    pub fn with_metadata(mut self, relation_name: &'static str, owner_table: &'static str) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self
    }

    /// Record the owning model's entity-manager identity key; see
    /// [`HasOne::with_owner_key`](crate::relations::HasOne::with_owner_key).
    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.state.owner_key = Some(owner_key);
        self
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
/// identity map and records a relation snapshot.
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
        let owner =
            SnapshotOwner::new(self.owner_table, &self.state.owner_key, self.relation_name)?;
        self.state.source.entity_manager = Some(entity_manager.clone());
        let query = self.load_query("HasManyThrough::load");
        load_many_in_entity_manager(&mut self.state, entity_manager, query, owner).await
    }
}
