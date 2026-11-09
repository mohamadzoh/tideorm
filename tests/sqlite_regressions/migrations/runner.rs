//! The migration lock across independent pools, and the operations the CLI's
//! migration runner maps its commands onto: a number of steps, one migration
//! run or reverted out of turn, marked without running, or pretended, and
//! what a batch a failure stops reports.

use std::time::Duration;
use tideorm::prelude::*;

/// A migration named after its table, running `up` and dropping the table.
struct Sql {
    version: &'static str,
    table: &'static str,
    up: &'static [&'static str],
}

fn sql(version: &'static str, table: &'static str, up: &'static [&'static str]) -> Sql {
    Sql { version, table, up }
}

#[tideorm::async_trait::async_trait]
impl Migration for Sql {
    fn version(&self) -> &str {
        self.version
    }
    fn name(&self) -> &str {
        self.table
    }
    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        for statement in self.up {
            schema.raw(statement).await?;
        }
        Ok(())
    }
    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table(self.table).await
    }
}

/// Writes a row past the schema, then creates `main.fourth`, which SQLite
/// keeps in its main database as `fourth`.
struct Backfill;

#[tideorm::async_trait::async_trait]
impl Migration for Backfill {
    fn version(&self) -> &str {
        "004"
    }
    fn name(&self) -> &str {
        "backfill"
    }
    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        Database::execute("INSERT INTO third (id) VALUES (1)").await?;
        schema
            .create_table("main.fourth", |t| {
                t.id();
                t.string("code");
                t.index(&["code"]);
            })
            .await
    }
    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("main.fourth").await
    }
}

#[tokio::test]
async fn independent_sqlite_pools_share_the_migration_lock() {
    let path = crate::fixtures::TempFile::new("migration-audit", "sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let a = crate::fixtures::single_connection(&url).await;
    let b = crate::fixtures::single_connection(&url).await;
    const ONCE: &[&str] = &["CREATE TABLE audit_once(id INTEGER PRIMARY KEY)"];
    let a_once = Migrator::new()
        .database(&a)
        .add(sql("once", "audit_once", ONCE));
    let b_once = Migrator::new()
        .database(&b)
        .add(sql("once", "audit_once", ONCE));
    let (a_result, b_result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(a_once.run(), b_once.run())
    })
    .await
    .unwrap();
    assert_eq!(
        a_result.unwrap().applied.len() + b_result.unwrap().applied.len(),
        1
    );
    // A failed later migration rolls back its own DDL and releases the lock.
    let failing = sql(
        "fail",
        "audit_rollback",
        &[
            "CREATE TABLE audit_rollback(id INTEGER)",
            "INSERT INTO missing_table VALUES(1)",
        ],
    );
    assert!(
        Migrator::new()
            .database(&a)
            .add(failing)
            .run()
            .await
            .is_err()
    );
    let clean = sql(
        "fail",
        "audit_rollback",
        &["CREATE TABLE audit_rollback(id INTEGER)"],
    );
    let applied = Migrator::new().database(&b).add(clean).run().await.unwrap();
    assert_eq!(applied.applied.len(), 1);
    drop((a, b));
}

/// `sqlite:///C:/..` is how the CLI writes a Windows path, and the SQLite driver then names
/// the file `/C:/..`, which Windows cannot resolve: the lock follows the file
/// SQLite opened, whatever the URL's spelling.
#[tokio::test]
async fn migration_lock_follows_the_opened_file_for_any_url_spelling() {
    let path = crate::fixtures::TempFile::new("migration-url", "sqlite");
    let slashed = path.display().to_string().replace('\\', "/");
    let url = format!("sqlite:///{}?mode=rwc", slashed.trim_start_matches('/'));
    let db = crate::fixtures::single_connection(&url).await;
    let migration = sql(
        "url",
        "audit_url",
        &["CREATE TABLE audit_url(id INTEGER PRIMARY KEY)"],
    );
    let result = Migrator::new().database(&db).add(migration).run().await;
    assert_eq!(result.unwrap().applied.len(), 1);
    let lock = format!("{}.tideorm-migrations.lock", path.display());
    assert!(std::path::Path::new(&lock).exists(), "{lock}");
    drop(db);
}

