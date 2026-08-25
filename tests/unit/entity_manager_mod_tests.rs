use super::{EntityManager, EntityState, managed, save};
use crate::database::Database;
use crate::error::Error;
use crate::model::{Model, ModelMeta, OnConflictBuilder};
use async_trait::async_trait;
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

#[async_trait]
impl managed::ManagedOps for AppendingManagedEntry {
    fn current_state(&self) -> EntityState {
        EntityState::Managed
    }

    fn detach_from_context(&self, _entity_manager: &EntityManager) {}

    fn checkpoint(self: Arc<Self>) -> Box<dyn managed::ManagedCheckpoint> {
        Box::new(NoopCheckpoint)
    }

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        self.flush_count.fetch_add(1, Ordering::SeqCst);

        if let Some(child) = &self.child {
            if !self.appended.swap(true, Ordering::SeqCst) {
                entity_manager.managed_entries.write().push(child.clone());
            }
        }

        Ok(())
    }
}

struct CheckpointedManagedEntry {
    flush_count: Arc<AtomicUsize>,
    rollback_count: Arc<AtomicUsize>,
    appended: AtomicBool,
    child: Option<Arc<dyn managed::ManagedOps>>,
    fail: bool,
}

#[async_trait]
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

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
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
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RefreshAwareSyncOnlyModel {
    id: i64,
    name: String,
    runtime_state: String,
}

impl ModelMeta for RefreshAwareSyncOnlyModel {
    type PrimaryKey = i64;

    fn table_name() -> &'static str {
        <EntityManagerModTestUser as ModelMeta>::table_name()
    }

    fn primary_key_names() -> &'static [&'static str] {
        <EntityManagerModTestUser as ModelMeta>::primary_key_names()
    }

    fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
        primary_key.to_string()
    }

    fn primary_key_is_new(primary_key: &Self::PrimaryKey) -> bool {
        *primary_key == 0
    }

    fn column_names() -> &'static [&'static str] {
        <EntityManagerModTestUser as ModelMeta>::column_names()
    }

    fn field_names() -> &'static [&'static str] {
        <EntityManagerModTestUser as ModelMeta>::field_names()
    }
}

impl crate::internal::InternalModel for RefreshAwareSyncOnlyModel {
    type Entity = <EntityManagerModTestUser as crate::internal::InternalModel>::Entity;
    type ActiveModel = <EntityManagerModTestUser as crate::internal::InternalModel>::ActiveModel;

    fn into_active_model(self) -> Self::ActiveModel {
        EntityManagerModTestUser {
            id: self.id,
            name: self.name,
        }
        .into_active_model()
    }

    fn from_entity_model(model: <Self::Entity as crate::internal::EntityTrait>::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            runtime_state: "from-entity".to_string(),
        }
    }

    fn to_entity_model(&self) -> <Self::Entity as crate::internal::EntityTrait>::Model {
        EntityManagerModTestUser {
            id: self.id,
            name: self.name.clone(),
        }
        .to_entity_model()
    }

    fn column_from_str(
        name: &str,
    ) -> Option<<Self::Entity as crate::internal::EntityTrait>::Column> {
        <EntityManagerModTestUser as crate::internal::InternalModel>::column_from_str(name)
    }

    fn primary_key_columns() -> Vec<<Self::Entity as crate::internal::EntityTrait>::Column> {
        <EntityManagerModTestUser as crate::internal::InternalModel>::primary_key_columns()
    }

    fn primary_key_condition(
        primary_key: &<Self as ModelMeta>::PrimaryKey,
    ) -> crate::internal::Condition {
        <EntityManagerModTestUser as crate::internal::InternalModel>::primary_key_condition(
            primary_key,
        )
    }

    fn refresh_runtime_relations_from(&mut self, previous: &Self) {
        self.runtime_state = format!("refreshed:{}", previous.runtime_state);
    }
}

#[crate::async_trait::async_trait]
impl Model for RefreshAwareSyncOnlyModel {
    fn primary_key(&self) -> Self::PrimaryKey {
        self.id
    }

    async fn insert_or_update(
        model: Self,
        _conflict_columns: Vec<&str>,
    ) -> crate::error::Result<Self>
    where
        Self: Sized,
    {
        Ok(model)
    }

    async fn find(_id: Self::PrimaryKey) -> crate::error::Result<Option<Self>>
    where
        Self: Sized,
    {
        Ok(None)
    }

    async fn find_with(
        _id: Self::PrimaryKey,
        _db: &crate::database::Database,
    ) -> crate::error::Result<Option<Self>>
    where
        Self: Sized,
    {
        Ok(None)
    }

