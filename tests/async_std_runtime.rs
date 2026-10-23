//! Execute using async-std alone; CI disables TideORM's Tokio runtime feature.
use std::time::Duration;
use tideorm::prelude::*;

#[tideorm::model(table = "runtime_items")]
struct Item {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

#[test]
fn async_std_connections_transactions_and_cancellation() {
    async_std::task::block_on(async {
        let db = Database::builder()
            .url("sqlite::memory:")
            .max_connections(1)
            .build()
            .await
            .unwrap();
        db.exec_raw("CREATE TABLE runtime_items(id INTEGER PRIMARY KEY, name TEXT)")
            .await
            .unwrap();
        let writer = db.clone();
        db.transaction(move |_| {
            Box::pin(async move {
                writer
                    .exec_raw("INSERT INTO runtime_items VALUES(1, 'committed')")
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
        let writer = db.clone();
        let rolled_back: tideorm::Result<()> = db
            .transaction(move |_| {
                Box::pin(async move {
                    writer
                        .exec_raw("UPDATE runtime_items SET name = 'rolled back'")
                        .await?;
                    Err(tideorm::Error::query("roll back"))
                })
            })
            .await;
        assert!(rolled_back.is_err());
        let writer = db.clone();
        let cancelled = db.transaction(move |_| {
            Box::pin(async move {
                writer
                    .exec_raw("UPDATE runtime_items SET name = 'cancelled'")
                    .await?;
                std::future::pending::<()>().await;
                Ok(())
            })
        });
        assert!(
            async_std::future::timeout(Duration::from_millis(100), cancelled)
                .await
                .is_err()
        );
        let item = async_std::future::timeout(
            Duration::from_secs(5),
            Item::query_with(&db).first_or_fail(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(item.name, "committed");
        db.ping().await.unwrap();
        db.__internal_connection()
            .unwrap()
            .close_by_ref()
            .await
            .unwrap();
    });
}

#[test]
fn async_std_migration_lock_wait_can_be_cancelled_and_retried() {
    async_std::task::block_on(async {
        let file =
            std::env::temp_dir().join(format!("tideorm-async-std-{}.sqlite", Uuid::new_v4()));
        let db = Database::builder()
            .url(format!("sqlite://{}?mode=rwc", file.display()))
            .max_connections(1)
            .build()
            .await
            .unwrap();
        Database::set_global(db.clone()).unwrap();
        let lock_path = format!("{}.tideorm-migrations.lock", file.display());
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();
        lock.lock().unwrap();
        let migrator = tideorm::migration::Migrator::new();
        for _ in 0..3 {
            assert!(
                async_std::future::timeout(Duration::from_millis(50), migrator.run())
                    .await
                    .is_err()
            );
        }
        drop(lock);
        async_std::future::timeout(Duration::from_secs(5), migrator.run())
            .await
            .unwrap()
            .unwrap();
        Database::reset_global();
        db.__internal_connection()
            .unwrap()
            .close_by_ref()
            .await
            .unwrap();
        drop(db);
        std::fs::remove_file(file).unwrap();
        std::fs::remove_file(lock_path).unwrap();
    });
}
