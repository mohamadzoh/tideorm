//! Seed ledger decisions and callbacks share one lock for each operation.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tideorm::prelude::*;
use tideorm::seeding::{Seed, Seeder, async_trait};
use tokio::sync::Notify;

struct CountedSeed(Arc<AtomicUsize>);

/// A file database, installed as the global one, holding `seed_effects` and
/// the seed ledger, in WAL mode: a transaction's read snapshot then outlives
/// another connection's commit.
async fn seed_effects_database(tag: &str) -> (Database, crate::fixtures::TempFile) {
    let (db, file) = crate::fixtures::file_database(tag).await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("PRAGMA journal_mode=WAL; CREATE TABLE seed_effects(value INTEGER)")
        .await
        .unwrap();
    Seeder::new().status().await.unwrap();
    (db, file)
}

/// Assert that `runner` is still waiting for the seed lock.
async fn assert_waiting<T>(runner: &mut tokio::task::JoinHandle<T>, context: &str) {
    assert!(
        tokio::time::timeout(Duration::from_millis(100), runner)
            .await
            .is_err(),
        "{context}"
    );
}

#[tokio::test]
async fn stale_seed_snapshot_fails_before_repeating_a_committed_callback() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (db, _file) = seed_effects_database("seed-snapshot").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let seeder = Arc::new(Seeder::new().add(CountedSeed(calls.clone())));
        let scoped = db.clone();
        let outer_seeder = seeder.clone();
        let result: tideorm::Result<()> = db
            .transaction(move |_| {
                Box::pin(async move {
                    scoped.query_raw_json("SELECT * FROM _seeds").await?;
                    let other = outer_seeder.clone();
                    assert_eq!(
                        tokio::spawn(async move { other.run().await })
                            .await
                            .unwrap()?
                            .executed
                            .len(),
                        1
                    );
                    outer_seeder.run().await?;
                    Ok(())
                })
            })
            .await;
        assert!(result.is_err(), "a stale write snapshot must be restarted");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(seeder.run().await.unwrap().skipped.len(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        crate::fixtures::close_global(&db).await;
    })
    .await
    .unwrap();
}

#[async_trait]
impl Seed for CountedSeed {
    fn name(&self) -> &str {
        "counted"
    }
    async fn run(&self, db: &Database) -> tideorm::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        db.exec_raw("INSERT INTO seed_effects VALUES(1)").await?;
        Ok(())
    }
}

#[tokio::test]
async fn outer_transactions_hold_seed_locks_until_commit_rollback_or_cancellation() {
    tokio::time::timeout(Duration::from_secs(15), async {
        for outcome in ["commit", "rollback", "cancel"] {
            let (db, _file) = seed_effects_database("outer-seed").await;
            let calls = Arc::new(AtomicUsize::new(0));
            let seeder = Arc::new(Seeder::new().add(CountedSeed(calls.clone())));
            let entered = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let outer = {
                let (db, seeder, entered, release) =
                    (db.clone(), seeder.clone(), entered.clone(), release.clone());
                tokio::spawn(async move {
                    db.transaction(move |_| {
                        Box::pin(async move {
                            assert_eq!(seeder.run().await?.executed.len(), 1);
                            // Reusing the guard must not deadlock a sequential call.
                            assert_eq!(seeder.run().await?.skipped.len(), 1);
                            entered.notify_one();
                            release.notified().await;
                            if outcome == "rollback" {
                                return Err(tideorm::Error::query("rollback outer"));
                            }
                            Ok(())
                        })
                    })
                    .await
                })
            };
            entered.notified().await;
            let mut second = tokio::spawn(async move { seeder.run().await });
            assert_waiting(&mut second, outcome).await;
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{outcome}");
            if outcome == "cancel" {
                outer.abort();
                assert!(outer.await.unwrap_err().is_cancelled());
            } else {
                release.notify_one();
                assert_eq!(outer.await.unwrap().is_ok(), outcome == "commit");
            }
            let result = second.await.unwrap().unwrap();
            if outcome == "commit" {
                assert_eq!(result.skipped.len(), 1);
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            } else {
                assert_eq!(result.executed.len(), 1);
                assert_eq!(calls.load(Ordering::SeqCst), 2);
            }
            assert_eq!(crate::fixtures::row_count(&db, "seed_effects").await, 1);
            crate::fixtures::close_global(&db).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn seed_locks_survive_committed_and_rolled_back_savepoints() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for rollback in [false, true] {
            let (db, _file) = seed_effects_database("nested-seed").await;
            let calls = Arc::new(AtomicUsize::new(0));
            let seeder = Arc::new(Seeder::new().add(CountedSeed(calls.clone())));
            let entered = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let outer = {
                let (db, seeder, entered, release) =
                    (db.clone(), seeder.clone(), entered.clone(), release.clone());
                tokio::spawn(async move {
                    let nested_db = db.clone();
                    db.transaction(move |_| {
                        Box::pin(async move {
                            let nested_seeder = seeder.clone();
                            let nested = nested_db
                                .transaction(move |_| {
                                    Box::pin(async move {
                                        nested_seeder.run().await?;
                                        if rollback {
                                            return Err(tideorm::Error::query(
                                                "rollback savepoint",
                                            ));
                                        }
                                        Ok(())
                                    })
                                })
                                .await;
                            assert_eq!(nested.is_err(), rollback);
                            let again = seeder.run().await?;
                            assert_eq!(again.executed.len(), usize::from(rollback));
                            entered.notify_one();
                            release.notified().await;
                            Ok(())
                        })
                    })
                    .await
                })
            };
            entered.notified().await;
            let mut second = tokio::spawn(async move { seeder.run().await });
            assert_waiting(&mut second, "").await;
            assert_eq!(calls.load(Ordering::SeqCst), 1 + usize::from(rollback));
            release.notify_one();
            outer.await.unwrap().unwrap();
            assert_eq!(second.await.unwrap().unwrap().skipped.len(), 1);
            crate::fixtures::close_global(&db).await;
        }
    })
    .await
    .unwrap();
}

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
    calls: AtomicUsize,
}