    async fn create(model: Self) -> crate::error::Result<Self>
    where
        Self: Sized,
    {
        Ok(model)
    }

    async fn destroy(_id: Self::PrimaryKey) -> crate::error::Result<u64>
    where
        Self: Sized,
    {
        Ok(0)
    }

    async fn save(self) -> crate::error::Result<Self>
    where
        Self: Sized,
    {
        Ok(self)
    }

    async fn update(self) -> crate::error::Result<Self>
    where
        Self: Sized,
    {
        Ok(self)
    }

    async fn delete(self) -> crate::error::Result<u64>
    where
        Self: Sized,
    {
        Ok(0)
    }

    async fn __insert_with_conflict(
        model: Self,
        _builder: OnConflictBuilder<Self>,
    ) -> crate::error::Result<Self>
    where
        Self: Sized,
    {
        Ok(model)
    }
}

impl super::TideEntityManagerMeta for RefreshAwareSyncOnlyModel {
    fn tide_table_name() -> &'static str
    where
        Self: Sized,
    {
        Self::table_name()
    }

    fn tide_pk_key(&self) -> String {
        self.id.to_string()
    }
}

impl super::TideEntityManagerMergePersisted for RefreshAwareSyncOnlyModel {
    fn tide_merge_persisted(&mut self, persisted: Self) {
        self.id = persisted.id;
        self.name = persisted.name;
    }
}

impl super::TideEntityManagerSync for RefreshAwareSyncOnlyModel {
    async fn tide_sync_entity_manager_relations<'a>(
        &'a mut self,
        _entity_manager: &'a Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        self.runtime_state = format!("synced:{}", self.runtime_state);
        Ok(())
    }
}

struct RunawayManagedEntry {
    flush_count: Arc<AtomicUsize>,
}

#[async_trait]
impl managed::ManagedOps for RunawayManagedEntry {
    fn current_state(&self) -> EntityState {
        EntityState::Managed
    }

    fn detach_from_context(&self, _entity_manager: &EntityManager) {}

    fn checkpoint(self: Arc<Self>) -> Box<dyn managed::ManagedCheckpoint> {
        Box::new(NoopCheckpoint)
    }

    async fn flush(
        self: Arc<Self>,
        entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        self.flush_count.fetch_add(1, Ordering::SeqCst);
        let next: Arc<dyn managed::ManagedOps> = Arc::new(RunawayManagedEntry {
            flush_count: self.flush_count.clone(),
        });
        entity_manager.managed_entries.write().push(next);
        Ok(())
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

#[tokio::test]
async fn sync_entity_manager_relations_only_refreshes_runtime_relations_before_sync()
-> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let result = save::sync_entity_manager_relations_only_impl(
        &RefreshAwareSyncOnlyModel {
            id: 42,
            name: "Refresh Aware".to_string(),
            runtime_state: "initial".to_string(),
        },
        &entity_manager,
    )
    .await?;

    assert_eq!(result.runtime_state, "synced:refreshed:initial");

    Ok(())
}

#[tokio::test]
async fn flush_in_scope_rejects_runaway_managed_entry_growth() {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let flush_count = Arc::new(AtomicUsize::new(0));
    let runaway: Arc<dyn managed::ManagedOps> = Arc::new(RunawayManagedEntry {
        flush_count: flush_count.clone(),
    });

    entity_manager.managed_entries.write().push(runaway);

    let error = entity_manager
        .flush_in_scope_with_checkpoints(None)
        .await
        .expect_err("runaway flush growth should fail once the pass guard is exceeded");

    assert!(error.to_string().contains("flush exceeded 16 passes"));
    assert_eq!(flush_count.load(Ordering::SeqCst), 16);
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

    new_managed.replace(EntityManagerModTestUser {
        id: 0,
        name: "Detached Replacement".to_string(),
    });

    assert_eq!(new_managed.state(), EntityState::Detached);
    assert_eq!(new_managed.get().name, "Detached Replacement");
    assert_eq!(entity_manager.managed_entries.read().len(), 1);
    assert_eq!(entity_manager.managed_identity_map.read().len(), 1);

    entity_manager.detach(&merged_managed);

    assert_eq!(merged_managed.state(), EntityState::Detached);
    assert!(entity_manager.managed_entries.read().is_empty());
    assert!(entity_manager.managed_identity_map.read().is_empty());

    merged_managed.replace(EntityManagerModTestUser {
        id: 42,
        name: "Detached Merge Replacement".to_string(),
    });

    assert_eq!(merged_managed.state(), EntityState::Detached);
    assert_eq!(merged_managed.get().name, "Detached Merge Replacement");
    assert!(entity_manager.managed_entries.read().is_empty());
    assert!(entity_manager.managed_identity_map.read().is_empty());

    Ok(())
}

#[tokio::test]
async fn rollback_entity_manager_state_restores_managed_entries_and_checkpoints()
-> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let managed = entity_manager.merge(EntityManagerModTestUser {
        id: 42,
        name: "Before".to_string(),
    })?;