/// Steps, one migration out of turn either way, marking, and pretending,
/// each refusing a migration in the wrong state or one it cannot name.
#[tokio::test]
async fn a_migrator_runs_reverts_marks_and_pretends_one_migration() {
    let db = crate::fixtures::memory_database("").await;
    let migrator = Migrator::new()
        .database(&db)
        .add(sql("001", "first", &["CREATE TABLE first(id INTEGER)"]))
        .add(sql("002", "second", &["CREATE TABLE second(id INTEGER)"]))
        .add(sql("003", "third", &["CREATE TABLE third(id INTEGER)"]))
        .add(Backfill);
    // The migrations' own tables, without the ledger or `sqlite_sequence`.
    let tables = async || {
        let rows = db
            .query_raw_json(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE '\\_%' ESCAPE '\\' AND name NOT LIKE 'sqlite%' \
                 ORDER BY name",
            )
            .await
            .unwrap();
        rows.iter()
            .map(|row| row["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let applied = async || {
        migrator
            .applied()
            .await
            .unwrap()
            .into_iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>()
    };
    fn error<T>(result: tideorm::Result<T>) -> String {
        result.map(drop).unwrap_err().to_string()
    }

    let ran = migrator.run_steps(1).await.unwrap();
    assert_eq!(ran.applied.len(), 1);
    assert_eq!(tables().await, ["first"]);

    // By name, past the pending `002`.
    let ran = migrator.run_one("third").await.unwrap();
    assert_eq!(ran.applied[0].version, "003");
    assert_eq!(tables().await, ["first", "third"]);
    assert_eq!(applied().await, ["001", "003"]);
    let refused = error(migrator.run_one("003").await);
    assert!(refused.contains("003 - third has already run"), "{refused}");

    let reverted = migrator.rollback_one("001").await.unwrap();
    assert_eq!(reverted.rolled_back[0].name, "first");
    assert_eq!(tables().await, ["third"]);
    let refused = error(migrator.rollback_one("first").await);
    assert!(refused.contains("001 - first has not run"), "{refused}");

    // Marking records or forgets a migration without running it.
    migrator.mark("002", true).await.unwrap();
    assert_eq!(applied().await, ["003", "002"]);
    assert_eq!(tables().await, ["third"]);
    assert!(error(migrator.mark("second", true).await).contains("has already run"));
    migrator.mark("second", false).await.unwrap();
    assert_eq!(applied().await, ["003"]);
    assert!(error(migrator.mark("002", false).await).contains("has not run"));

    // Pretending runs nothing, writes no ledger row, and rolls back what the
    // migration wrote past the schema.
    assert_eq!(
        migrator.pretend("third", false).await.unwrap(),
        [r#"DROP TABLE "third""#]
    );
    let pretended = migrator.pretend("004", true).await.unwrap();
    assert_eq!(pretended.len(), 2, "{pretended:?}");
    assert!(
        pretended[0].starts_with(r#"CREATE TABLE "fourth" ("#),
        "{pretended:?}"
    );
    assert_eq!(
        pretended[1],
        r#"CREATE INDEX "idx_fourth_code" ON "fourth" ("code")"#
    );
    assert_eq!(tables().await, ["third"]);
    assert_eq!(crate::fixtures::row_count(&db, "third").await, 0);
    assert_eq!(applied().await, ["003"]);

    // Run for real, `main.fourth` is the main database's `fourth`.
    migrator.run_one("backfill").await.unwrap();
    assert_eq!(tables().await, ["fourth", "third"]);
    assert_eq!(crate::fixtures::row_count(&db, "third").await, 1);
    migrator.rollback_one("004").await.unwrap();
    assert_eq!(tables().await, ["third"]);

    let unknown = error(migrator.run_one("missing").await);
    assert!(unknown.contains("'missing' is not registered"), "{unknown}");
    let twice = Migrator::new()
        .database(&db)
        .add(sql("005", "same", &[]))
        .add(sql("006", "same", &[]));
    let ambiguous = error(twice.pretend("same", true).await);
    assert!(ambiguous.contains("is ambiguous"), "{ambiguous}");
}

/// A batch command a failing migration stops reports what it applied or
/// rolled back before the failure, which stays applied or rolled back.
#[tokio::test]
async fn a_failed_batch_reports_the_migrations_done_before_it() {
    fn versions(migrations: &[MigrationInfo]) -> Vec<&str> {
        migrations
            .iter()
            .map(|migration| migration.version.as_str())
            .collect()
    }
    let db = crate::fixtures::memory_database("").await;
    let migrator = Migrator::new()
        .database(&db)
        .add(sql("001", "first", &["CREATE TABLE first(id INTEGER)"]))
        .add(sql("002", "second", &["CREATE TABLE second(id INTEGER)"]))
        .add(sql("003", "again", &["CREATE TABLE first(id INTEGER)"]));

    let failure = migrator.run().await.unwrap_err();
    assert!(failure.to_string().contains("first"), "{failure}");
    assert_eq!(versions(&failure.result.applied), ["001", "002"]);
    let error: tideorm::Error = failure.into();
    assert!(error.to_string().contains("first"), "{error}");

    let failure = migrator.refresh().await.unwrap_err();
    assert_eq!(versions(&failure.result.rolled_back), ["002", "001"]);
    assert_eq!(versions(&failure.result.applied), ["001", "002"]);

    // `001`'s `down` drops `gone`, which its `up` never created.
    let db = crate::fixtures::memory_database("").await;
    let migrator = Migrator::new()
        .database(&db)
        .add(sql("001", "gone", &["CREATE TABLE kept(id INTEGER)"]))
        .add(sql("002", "second", &["CREATE TABLE second(id INTEGER)"]));
    migrator.run().await.unwrap();
    let failure = migrator.rollback_steps(2).await.unwrap_err();
    assert_eq!(versions(&failure.result.rolled_back), ["002"]);
    let failure = migrator.reset().await.unwrap_err();
    assert!(failure.result.rolled_back.is_empty());
    assert_eq!(
        migrator
            .applied()
            .await
            .unwrap()
            .iter()
            .map(|migration| migration.version.as_str())
            .collect::<Vec<_>>(),
        ["001"]
    );
}

/// The CLI's runners connect with the application's configuration through
/// `connect_only`, which runs none of the startup work it enables.
#[tokio::test]
async fn connect_only_runs_none_of_the_startup_work() {
    struct Refused;

    #[tideorm::async_trait::async_trait]
    impl tideorm::seeding::Seed for Refused {
        fn name(&self) -> &str {
            "refused"
        }
        async fn run(&self, _db: &Database) -> tideorm::Result<()> {
            Err(tideorm::Error::query("the seed ran"))
        }
    }

    let schema = crate::fixtures::TempFile::new("connect-only", "sql");
    Database::reset_global();
    TideConfig::reset();
    let db = TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .migration(sql("001", "startup", &["NOT A STATEMENT"]))
        .run_migrations(true)
        .seed(Refused)
        .run_seeds(true)
        .sync(true)
        .force_sync(true)
        .schema_file(schema.display().to_string())
        .connect_only()
        .await
        .unwrap();
    // No ledger, and no table a sync of the registered models would create.
    assert_eq!(crate::fixtures::row_count(db, "sqlite_master").await, 0);
    assert!(!schema.exists());
    // `db` is the global handle: close it while it still resolves, then uninstall it.
    crate::fixtures::close(db).await;
    Database::reset_global();
    TideConfig::reset();
}
