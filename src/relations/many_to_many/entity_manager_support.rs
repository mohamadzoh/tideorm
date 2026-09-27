use std::sync::Arc;

use super::*;
use crate::entity_manager::{EntityManager, EntityManagerLoad, TideEntityManagerMeta};
use crate::relations::{SnapshotOwner, register_loaded};

impl<Related: Model, Pivot: Model> HasManyThrough<Related, Pivot> {
    /// Attach the identity the entity manager needs to track this relation.
    ///
    /// Called by macro-generated model code, not by hand. Without it the wrapper
    /// has an empty `relation_name` and [`EntityManager`](crate::entity_manager::EntityManager)
    /// cannot key its snapshot, so `load_in_entity_manager` fails.
    pub fn with_metadata(mut self, relation_name: &'static str, owner_table: &'static str) -> Self {
        self.relation_name = relation_name;
        self.owner_table = owner_table;
        self
    }

    /// Record the owning row's entity-manager key.
    ///
    /// Also macro-generated. The key identifies which parent row this relation
    /// hangs off, so snapshots taken before and after a flush compare like for
    /// like. An owner with no primary key yet has none, and the relation stays
    /// untracked until it is saved.
    pub fn with_owner_key(mut self, owner_key: String) -> Self {
        self.owner_key = Some(owner_key);
        self
    }

    #[doc(hidden)]
    pub fn attach_query_database(&mut self, database: &crate::database::Database) {
        self.source.database = Some(database.clone());
    }

    /// Load the relation through an [`EntityManager`], registering each related
    /// model in its identity map.
    ///
    /// Prefer this over [`load`](HasManyThrough::load) inside a unit of work: rows
    /// come back as the same tracked instances the manager already holds, so edits
    /// are seen at flush time, and an already cached row gives way to the
    /// instance the manager tracks. Like `load`, a pivot table holding the same
    /// pair twice yields the related row once. It also snapshots the current key
    /// set, which is what lets the manager tell attached pivot rows from detached
    /// ones.
    ///
    /// Errors when the wrapper carries no owner key — i.e. the owning model has not
    /// been saved, or the relation was built without [`with_metadata`](HasManyThrough::with_metadata).
    pub async fn load_in_entity_manager(
        &mut self,
        entity_manager: &Arc<EntityManager>,
    ) -> Result<&Vec<Related>>
    where
        Related: TideEntityManagerMeta,
    {
        let owner = SnapshotOwner::new(self.owner_table, &self.owner_key, self.relation_name)?;
        self.source.entity_manager = Some(entity_manager.clone());

        let models = match self.cached.take() {
            Some(models) => models,
            None => {
                self.load_query("HasManyThrough::load_in_entity_manager")?
                    .get()
                    .await?
            }
        };
        let cached = self.cached.insert(models);
        register_loaded(entity_manager, cached.iter_mut(), Some(owner)).await?;
        Ok(cached)
    }
}

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
        self.load_in_entity_manager(entity_manager).await
    }
}
