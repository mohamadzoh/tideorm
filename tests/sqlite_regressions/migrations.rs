//! Migration locking and schema export on SQLite.

use tideorm::prelude::*;

#[tokio::test]
async fn concurrent_sqlite_migrators_apply_once() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tideorm::migration::{Migration, Migrator, Schema};
    struct Once(Arc<AtomicUsize>);
    #[tideorm::async_trait::async_trait]
    impl Migration for Once {
        fn version(&self) -> &str {
            "audit_once"
        }
        fn name(&self) -> &str {
            "audit_once"
        }
        async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            schema
                .raw("CREATE TABLE audit_migration(id INTEGER PRIMARY KEY)")
                .await
        }
        async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
            schema.raw("DROP TABLE audit_migration").await
        }
    }
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(2)
        .build()
        .await
        .unwrap();
    Database::set_global(db).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let a = Migrator::new().add(Once(calls.clone()));
    let b = Migrator::new().add(Once(calls.clone()));
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(a.run(), b.run())
    })
    .await
    .unwrap();
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(a.applied.len() + b.applied.len(), 1);
    assert_eq!(a.skipped.len() + b.skipped.len(), 1);
}

#[tokio::test]
async fn independent_sqlite_pools_share_the_migration_lock() {
    use tideorm::migration::__run_sql_migration;
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tideorm-migration-audit-{}.sqlite", Uuid::new_v4()));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let a = Database::builder()
        .url(&url)
        .max_connections(1)
        .build()
        .await
        .unwrap();
    let b = Database::builder()
        .url(&url)
        .max_connections(1)
        .build()
        .await
        .unwrap();
    let migration = |db| {
        __run_sql_migration(
            db,
            "_migrations",
            "once",
            "once",
            vec!["CREATE TABLE audit_once(id INTEGER PRIMARY KEY)".into()],
            true,
        )
    };
    let (a_result, b_result) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(migration(&a), migration(&b))
    })
    .await
    .unwrap();
    assert_ne!(a_result.unwrap(), b_result.unwrap());
    // A failed later migration rolls back its own DDL and releases the lock.
    assert!(
        __run_sql_migration(
            &a,
            "_migrations",
            "fail",
            "fail",
            vec![
                "CREATE TABLE audit_rollback(id INTEGER)".into(),
                "INSERT INTO missing_table VALUES(1)".into()
            ],
            true
        )
        .await
        .is_err()
    );
    assert!(
        __run_sql_migration(
            &b,
            "_migrations",
            "fail",
            "fail",
            vec!["CREATE TABLE audit_rollback(id INTEGER)".into()],
            true
        )
        .await
        .unwrap()
    );
    drop((a, b));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.tideorm-migrations.lock", path.display()));
}

/// `sqlite:///C:/..` is how the CLI writes a Windows path, and SQLx then names
/// the file `/C:/..`, which Windows cannot resolve: the lock follows the file
/// SQLite opened, whatever the URL's spelling.
#[tokio::test]
async fn migration_lock_follows_the_opened_file_for_any_url_spelling() {
    use tideorm::migration::__run_sql_migration;
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tideorm-migration-url-{}.sqlite", Uuid::new_v4()));
    let slashed = path.display().to_string().replace('\\', "/");
    let url = format!("sqlite:///{}?mode=rwc", slashed.trim_start_matches('/'));
    let db = Database::builder()
        .url(&url)
        .max_connections(1)
        .build()
        .await
        .unwrap();
    let applied = __run_sql_migration(
        &db,
        "_migrations",
        "url",
        "url",
        vec!["CREATE TABLE audit_url(id INTEGER PRIMARY KEY)".into()],
        true,
    )
    .await
    .unwrap();
    assert!(applied);
    let lock = format!("{}.tideorm-migrations.lock", path.display());
    assert!(std::path::Path::new(&lock).exists(), "{lock}");
    drop(db);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&lock);
}

#[tokio::test]
async fn sqlite_export_keeps_names_similar_to_reserved_prefix() {
    let source = Database::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE sqlitex_items(id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE normal(id INTEGER)",
        "CREATE INDEX sqlitex_name ON sqlitex_items(name)",
        "CREATE TRIGGER sqlitex_insert AFTER INSERT ON sqlitex_items BEGIN INSERT INTO normal VALUES(NEW.id); END",
    ] {
        source.exec_raw(sql).await.unwrap();
    }
    Database::set_global(source).unwrap();
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("tideorm-prefix-{}.sql", uuid::Uuid::new_v4()));
    tideorm::schema::SchemaWriter::write_schema(&path)
        .await
        .unwrap();
    let script = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let restored = Database::connect("sqlite::memory:").await.unwrap();
    restored.exec_raw(&script).await.unwrap();
    restored
        .exec_raw("INSERT INTO sqlitex_items VALUES(7,'kept')")
        .await
        .unwrap();
    assert_eq!(
        restored
            .query_raw_json("SELECT id FROM normal")
            .await
            .unwrap()[0]["id"],
        7
    );
    assert_eq!(
        restored
            .query_raw_json(
                "SELECT count(*) AS n FROM sqlite_master WHERE type='index' AND name='sqlitex_name'"
            )
            .await
            .unwrap()[0]["n"],
        1
    );
    Database::reset_global();
}
