//! Migration locking and schema export on SQLite.

use tideorm::prelude::*;

#[tokio::test]
async fn schema_export_ignores_shadow_names_in_other_schemas() {
    for schema in ["aux", "temp"] {
        let db = crate::fixtures::global_memory_database(
            "CREATE TABLE search_data(id INTEGER, name TEXT); \
             CREATE UNIQUE INDEX important_unique ON search_data(name); \
             CREATE TRIGGER important_trigger AFTER INSERT ON search_data \
             BEGIN UPDATE search_data SET id=10 WHERE rowid=NEW.rowid; END; \
             CREATE VIRTUAL TABLE main.own_search USING fts5(body); \
             ATTACH DATABASE ':memory:' AS aux",
        )
        .await;
        db.exec_raw(&format!(
            "CREATE VIRTUAL TABLE {schema}.search USING fts5(body)"
        ))
        .await
        .unwrap();
        let file = crate::fixtures::TempFile::new("shadow-export", "sql");
        tideorm::schema::SchemaWriter::write_schema(&*file)
            .await
            .unwrap();
        let sql = std::fs::read_to_string(&*file).unwrap();
        assert!(sql.contains("important_unique"), "{schema}: {sql}");
        assert!(sql.contains("important_trigger"), "{schema}: {sql}");
        // Main's virtual table recreates its own shadow tables during replay.
        assert!(!sql.contains("CREATE TABLE 'own_search_data'"), "{sql}");
        let restored = crate::fixtures::memory_database(&sql).await;
        restored
            .exec_raw("INSERT INTO search_data VALUES(1, 'same')")
            .await
            .unwrap();
        assert_eq!(
            restored
                .query_raw_json("SELECT id FROM search_data")
                .await
                .unwrap()[0]["id"],
            10
        );
        assert!(
            restored
                .exec_raw("INSERT INTO search_data VALUES(2, 'same')")
                .await
                .is_err()
        );
        restored
            .exec_raw("INSERT INTO own_search(body) VALUES('restored')")
            .await
            .unwrap();
        Database::reset_global();
    }
}

#[tokio::test]
async fn whole_migration_operations_exclude_concurrent_runners() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use tideorm::migration::{Migration, Migrator, Schema};
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Gate {
        entered: Notify,
        release: Notify,
        downs: AtomicUsize,
    }
    struct Step(&'static str, Arc<Gate>);
    #[tideorm::async_trait::async_trait]
    impl Migration for Step {
        fn version(&self) -> &str {
            self.0
        }
        fn name(&self) -> &str {
            self.0
        }
        async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
            schema
                .raw(&format!("CREATE TABLE step_{}(id INTEGER)", self.0))
                .await
        }
        async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
            if self.0 == "02" && self.1.downs.fetch_add(1, Ordering::SeqCst) == 0 {
                self.1.entered.notify_one();
                self.1.release.notified().await;
            }
            schema.raw(&format!("DROP TABLE step_{}", self.0)).await
        }
    }

    tokio::time::timeout(Duration::from_secs(10), async {
        for operation in ["reset", "steps", "refresh"] {
            let db = Database::builder()
                .url("sqlite::memory:")
                .max_connections(3)
                .build()
                .await
                .unwrap();
            Database::set_global(db).unwrap();
            let gate = Arc::new(Gate::default());
            let migrator = Arc::new(
                Migrator::new()
                    .add(Step("01", gate.clone()))
                    .add(Step("02", gate.clone())),
            );
            migrator.run().await.unwrap();
            let rollback = migrator.clone();
            let rollback = tokio::spawn(async move {
                match operation {
                    "reset" => rollback.reset().await,
                    "steps" => rollback.rollback_steps(2).await,
                    _ => rollback.refresh().await,
                }
            });
            gate.entered.notified().await;
            let runner = tokio::spawn(async move { migrator.run().await });
            // Give the other runner time to queue while the first down() is held.
            tokio::time::sleep(Duration::from_millis(100)).await;
            gate.release.notify_one();
            let rolled_back = rollback.await.unwrap().unwrap();
            let ran = runner.await.unwrap().unwrap();
            assert_eq!(
                rolled_back
                    .rolled_back
                    .iter()
                    .map(|m| m.version.as_str())
                    .collect::<Vec<_>>(),
                ["02", "01"],
                "{operation}"
            );
            if operation == "refresh" {
                assert_eq!(rolled_back.applied.len(), 2);
                assert_eq!(ran.skipped.len(), 2);
            } else {
                assert_eq!(ran.applied.len(), 2);
            }
            Database::reset_global();
        }
    })
    .await
    .unwrap();
}

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
    let path = crate::fixtures::TempFile::new("migration-audit", "sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let a = crate::fixtures::single_connection(&url).await;
    let b = crate::fixtures::single_connection(&url).await;
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
}

