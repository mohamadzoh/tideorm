use super::{EntityManager, EntityState, managed};
use crate::database::Database;
use crate::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[tideorm::model(table = "entity_manager_mod_test_users")]
struct EntityManagerModTestUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

struct NoopCheckpoint;

impl managed::ManagedCheckpoint for NoopCheckpoint {
    fn rollback(self: Box<Self>, _entity_manager: &EntityManager) {}
}

struct CountingCheckpoint {
    rollback_count: Arc<AtomicUsize>,
}

impl managed::ManagedCheckpoint for CountingCheckpoint {
    fn rollback(self: Box<Self>, _entity_manager: &EntityManager) {
        self.rollback_count.fetch_add(1, Ordering::SeqCst);
    }
}

struct AppendingManagedEntry {
    flush_count: Arc<AtomicUsize>,
    appended: AtomicBool,
    child: Option<Arc<dyn managed::ManagedOps>>,
}

impl managed::ManagedOps for AppendingManagedEntry {
    fn current_state(&self) -> EntityState {
        EntityState::Managed
    }

    fn detach_from_context(&self, _entity_manager: &EntityManager) {}

    fn checkpoint(self: Arc<Self>) -> Box<dyn managed::ManagedCheckpoint> {
        Box::new(NoopCheckpoint)
    }

    fn flush<'a>(
        self: Arc<Self>,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.flush_count.fetch_add(1, Ordering::SeqCst);

            if let Some(child) = &self.child {
                if !self.appended.swap(true, Ordering::SeqCst) {
                    entity_manager.managed_entries.write().push(child.clone());
                }
            }

            Ok(())
        })
    }
}

struct CheckpointedManagedEntry {
    flush_count: Arc<AtomicUsize>,
    rollback_count: Arc<AtomicUsize>,
    appended: AtomicBool,
    child: Option<Arc<dyn managed::ManagedOps>>,
    fail: bool,
}

impl managed::ManagedOps for CheckpointedManagedEntry {
    fn current_state(&self) -> EntityState {
        EntityState::Managed
    }

    fn detach_from_context(&self, _entity_manager: &EntityManager) {}

    fn checkpoint(self: Arc<Self>) -> Box<dyn managed::ManagedCheckpoint> {
        Box::new(CountingCheckpoint {
            rollback_count: self.rollback_count.clone(),
        })
    }

    fn flush<'a>(
        self: Arc<Self>,
        entity_manager: &'a Arc<EntityManager>,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.flush_count.fetch_add(1, Ordering::SeqCst);

            if let Some(child) = &self.child {
                if !self.appended.swap(true, Ordering::SeqCst) {
                    entity_manager.managed_entries.write().push(child.clone());
                }
            }

            if self.fail {
                return Err(Error::invalid_query("forced flush failure".to_string()));
            }

            Ok(())
        })
    }
}

#[tokio::test]
async fn flush_in_scope_processes_entries_added_during_flush() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let child_flush_count = Arc::new(AtomicUsize::new(0));
    let child: Arc<dyn managed::ManagedOps> = Arc::new(AppendingManagedEntry {
        flush_count: child_flush_count.clone(),
        appended: AtomicBool::new(false),
        child: None,
    });
    let parent_flush_count = Arc::new(AtomicUsize::new(0));
    let parent: Arc<dyn managed::ManagedOps> = Arc::new(AppendingManagedEntry {
        flush_count: parent_flush_count.clone(),
        appended: AtomicBool::new(false),
        child: Some(child),
    });

    entity_manager.managed_entries.write().push(parent);

    entity_manager.flush_in_scope().await?;

    assert_eq!(parent_flush_count.load(Ordering::SeqCst), 1);
    assert_eq!(child_flush_count.load(Ordering::SeqCst), 1);
    assert_eq!(entity_manager.managed_entries.read().len(), 2);

    Ok(())
}

#[test]
fn detach_removes_managed_entry_immediately_without_flush() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));

    let new_managed = entity_manager.persist(EntityManagerModTestUser {
        id: 0,
        name: "New".to_string(),
    });
    let merged_managed = entity_manager.merge(EntityManagerModTestUser {
        id: 42,
        name: "Merged".to_string(),
    })?;

    assert_eq!(entity_manager.managed_entries.read().len(), 2);
    assert_eq!(entity_manager.managed_identity_map.read().len(), 1);

    entity_manager.detach(&new_managed);

    assert_eq!(new_managed.state(), EntityState::Detached);
    assert_eq!(entity_manager.managed_entries.read().len(), 1);
    assert_eq!(entity_manager.managed_identity_map.read().len(), 1);

    entity_manager.detach(&merged_managed);

    assert_eq!(merged_managed.state(), EntityState::Detached);
    assert!(entity_manager.managed_entries.read().is_empty());
    assert!(entity_manager.managed_identity_map.read().is_empty());

    Ok(())
}

#[tokio::test]
async fn flush_collects_checkpoints_for_entries_added_during_flush() {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let grandchild_flush_count = Arc::new(AtomicUsize::new(0));
    let grandchild_rollback_count = Arc::new(AtomicUsize::new(0));
    let grandchild: Arc<dyn managed::ManagedOps> = Arc::new(CheckpointedManagedEntry {
        flush_count: grandchild_flush_count.clone(),
        rollback_count: grandchild_rollback_count.clone(),
        appended: AtomicBool::new(false),
        child: None,
        fail: true,
    });
    let child_flush_count = Arc::new(AtomicUsize::new(0));
    let child_rollback_count = Arc::new(AtomicUsize::new(0));
    let child: Arc<dyn managed::ManagedOps> = Arc::new(CheckpointedManagedEntry {
        flush_count: child_flush_count.clone(),
        rollback_count: child_rollback_count.clone(),
        appended: AtomicBool::new(false),
        child: Some(grandchild),
        fail: false,
    });
    let parent_flush_count = Arc::new(AtomicUsize::new(0));
    let parent_rollback_count = Arc::new(AtomicUsize::new(0));
    let parent: Arc<dyn managed::ManagedOps> = Arc::new(CheckpointedManagedEntry {
        flush_count: parent_flush_count.clone(),
        rollback_count: parent_rollback_count.clone(),
        appended: AtomicBool::new(false),
        child: Some(child),
        fail: false,
    });
    let checkpoints = Arc::new(parking_lot::Mutex::new(Vec::<
        Box<dyn managed::ManagedCheckpoint>,
    >::new()));

    entity_manager.managed_entries.write().push(parent);

    let result = entity_manager
        .flush_in_scope_with_checkpoints(Some(&checkpoints))
        .await;
    assert!(result.is_err());

    for checkpoint in std::mem::take(&mut *checkpoints.lock()) {
        checkpoint.rollback(entity_manager.as_ref());
    }

    assert_eq!(parent_flush_count.load(Ordering::SeqCst), 1);
    assert_eq!(child_flush_count.load(Ordering::SeqCst), 1);
    assert_eq!(grandchild_flush_count.load(Ordering::SeqCst), 1);
    assert_eq!(parent_rollback_count.load(Ordering::SeqCst), 1);
    assert_eq!(child_rollback_count.load(Ordering::SeqCst), 1);
    assert_eq!(grandchild_rollback_count.load(Ordering::SeqCst), 1);
}