struct Once(Arc<Gate>);
#[async_trait]
impl Seed for Once {
    fn name(&self) -> &str {
        "once"
    }
    async fn run(&self, db: &Database) -> tideorm::Result<()> {
        if self.0.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.0.entered.notify_one();
            self.0.release.notified().await;
        }
        db.exec_raw("INSERT INTO seed_effects VALUES(1)").await?;
        Ok(())
    }
}

#[tokio::test]
async fn concurrent_seed_runs_execute_once_on_file_and_memory_databases() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for file_backed in [false, true] {
            let file = crate::fixtures::TempFile::new("seeds", "sqlite");
            let db = Database::builder()
                .url(if file_backed {
                    format!("sqlite://{}?mode=rwc", file.display())
                } else {
                    "sqlite::memory:".into()
                })
                .max_connections(3)
                .build()
                .await
                .unwrap();
            Database::set_global(db.clone()).unwrap();
            db.exec_raw("CREATE TABLE seed_effects(value INTEGER)")
                .await
                .unwrap();
            let gate = Arc::new(Gate::default());
            let seeder = Arc::new(Seeder::new().add(Once(gate.clone())));
            let first = seeder.clone();
            let first = tokio::spawn(async move { first.run().await });
            gate.entered.notified().await;
            let mut second = tokio::spawn(async move { seeder.run().await });
            assert_waiting(&mut second, "").await;
            gate.release.notify_one();
            assert_eq!(first.await.unwrap().unwrap().executed.len(), 1);
            assert_eq!(second.await.unwrap().unwrap().skipped.len(), 1);
            assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
            assert_eq!(crate::fixtures::row_count(&db, "seed_effects").await, 1);
            crate::fixtures::close_global(&db).await;
        }
    })
    .await
    .unwrap();
}

struct Step(&'static str, Arc<Gate>);
#[async_trait]
impl Seed for Step {
    fn name(&self) -> &str {
        self.0
    }
    async fn run(&self, db: &Database) -> tideorm::Result<()> {
        db.exec_raw(&format!("INSERT INTO seed_steps VALUES('{}')", self.0))
            .await?;
        Ok(())
    }
    async fn rollback(&self, db: &Database) -> tideorm::Result<()> {
        if self.0 == "02" && self.1.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.1.entered.notify_one();
            self.1.release.notified().await;
        }
        db.exec_raw(&format!("DELETE FROM seed_steps WHERE name = '{}'", self.0))
            .await?;
        Ok(())
    }
}