/// `sqlite:///C:/..` is how the CLI writes a Windows path, and SQLx then names
/// the file `/C:/..`, which Windows cannot resolve: the lock follows the file
/// SQLite opened, whatever the URL's spelling.
#[tokio::test]
async fn migration_lock_follows_the_opened_file_for_any_url_spelling() {
    use tideorm::migration::__run_sql_migration;
    let path = crate::fixtures::TempFile::new("migration-url", "sqlite");
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
    let path = crate::fixtures::TempFile::new("prefix", "sql");
    tideorm::schema::SchemaWriter::write_schema(&*path)
        .await
        .unwrap();
    let script = std::fs::read_to_string(&*path).unwrap();
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

/// A migrator run inside a transaction on a one-connection pool finds the
/// lock without asking the pool for the connection the transaction holds.
#[tokio::test]
async fn migrator_runs_inside_a_transaction_on_a_one_connection_pool() {
    use tideorm::migration::{Migration, Migrator, Schema};
    struct CreateTable;
    #[tideorm::async_trait::async_trait]
    impl Migration for CreateTable {
        fn version(&self) -> &str {
            "in_transaction"
        }
        fn name(&self) -> &str {
            "in_transaction"
        }
        async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
            schema
                .raw("CREATE TABLE migrated_in_transaction(id INTEGER PRIMARY KEY)")
                .await
        }
        async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
            schema.raw("DROP TABLE migrated_in_transaction").await
        }
    }
    let db = crate::fixtures::global_memory_database("").await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        db.transaction(|_| Box::pin(async { Migrator::new().add(CreateTable).run().await })),
    )
    .await
    .expect("the migrator waited for a second connection");
    assert_eq!(result.unwrap().applied.len(), 1);
    assert_eq!(
        db.query_raw_json(
            "SELECT count(*) AS n FROM sqlite_master WHERE name = 'migrated_in_transaction'"
        )
        .await
        .unwrap()[0]["n"],
        1
    );
    Database::reset_global();
}

/// SQLite refuses `ADD COLUMN .. UNIQUE`, so a unique column added to a table
/// that has rows comes with a unique index that enforces it instead.
#[tokio::test]
async fn sqlite_adds_a_unique_column_through_a_unique_index() {
    use tideorm::config::DatabaseType;
    use tideorm::migration::{ColumnType, Schema};
    let db = crate::fixtures::global_memory_database(
        "CREATE TABLE unique_additions(id INTEGER PRIMARY KEY); \
         INSERT INTO unique_additions VALUES (1), (2)",
    )
    .await;
    Schema::new(DatabaseType::SQLite)
        .alter_table("unique_additions", |table| {
            table.add_column("email", ColumnType::Text).unique();
        })
        .await
        .unwrap();
    db.exec_raw("UPDATE unique_additions SET email = 'a@x' WHERE id = 1")
        .await
        .unwrap();
    assert!(
        db.exec_raw("UPDATE unique_additions SET email = 'a@x' WHERE id = 2")
            .await
            .is_err()
    );
    // The rollback drops the index with the column.
    Schema::new(DatabaseType::SQLite)
        .alter_table("unique_additions", |table| {
            table.drop_column("email");
        })
        .await
        .unwrap();
    Database::reset_global();
}

/// Dropping a column on SQLite drops the unique index TideORM gave it there
/// and no other: `user_roles`' index on `email` has the name a unique
/// `roles_email` column of `user` would get, and keeps enforcing.
#[tokio::test]
async fn sqlite_drop_column_leaves_another_tables_index_of_the_same_name() {
    use tideorm::config::DatabaseType;
    use tideorm::migration::Schema;
    let db = crate::fixtures::global_memory_database(
        "CREATE TABLE user(id INTEGER PRIMARY KEY, roles_email TEXT); \
         CREATE TABLE user_roles(id INTEGER PRIMARY KEY, email TEXT); \
         CREATE UNIQUE INDEX idx_user_roles_email_unique ON user_roles(email); \
         INSERT INTO user_roles VALUES (1, 'a@x')",
    )
    .await;
    Schema::new(DatabaseType::SQLite)
        .alter_table("user", |table| {
            table.drop_column("roles_email");
        })
        .await
        .unwrap();
    assert!(
        db.exec_raw("INSERT INTO user_roles VALUES (2, 'a@x')")
            .await
            .is_err(),
        "user_roles.email lost its unique index"
    );
    Database::reset_global();
}