    let rollback_state = save::capture_entity_manager_rollback_state(entity_manager.as_ref());
    let checkpoints = save::capture_managed_checkpoints(entity_manager.as_ref());

    managed.edit(|user| user.name = "After".to_string());
    let _orphan = entity_manager.merge(EntityManagerModTestUser {
        id: 84,
        name: "Orphan".to_string(),
    })?;
    entity_manager.snapshot::<EntityManagerModTestUser>(
        "entity_manager_mod_test_users",
        "42",
        "posts",
        &["84".to_string()],
    );

    let identity_rollback = save::new_identity_rollback_log();
    save::rollback_entity_manager_state(
        entity_manager.as_ref(),
        checkpoints,
        rollback_state,
        &identity_rollback,
    );

    assert_eq!(managed.get().name, "Before");
    assert_eq!(entity_manager.managed_entries.read().len(), 1);
    assert_eq!(entity_manager.managed_identity_map.read().len(), 1);
    assert!(
        entity_manager
            .get_managed_by_key::<EntityManagerModTestUser>("42")
            .is_some()
    );
    assert!(
        entity_manager
            .get_managed_by_key::<EntityManagerModTestUser>("84")
            .is_none()
    );
    assert!(entity_manager.snapshots.read().is_empty());

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

struct OrderRecordingManagedEntry {
    state: EntityState,
    label: &'static str,
    order: Arc<parking_lot::Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl managed::ManagedOps for OrderRecordingManagedEntry {
    fn current_state(&self) -> EntityState {
        self.state
    }

    fn detach_from_context(&self, _entity_manager: &EntityManager) {}

    fn checkpoint(self: Arc<Self>) -> Box<dyn managed::ManagedCheckpoint> {
        Box::new(NoopCheckpoint)
    }

    async fn flush(
        self: Arc<Self>,
        _entity_manager: &Arc<EntityManager>,
    ) -> crate::error::Result<()> {
        self.order.lock().push(self.label);
        Ok(())
    }
}

#[tokio::test]
async fn flush_runs_inserts_then_updates_then_deletes() -> crate::error::Result<()> {
    let entity_manager = EntityManager::new(Arc::new(Database::disconnected()));
    let order = Arc::new(parking_lot::Mutex::new(Vec::new()));

    for (state, label) in [
        (EntityState::Removed, "delete-1"),
        (EntityState::Managed, "update-1"),
        (EntityState::New, "insert-1"),
        (EntityState::Removed, "delete-2"),
        (EntityState::New, "insert-2"),
        (EntityState::Managed, "update-2"),
    ] {
        let entry: Arc<dyn managed::ManagedOps> = Arc::new(OrderRecordingManagedEntry {
            state,
            label,
            order: order.clone(),
        });
        entity_manager.managed_entries.write().push(entry);
    }

    entity_manager.flush_in_scope().await?;

    assert_eq!(
        *order.lock(),
        vec![
            "insert-1", "insert-2", "update-1", "update-2", "delete-1", "delete-2"
        ]
    );

    Ok(())
}

#[test]
fn transaction_scope_is_restored_when_a_polled_future_panics() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct PanickingFuture {
        scope_seen: Arc<AtomicBool>,
    }

    impl Future for PanickingFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.scope_seen.store(
                save::in_entity_manager_transaction_scope(),
                Ordering::SeqCst,
            );
            panic!("flush failed inside the entity manager transaction scope");
        }
    }

    let scope_seen = Arc::new(AtomicBool::new(false));
    let mut future = Box::pin(save::with_entity_manager_transaction_scope(
        save::new_identity_rollback_log(),
        PanickingFuture {
            scope_seen: scope_seen.clone(),
        },
    ));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);

    // Tests run single-threaded, so replacing the hook only silences the
    // deliberate panic below.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let polled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = future.as_mut().poll(&mut context);
    }));
    std::panic::set_hook(previous_hook);

    assert!(polled.is_err());
    assert!(scope_seen.load(Ordering::SeqCst));
    assert!(
        !save::in_entity_manager_transaction_scope(),
        "a panic must not leave the transaction scope pinned on this worker thread"
    );
}