#[tokio::test]
async fn complete_seed_rollbacks_and_refresh_exclude_other_runners() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for operation in ["reset", "steps", "refresh", "rollback", "named"] {
            let db = Database::builder()
                .url("sqlite::memory:")
                .max_connections(3)
                .build()
                .await
                .unwrap();
            Database::set_global(db.clone()).unwrap();
            db.exec_raw("CREATE TABLE seed_steps(name TEXT UNIQUE)")
                .await
                .unwrap();
            let gate = Arc::new(Gate::default());
            let seeder = Arc::new(
                Seeder::new()
                    .add(Step("01", gate.clone()))
                    .add(Step("02", gate.clone())),
            );
            seeder.run().await.unwrap();
            let rollback = seeder.clone();
            let rollback = tokio::spawn(async move {
                match operation {
                    "reset" => rollback.reset().await,
                    "steps" => rollback.rollback_steps(2).await,
                    "refresh" => rollback.refresh().await,
                    "rollback" => rollback.rollback().await,
                    _ => rollback.rollback_seed("02").await,
                }
            });
            gate.entered.notified().await;
            let mut runner = tokio::spawn(async move { seeder.run().await });
            assert_waiting(&mut runner, operation).await;
            gate.release.notify_one();
            let result = rollback.await.unwrap().unwrap();
            let ran = runner.await.unwrap().unwrap();
            let expected = if matches!(operation, "rollback" | "named") {
                vec!["02"]
            } else {
                vec!["02", "01"]
            };
            assert_eq!(
                result
                    .rolled_back
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            if operation == "refresh" {
                assert_eq!(result.executed.len(), 2);
                assert_eq!(ran.skipped.len(), 2);
            } else {
                assert_eq!(ran.executed.len(), expected.len());
            }
            assert_eq!(crate::fixtures::row_count(&db, "seed_steps").await, 2);
            crate::fixtures::close_global(&db).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn seed_lock_wait_cancellation_and_failed_callback_allow_retry() {
    struct FailOnce(AtomicUsize);
    #[async_trait]
    impl Seed for FailOnce {
        fn name(&self) -> &str {
            "retry"
        }
        async fn run(&self, db: &Database) -> tideorm::Result<()> {
            db.exec_raw("INSERT INTO seed_effects VALUES(1)").await?;
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(tideorm::Error::query("intentional failure"));
            }
            Ok(())
        }
    }
    let (db, file) = crate::fixtures::file_database("seed-cancel").await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE seed_effects(value INTEGER)")
        .await
        .unwrap();
    let path = format!("{}.tideorm-seeds.lock", file.display());
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    lock.lock().unwrap();
    let seeder = Seeder::new().add(FailOnce(AtomicUsize::new(0)));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), seeder.run())
            .await
            .is_err()
    );
    lock.unlock().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), seeder.run())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), seeder.run())
            .await
            .unwrap()
            .unwrap()
            .executed
            .len(),
        1
    );
    // Explicit reruns keep one ledger row, while still invoking the seed.
    seeder.run_seed("retry").await.unwrap();
    assert_eq!(crate::fixtures::row_count(&db, "_seeds").await, 1);
    assert_eq!(crate::fixtures::row_count(&db, "seed_effects").await, 2);
    drop(lock);
    crate::fixtures::close_global(&db).await;
}

#[tokio::test]
async fn migration_callbacks_can_use_the_separate_seed_lock() {
    struct SeedsInMigration;
    #[async_trait]
    impl tideorm::migration::Migration for SeedsInMigration {
        fn version(&self) -> &str {
            "01"
        }
        fn name(&self) -> &str {
            "seeds"
        }
        async fn up(&self, _: &mut tideorm::migration::Schema) -> tideorm::Result<()> {
            Seeder::new().run().await?;
            Ok(())
        }
        async fn down(&self, _: &mut tideorm::migration::Schema) -> tideorm::Result<()> {
            Ok(())
        }
    }
    let db = crate::fixtures::global_memory_database("").await;
    tokio::time::timeout(
        Duration::from_secs(5),
        tideorm::migration::Migrator::new()
            .add(SeedsInMigration)
            .run(),
    )
    .await
    .unwrap()
    .unwrap();
    crate::fixtures::close_global(&db).await;
}