#[tokio::test]
async fn dropping_a_renamed_unique_column_removes_its_original_index() {
    use tideorm::config::DatabaseType;
    use tideorm::migration::{ColumnType, Schema};
    let db = crate::fixtures::global_memory_database(
        "CREATE TABLE renamed_unique(id INTEGER PRIMARY KEY, keep TEXT); \
         CREATE INDEX keep_index ON renamed_unique(keep); \
         INSERT INTO renamed_unique VALUES(1,'preserved')",
    )
    .await;
    let mut schema = Schema::new(DatabaseType::SQLite);
    schema
        .alter_table("renamed_unique", |table| {
            table
                .add_column("code", ColumnType::String)
                .nullable()
                .unique();
        })
        .await
        .unwrap();
    schema
        .alter_table("renamed_unique", |table| {
            table.rename_column("code", "renamed");
        })
        .await
        .unwrap();
    schema
        .rename_table("renamed_unique", "renamed_table")
        .await
        .unwrap();
    schema
        .alter_table("renamed_table", |table| {
            table.drop_column("renamed");
        })
        .await
        .unwrap();
    // Renames in the same alteration must track both catalog indexes and
    // indexes created by earlier operations of that alteration.
    for added_in_same_call in [false, true] {
        if !added_in_same_call {
            schema
                .alter_table("renamed_table", |table| {
                    table
                        .add_column("code", ColumnType::String)
                        .nullable()
                        .unique();
                })
                .await
                .unwrap();
        }
        schema
            .alter_table("renamed_table", |table| {
                if added_in_same_call {
                    table
                        .add_column("code", ColumnType::String)
                        .nullable()
                        .unique();
                }
                table.rename_column("code", "intermediate");
                table.rename_column("intermediate", "renamed");
                table.drop_column("renamed");
            })
            .await
            .unwrap();
    }
    assert_eq!(
        db.query_raw_json("SELECT * FROM renamed_table")
            .await
            .unwrap(),
        vec![serde_json::json!({"id":1,"keep":"preserved"})]
    );
    let indexes = db
        .query_raw_json("SELECT name FROM pragma_index_list('renamed_table')")
        .await
        .unwrap();
    assert_eq!(indexes, vec![serde_json::json!({"name":"keep_index"})]);
    Database::reset_global();
}

#[tokio::test]
async fn schema_sync_rejects_index_names_owned_by_another_table() {
    use tideorm::model::IndexDefinition;
    use tideorm::sync::{ColumnDef, ModelSchema, SyncRegistry, sync_database};
    for preexisting in [false, true] {
        let db = crate::fixtures::memory_database("").await;
        if preexisting {
            db.exec_raw(
                "CREATE TABLE collision_first(id INTEGER PRIMARY KEY, code TEXT); \
                         CREATE TABLE collision_second(id INTEGER PRIMARY KEY, code TEXT)",
            )
            .await
            .unwrap();
        }
        SyncRegistry::clear();
        for table in ["collision_first", "collision_second"] {
            SyncRegistry::register_schema(
                ModelSchema::new(table)
                    .column(ColumnDef::new("id", "i64").primary_key())
                    .column(ColumnDef::new("code", "String"))
                    .indexes(vec![IndexDefinition {
                        name: "shared_unique".into(),
                        columns: vec!["code".into()],
                        unique: true,
                    }]),
            );
        }
        let error = sync_database(&db).await.unwrap_err().to_string();
        assert!(
            error.contains("shared_unique") && error.contains("collision_first"),
            "{error}"
        );
        // A repeated sync must report the same collision on the now-existing tables.
        assert!(sync_database(&db).await.is_err());
        assert!(
            db.exec_raw("INSERT INTO collision_first VALUES(1,'same'),(2,'same')")
                .await
                .is_err()
        );
        SyncRegistry::clear();
    }
}

/// A migration rebuilding a table runs with SQLite's foreign keys off when it
/// asks to, through `Migration::foreign_keys_off`, so the rows pointing at the
/// table keep pointing at it, where the `DROP TABLE` deleted them. Without
/// asking, the pragma is refused rather than ignored, and a migration leaving
/// a foreign key pointing nowhere is rolled back.
#[tokio::test]
async fn a_table_rebuild_with_foreign_keys_off_keeps_the_rows_pointing_at_it() {
    use tideorm::migration::{Migration, Migrator, Schema};

    const REBUILD: [&str; 4] = [
        "CREATE TABLE parents_new(id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "INSERT INTO parents_new SELECT id, name FROM parents",
        "DROP TABLE parents",
        "ALTER TABLE parents_new RENAME TO parents",
    ];

    struct Rebuild {
        version: &'static str,
        off: bool,
        orphan: bool,
    }

    #[tideorm::async_trait::async_trait]
    impl Migration for Rebuild {
        fn version(&self) -> &str {
            self.version
        }

        fn name(&self) -> &str {
            self.version
        }

        async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
            schema.raw("PRAGMA foreign_keys = OFF").await?;
            for sql in REBUILD {
                schema.raw(sql).await?;
            }
            if self.orphan {
                schema.raw("DELETE FROM parents").await?;
            }
            schema.raw("PRAGMA foreign_keys = ON").await
        }

        async fn down(&self, _schema: &mut Schema) -> tideorm::Result<()> {
            Ok(())
        }

        fn foreign_keys_off(&self) -> bool {
            self.off
        }
    }

    let (db, _file) = crate::fixtures::file_database("rebuild").await;
    db.exec_raw(
        "CREATE TABLE parents(id INTEGER PRIMARY KEY, name TEXT); \
         CREATE TABLE children(id INTEGER PRIMARY KEY, \
             parent_id INTEGER REFERENCES parents(id) ON DELETE CASCADE); \
         INSERT INTO parents VALUES (1, 'p'); INSERT INTO children VALUES (1, 1)",
    )
    .await
    .unwrap();
    Database::set_global(db.clone()).unwrap();
    let count = |table: &'static str| {
        let db = db.clone();
        async move {
            db.query_raw_json(&format!("SELECT id FROM {table}"))
                .await
                .unwrap()
                .len()
        }
    };
    let run = |version, off, orphan| async move {
        Migrator::new()
            .add(Rebuild {
                version,
                off,
                orphan,
            })
            .run()
            .await
            .map(drop)
    };

    let refused = run("refused", false, false).await.unwrap_err().to_string();
    assert!(refused.contains("foreign_keys_off"), "{refused}");
    let orphaned = run("orphan", true, true).await.unwrap_err().to_string();
    assert!(orphaned.contains("matches no row"), "{orphaned}");
    assert_eq!(count("parents").await, 1);
    run("rebuild", true, false).await.unwrap();
    assert_eq!(count("children").await, 1);

    // The pool's own connections still enforce foreign keys.
    assert!(
        db.exec_raw("INSERT INTO children VALUES (2, 99)")
            .await
            .is_err()
    );

    // A row that already pointed nowhere is the database's, not the migration's,
    // and is reported before the migration runs.
    db.exec_raw(
        "PRAGMA foreign_keys = OFF; INSERT INTO children VALUES (3, 77); PRAGMA foreign_keys = ON",
    )
    .await
    .unwrap();
    let existing = run("existing", true, false).await.unwrap_err().to_string();
    assert!(
        existing.contains("already has 1 row(s) whose foreign key matches no row"),
        "{existing}"
    );

    Database::reset_global();
    drop(db);
}

/// A pragma turning foreign keys off must not let a rebuild silently
/// cascade-delete children while SQLite ignores it in a transaction; the
/// spellings it is recognized in are unit-tested with `disables_foreign_keys`.
#[tokio::test]
async fn migration_foreign_key_guard_handles_sqlite_pragma_spellings() {
    use tideorm::migration::{Migration, Migrator, Schema};

    struct Rebuild(&'static str);
    #[tideorm::async_trait::async_trait]
    impl Migration for Rebuild {
        fn version(&self) -> &str {
            "guarded_rebuild"
        }
        fn name(&self) -> &str {
            "guarded_rebuild"
        }
        async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
            schema.raw(self.0).await?;
            schema.raw("DROP TABLE pragma_parents").await
        }
        async fn down(&self, _: &mut Schema) -> tideorm::Result<()> {
            Ok(())
        }
    }

    for sql in [
        "PRAGMA foreign_keys(OFF)",
        "PRAGMA foreign_keys = ON; PRAGMA foreign_keys = OFF;",
    ] {
        let db = crate::fixtures::global_memory_database(
            "CREATE TABLE pragma_parents(id INTEGER PRIMARY KEY); \
             CREATE TABLE pragma_children(parent_id INTEGER REFERENCES pragma_parents(id) ON DELETE CASCADE); \
             INSERT INTO pragma_parents VALUES(1); INSERT INTO pragma_children VALUES(1)",
        ).await;
        let mut schema = Schema::new(tideorm::config::DatabaseType::SQLite);
        schema.raw("PRAGMA foreign_keys = 257").await.unwrap();
        // Outside a migration the pragma would change one pooled connection only.
        let refusal = schema.raw(sql).await.unwrap_err().to_string();
        assert!(
            refusal.contains("one connection of a pool"),
            "{sql}: {refusal}"
        );
        let outcome = Migrator::new().add(Rebuild(sql)).run().await;
        let children = db
            .query_raw_json("SELECT count(*) AS n FROM pragma_children")
            .await
            .unwrap();
        assert!(
            outcome.is_err(),
            "{sql} was accepted, remaining children: {children:?}"
        );
        assert!(
            outcome
                .unwrap_err()
                .to_string()
                .contains("foreign_keys_off")
        );
        assert_eq!(children[0]["n"], 1, "{sql}");
        assert_eq!(
            db.query_raw_json("SELECT count(*) AS n FROM pragma_parents")
                .await
                .unwrap()[0]["n"],
            1
        );
        Database::reset_global();
    }
}
#[tokio::test]
async fn sqlite_drop_column_uses_the_index_owners_schema() {
    use tideorm::migration::Schema;
    for owner in ["main", "temp", "aux"] {
        let db = crate::fixtures::global_memory_database("").await;
        db.exec_raw(&format!(
            "ATTACH DATABASE ':memory:' AS aux;
            CREATE TABLE {owner}.items(id INTEGER PRIMARY KEY, code TEXT);
            CREATE UNIQUE INDEX {owner}.collision ON items(code);
            INSERT INTO {owner}.items VALUES(1,'keep');"
        ))
        .await
        .unwrap();
        for other in ["main", "temp", "aux"].into_iter().filter(|s| *s != owner) {
            db.exec_raw(&format!(
                "CREATE TABLE {other}.staging(other_code TEXT);
                CREATE UNIQUE INDEX {other}.collision ON staging(other_code);
                INSERT INTO {other}.staging VALUES('unique');"
            ))
            .await
            .unwrap();
        }
        let mut schema = Schema::new(tideorm::config::DatabaseType::SQLite);
        schema
            .alter_table("items", |table| {
                table.drop_column("code");
            })
            .await
            .unwrap();
        assert_eq!(
            db.query_raw_json(&format!("SELECT * FROM {owner}.items"))
                .await
                .unwrap(),
            vec![serde_json::json!({"id":1})]
        );
        for other in ["main", "temp", "aux"].into_iter().filter(|s| *s != owner) {
            assert!(
                db.exec_raw(&format!("INSERT INTO {other}.staging VALUES('unique')"))
                    .await
                    .is_err()
            );
        }
        let distractor_schema = if owner == "temp" { "main" } else { "temp" };
        db.exec_raw(&format!(
            "CREATE TABLE {distractor_schema}.distractor(code TEXT);
            CREATE UNIQUE INDEX {distractor_schema}.idx_items_added_unique ON distractor(code);
            INSERT INTO {distractor_schema}.distractor VALUES('unique');"
        ))
        .await
        .unwrap();
        schema
            .alter_table("items", |table| {
                table
                    .add_column("added", tideorm::migration::ColumnType::Text)
                    .nullable()
                    .unique();
                table.rename_column("added", "renamed");
                table.drop_column("renamed");
            })
            .await
            .unwrap();
        assert!(
            db.exec_raw("INSERT INTO distractor VALUES('unique')")
                .await
                .is_err()
        );
        Database::reset_global();
    }
}
