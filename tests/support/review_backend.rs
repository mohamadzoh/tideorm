//! Scenarios the PostgreSQL, MySQL and MariaDB targets run, and SQLite does
//! not: mostly `SchemaWriter` exports replayed into the server, plus upserts,
//! array and JSON filters and assignments, transactions, migrator locking,
//! cache invalidation and raw row decoding there. Each returns early on a
//! backend it does not concern.
//!
//! The including target supplies `mod backend` with `DATABASE_TYPE`,
//! `database_url()` and `async fn connect() -> bool`, and `mod parity`, whose
//! schema-export helpers these scenarios share.

use super::backend;
use super::parity::{export_statements, exported_schema};
use std::time::Duration;
use tideorm::prelude::*;
use tideorm::schema::SchemaWriter;

/// Requires CREATE DATABASE, as does the MySQL complete-export regression.
/// Restore every statement on one connection so session setup and cross-schema
/// dependencies are tested together, without other fixtures in either database.
#[tokio::test]
async fn review_postgres_complete_export_restores_cross_schema_dependencies() {
    use futures_util::FutureExt;

    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let admin = tideorm::require_db().unwrap();
    let source_name = format!("review_source_{}", uuid::Uuid::new_v4().simple());
    let destination_name = format!("review_destination_{}", uuid::Uuid::new_v4().simple());
    let restore = async {
        for name in [&source_name, &destination_name] {
            admin
                .exec_raw(&format!(
                    "CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8'"
                ))
                .await
                .unwrap();
        }
        let mut url = url::Url::parse(backend::database_url()).unwrap();
        url.set_path(&source_name);
        let source = Database::builder()
            .url(url.as_str())
            .max_connections(1)
            .build()
            .await
            .unwrap();
        url.set_path(&destination_name);
        let destination = Database::builder()
            .url(url.as_str())
            .max_connections(1)
            .build()
            .await
            .unwrap();
        for sql in [
            "CREATE SCHEMA app",
            "CREATE SCHEMA shared",
            "CREATE SEQUENCE shared.serial START 10",
            "CREATE DOMAIN shared.identifier AS bigint DEFAULT nextval('shared.serial') CHECK (VALUE > 0)",
            "CREATE FUNCTION shared.label(n bigint) RETURNS text LANGUAGE SQL AS $$ SELECT 'item-' || n::text $$",
            "CREATE TABLE app.items(id shared.identifier PRIMARY KEY, name text NOT NULL UNIQUE)",
            "CREATE INDEX items_label ON app.items (lower(name))",
            "CREATE VIEW shared.items AS SELECT id, shared.label(id) AS label FROM app.items",
            "CREATE VIEW app.visible_items AS SELECT * FROM shared.items",
            "CREATE COLLATION app.eq_ab (provider=icu,locale='und',deterministic=false,rules='& a = b')",
            "CREATE TABLE app.names(value text COLLATE app.eq_ab UNIQUE)",
            "ALTER TABLE app.items CLUSTER ON items_label",
            "CREATE TABLE app.parent(id integer PRIMARY KEY)",
            "CREATE TABLE app.child(id integer PRIMARY KEY,parent_id integer REFERENCES app.parent(id))",
            "ALTER TABLE app.child DISABLE TRIGGER ALL",
            "CREATE TABLE app.self_ref(id integer PRIMARY KEY,parent_id integer REFERENCES app.self_ref(id))",
            "ALTER TABLE app.self_ref DISABLE TRIGGER ALL",
            "CREATE TABLE app.part_children(id integer,parent_id integer REFERENCES app.parent(id)) PARTITION BY RANGE(id)",
            "CREATE TABLE app.child_leaf PARTITION OF app.part_children FOR VALUES FROM (0) TO (100)",
            "ALTER TABLE ONLY app.child_leaf DISABLE TRIGGER ALL",
            "CREATE INDEX part_id ON app.part_children(id)",
            "ALTER INDEX app.child_leaf_id_idx RENAME TO special_cluster",
            "ALTER TABLE app.child_leaf CLUSTER ON special_cluster",
        ] {
            source.exec_raw(sql).await.unwrap();
        }
        Database::set_global(source.clone()).unwrap();
        let script = exported_schema(&source_name).await;
        destination
            .exec_raw("SET search_path=public")
            .await
            .unwrap();
        for statement in export_statements(&script) {
            destination
                .exec_raw(statement)
                .await
                .unwrap_or_else(|error| panic!("{error}\n{statement}"));
        }
        // Unique equality, disabled internal triggers and clustering metadata
        // must survive replay, including self references and partition clones.
        destination
            .exec_raw("INSERT INTO app.names VALUES ('a')")
            .await
            .unwrap();
        assert!(
            destination
                .exec_raw("INSERT INTO app.names VALUES ('b')")
                .await
                .is_err()
        );
        destination
            .exec_raw("INSERT INTO app.child VALUES (1,999)")
            .await
            .unwrap();
        destination
            .exec_raw("INSERT INTO app.self_ref VALUES (1,999)")
            .await
            .unwrap();
        destination
            .exec_raw("INSERT INTO app.child_leaf VALUES (1,999)")
            .await
            .unwrap();
        destination.exec_raw("CLUSTER app.items").await.unwrap();
        destination
            .exec_raw("CLUSTER app.child_leaf")
            .await
            .unwrap();
        destination
            .exec_raw("INSERT INTO app.items(name) VALUES ('Alice')")
            .await
            .unwrap();
        assert_eq!(
            destination
                .query_raw_json("SELECT id, label FROM app.visible_items")
                .await
                .unwrap(),
            vec![serde_json::json!({"id": 10, "label": "item-10"})]
        );
        assert!(
            destination
                .exec_raw("INSERT INTO app.items(id,name) VALUES (-1,'invalid')")
                .await
                .is_err()
        );
        assert!(
            destination
                .exec_raw("INSERT INTO app.items(name) VALUES ('Alice')")
                .await
                .is_err()
        );
        assert_eq!(
            source
                .query_raw_json("SELECT count(*) AS n FROM app.items")
                .await
                .unwrap()[0]["n"],
            0
        );
    };
    let result = std::panic::AssertUnwindSafe(restore).catch_unwind().await;

    Database::set_global(admin.clone()).unwrap();
    // FORCE releases pooled sessions, including those retained after a panic.
    // Attempt both cleanups before propagating the original test failure.
    let source_cleanup = admin
        .exec_raw(&format!(
            "DROP DATABASE IF EXISTS {source_name} WITH (FORCE)"
        ))
        .await;
    let destination_cleanup = admin
        .exec_raw(&format!(
            "DROP DATABASE IF EXISTS {destination_name} WITH (FORCE)"
        ))
        .await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    source_cleanup.unwrap();
    destination_cleanup.unwrap();
}

#[tokio::test]
async fn review_concurrent_migrators_leave_a_single_connection_pool_available() {
    if !backend::connect().await {
        return;
    }
    let db = Database::builder()
        .url(backend::database_url())
        .max_connections(1)
        .min_connections(0)
        .acquire_timeout(Duration::from_secs(2))
        .build()
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    let first = tideorm::migration::Migrator::new().migrations_table("review_lock_ledger");
    let second = tideorm::migration::Migrator::new().migrations_table("review_lock_ledger");
    let (a, b) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(first.run(), second.run())
    })
    .await
    .expect("migrators starved their query pool");
    a.unwrap();
    b.unwrap();
    db.exec_raw("DROP TABLE review_lock_ledger").await.unwrap();
}

#[tokio::test]
async fn review_postgres_export_restores_types_identity_and_index_semantics() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_export CASCADE",
        "CREATE SCHEMA review_export",
        "CREATE TYPE review_export.state AS ENUM ('new', 'done')",
        "CREATE DOMAIN review_export.status AS review_export.state NOT NULL",
        "CREATE TYPE review_export.detail AS (state review_export.status, note text)",
        "CREATE COLLATION review_export.casefold (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
        "CREATE TABLE review_export.items (id bigint GENERATED ALWAYS AS IDENTITY (START WITH 100 INCREMENT BY 7 CACHE 3) PRIMARY KEY, state review_export.status, detail review_export.detail, states review_export.state[], name text NOT NULL)",
        "CREATE UNIQUE INDEX review_casefold ON review_export.items (name COLLATE review_export.casefold)",
        "CREATE INDEX review_pattern ON review_export.items (name text_pattern_ops DESC NULLS FIRST)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let sql = exported_schema("review_pg_export").await;
    assert!(sql.contains("GENERATED ALWAYS AS IDENTITY"), "{sql}");
    assert!(sql.contains("INCREMENT BY 7"), "{sql}");
    assert!(sql.contains("text_pattern_ops DESC"), "{sql}");
    db.exec_raw("DROP SCHEMA review_export CASCADE")
        .await
        .unwrap();
    // Only recreate the objects owned by this test; the export may include
    // other scenarios' tables in the integration test database.
    for statement in export_statements(&sql)
        .into_iter()
        .filter(|statement| statement.contains("review_export"))
    {
        db.exec_raw(statement).await.unwrap();
    }
    assert!(
        db.exec_raw("INSERT INTO review_export.items(id, state, name) VALUES(1, 'new', 'manual')")
            .await
            .is_err()
    );
    db.exec_raw(
        "INSERT INTO review_export.items(state, name) VALUES('new', 'Alice'), ('done', 'Bob')",
    )
    .await
    .unwrap();
    let rows = db
        .query_raw_json("SELECT id FROM review_export.items ORDER BY id")
        .await
        .unwrap();
    assert_eq!(rows[0]["id"], 100);
    assert_eq!(rows[1]["id"], 107);
    assert!(
        db.exec_raw("INSERT INTO review_export.items(state, name) VALUES('new', 'alice')")
            .await
            .is_err()
    );
    let indexes = db.query_raw_json("SELECT indexdef FROM pg_indexes WHERE schemaname = 'review_export' AND indexname = 'review_pattern'").await.unwrap();
    assert!(
        indexes[0]["indexdef"]
            .as_str()
            .unwrap()
            .contains("text_pattern_ops DESC")
    );
    db.exec_raw("DROP SCHEMA review_export CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_export_keeps_on_update_timestamp() {
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("DROP TABLE IF EXISTS review_timestamp")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_timestamp(id BIGINT PRIMARY KEY, name TEXT, updated_at TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6))").await.unwrap();
    let sql = exported_schema("review_mysql_export").await;
    assert!(
        sql.to_ascii_uppercase()
            .contains("ON UPDATE CURRENT_TIMESTAMP(6)"),
        "{sql}"
    );
    db.exec_raw("DROP TABLE review_timestamp").await.unwrap();
    for statement in export_statements(&sql)
        .into_iter()
        .filter(|statement| statement.contains("review_timestamp"))
    {
        db.exec_raw(statement).await.unwrap();
    }
    db.exec_raw("INSERT INTO review_timestamp VALUES(1, 'before', '2000-01-01 00:00:00')")
        .await
        .unwrap();
    db.exec_raw("UPDATE review_timestamp SET name='after' WHERE id=1")
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT YEAR(updated_at) AS changed_year FROM review_timestamp WHERE id=1")
        .await
        .unwrap();
    assert!(rows[0]["changed_year"].as_i64().unwrap() > 2000);
    db.exec_raw("DROP TABLE review_timestamp").await.unwrap();
}

/// Export the database, drop `schema`, and recreate it from the exported
/// statements that name it, in one transaction, so the export's session
/// settings (`check_function_bodies`) apply to every object.
async fn replay_review_schema(db: &Database, schema: &str) {
    let sql = exported_schema(schema).await;
    db.exec_raw(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
    let statements: Vec<String> = export_statements(&sql)
        .into_iter()
        .filter(|statement| {
            statement.contains(schema) || statement.contains("check_function_bodies")
        })
        .map(str::to_owned)
        .collect();
    db.transaction(move |_| {
        Box::pin(async move {
            for statement in statements {
                Database::execute(&statement)
                    .await
                    .unwrap_or_else(|e| panic!("{e}\n{statement}"));
            }
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn review_postgres_column_collation_survives_export() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_column_collation CASCADE",
        "CREATE SCHEMA review_column_collation",
        "CREATE COLLATION review_column_collation.ci (provider=icu, locale='und-u-ks-level2', deterministic=false)",
        "CREATE TABLE review_column_collation.names(name TEXT COLLATE review_column_collation.ci UNIQUE)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_column_collation").await;
    db.exec_raw("INSERT INTO review_column_collation.names VALUES('Alice')")
        .await
        .unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_column_collation.names VALUES('alice')")
            .await
            .is_err()
    );
    let rows = db
        .query_raw_json("SELECT name FROM review_column_collation.names WHERE name='ALICE'")
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    db.exec_raw("DROP SCHEMA review_column_collation CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_partitions_survive_export_in_parent_order() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_partitions CASCADE",
        "CREATE SCHEMA review_partitions",
        "CREATE TABLE review_partitions.z_parent(id INTEGER PRIMARY KEY CHECK(id>=0), name TEXT) PARTITION BY RANGE(id)",
        "CREATE TABLE review_partitions.b_low PARTITION OF review_partitions.z_parent FOR VALUES FROM (0) TO (10) PARTITION BY RANGE(id)",
        "CREATE TABLE review_partitions.a_leaf PARTITION OF review_partitions.b_low FOR VALUES FROM (0) TO (5)",
        "CREATE TABLE review_partitions.c_default PARTITION OF review_partitions.z_parent DEFAULT",
        "CREATE INDEX review_partition_name ON review_partitions.z_parent(name)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_partitions").await;
    db.exec_raw("INSERT INTO review_partitions.z_parent VALUES(1, 'leaf'), (20, 'default')")
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT id FROM review_partitions.a_leaf")
        .await
        .unwrap();
    assert_eq!(rows[0]["id"], 1);
    let rows = db
        .query_raw_json("SELECT id FROM review_partitions.c_default")
        .await
        .unwrap();
    assert_eq!(rows[0]["id"], 20);
    assert!(
        db.exec_raw("INSERT INTO review_partitions.z_parent VALUES(1, 'duplicate')")
            .await
            .is_err()
    );
    assert!(
        db.exec_raw("INSERT INTO review_partitions.z_parent VALUES(-1, 'check')")
            .await
            .is_err()
    );
    let indexes = db.query_raw_json("SELECT indexname FROM pg_indexes WHERE schemaname='review_partitions' AND tablename='a_leaf'").await.unwrap();
    assert_eq!(indexes.len(), 2);
    db.exec_raw("DROP SCHEMA review_partitions CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_deferred_unique_and_referenced_constraints_survive_export() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_deferred CASCADE",
        "CREATE SCHEMA review_deferred",
        "CREATE TABLE review_deferred.z_target(id INTEGER UNIQUE)",
        "CREATE TABLE review_deferred.a_reference(id INTEGER REFERENCES review_deferred.z_target(id))",
        "CREATE TABLE review_deferred.deferred(id INTEGER, CONSTRAINT deferred_unique UNIQUE(id) DEFERRABLE INITIALLY DEFERRED)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_deferred").await;
    db.transaction(|_| {
        Box::pin(async {
            Database::execute("INSERT INTO review_deferred.deferred VALUES(1), (1)").await?;
            Database::execute("DELETE FROM review_deferred.deferred").await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_deferred.a_reference VALUES(5)")
            .await
            .is_err()
    );
    assert!(
        db.exec_raw("INSERT INTO review_deferred.deferred VALUES(1), (1)")
            .await
            .is_err()
    );
    db.exec_raw("DROP SCHEMA review_deferred CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_export_preserves_quotes_and_backslashes_in_expressions() {
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("DROP TABLE IF EXISTS review_expression_escaping")
        .await
        .unwrap();
    db.exec_raw(
        r"CREATE TABLE review_expression_escaping(
        id INTEGER PRIMARY KEY, txt VARCHAR(100),
        generated_text VARCHAR(100) GENERATED ALWAYS AS (concat(txt, 'it\'s', '\\end')) STORED,
        default_text VARCHAR(100) DEFAULT (concat('it\'s', '\\end')),
        CONSTRAINT review_no_cant CHECK(txt <> 'can\'t'))",
    )
    .await
    .unwrap();
    db.exec_raw("INSERT INTO review_expression_escaping(id,txt) VALUES(1,'x')")
        .await
        .unwrap();
    let before = db
        .query_raw_json("SELECT * FROM review_expression_escaping")
        .await
        .unwrap();
    let sql = exported_schema("review_expression_escaping").await;
    db.exec_raw("DROP TABLE review_expression_escaping")
        .await
        .unwrap();
    for statement in export_statements(&sql)
        .into_iter()
        .filter(|s| s.contains("review_expression_escaping"))
    {
        db.exec_raw(statement)
            .await
            .unwrap_or_else(|e| panic!("{e}\n{statement}"));
    }
    db.exec_raw("INSERT INTO review_expression_escaping(id,txt) VALUES(1,'x')")
        .await
        .unwrap();
    let after = db
        .query_raw_json("SELECT * FROM review_expression_escaping")
        .await
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(after[0]["generated_text"], "xit's\\end");
    assert_eq!(after[0]["default_text"], "it's\\end");
    assert!(
        db.exec_raw(r"INSERT INTO review_expression_escaping(id,txt) VALUES(2,'can\'t')")
            .await
            .is_err()
    );
    db.exec_raw("DROP TABLE review_expression_escaping")
        .await
        .unwrap();
}

// Array columns need sea-query's PostgreSQL arrays, which only the `postgres` feature enables.
#[cfg(feature = "postgres")]
#[tokio::test]
async fn review_postgres_bulk_null_assignments_keep_parameter_types() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    #[tideorm::model(table = "review_typed_nulls")]
    struct Nullable {
        #[tideorm(primary_key)]
        id: i64,
        payload: Option<serde_json::Value>,
        bytes: Option<Vec<u8>>,
        ints: Option<Vec<i32>>,
        longs: Option<Vec<i64>>,
        flags: Option<Vec<bool>>,
        doubles: Option<Vec<f64>>,
        names: Option<Vec<String>>,
        documents: Option<Vec<serde_json::Value>>,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("CREATE TABLE review_typed_nulls(id BIGINT PRIMARY KEY,payload JSONB,bytes BYTEA,ints INTEGER[],longs BIGINT[],flags BOOLEAN[],doubles DOUBLE PRECISION[],names TEXT[],documents JSONB[])").await.unwrap();
    db.exec_raw("INSERT INTO review_typed_nulls VALUES(1,'{}','abc',ARRAY[1],ARRAY[2::bigint],ARRAY[true],ARRAY[1.5::float8],ARRAY['x'],ARRAY['{}'::jsonb])").await.unwrap();
    for field in [
        "payload",
        "bytes",
        "ints",
        "longs",
        "flags",
        "doubles",
        "names",
        "documents",
    ] {
        assert_eq!(
            Nullable::query()
                .where_eq("id", 1)
                .update_all()
                .set(field, None::<serde_json::Value>)
                .execute()
                .await
                .unwrap(),
            1
        );
    }
    let row = Nullable::find(1).await.unwrap().unwrap();
    assert!(
        row.payload.is_none()
            && row.bytes.is_none()
            && row.ints.is_none()
            && row.longs.is_none()
            && row.flags.is_none()
            && row.doubles.is_none()
            && row.names.is_none()
            && row.documents.is_none()
    );
    db.exec_raw("DROP TABLE review_typed_nulls").await.unwrap();
}

// Array columns need sea-query's PostgreSQL arrays, which only the `postgres` feature enables.
#[cfg(feature = "postgres")]
#[tokio::test]
async fn review_postgres_empty_array_contains_excludes_sql_null() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    #[tideorm::model(table = "review_empty_contains")]
    struct Arrays {
        #[tideorm(primary_key)]
        id: i64,
        items: Option<Vec<i32>>,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("CREATE TABLE review_empty_contains(id BIGINT PRIMARY KEY,items INTEGER[])")
        .await
        .unwrap();
    db.exec_raw(
        "INSERT INTO review_empty_contains VALUES(1,ARRAY[]::integer[]),(2,ARRAY[1]),(3,NULL)",
    )
    .await
    .unwrap();
    assert_eq!(
        Arrays::query()
            .where_array_contains("items", Vec::<serde_json::Value>::new())
            .count()
            .await
            .unwrap(),
        2
    );
    db.exec_raw("DROP TABLE review_empty_contains")
        .await
        .unwrap();
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn review_postgres_failed_batch_commit_restores_snapshots() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    #[tideorm::model(table = "review_batch_commit")]
    struct Inserted {
        #[tideorm(primary_key)]
        id: i64,
        name: String,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("DROP TABLE IF EXISTS review_batch_commit")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_batch_commit(id BIGINT PRIMARY KEY,name TEXT UNIQUE DEFERRABLE INITIALLY DEFERRED)").await.unwrap();
    let first = Inserted {
        id: 391,
        name: "same".into(),
    };
    assert_eq!(first.original_value("name").unwrap(), None);
    assert!(
        Inserted::insert_all(vec![
            first.clone(),
            Inserted {
                id: 392,
                name: "same".into()
            }
        ])
        .await
        .is_err()
    );
    assert_eq!(first.original_value("name").unwrap(), None);
    assert_eq!(Inserted::query().count().await.unwrap(), 0);
    let result = db
        .transaction(|_| {
            Box::pin(async {
                Inserted::insert_all(vec![
                    Inserted {
                        id: 393,
                        name: "one".into(),
                    },
                    Inserted {
                        id: 394,
                        name: "two".into(),
                    },
                ])
                .await?;
                Err::<(), _>(tideorm::Error::query("rollback outer transaction"))
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(
        Inserted {
            id: 393,
            name: "one".into()
        }
        .original_value("name")
        .unwrap(),
        None
    );
    let stored = Inserted::insert_all(vec![
        Inserted {
            id: 395,
            name: "one".into(),
        },
        Inserted {
            id: 396,
            name: "two".into(),
        },
    ])
    .await
    .unwrap();
    assert_eq!(
        stored[0].original_value("name").unwrap(),
        Some(Some(serde_json::json!("one")))
    );
    db.exec_raw("DROP TABLE review_batch_commit").await.unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_shared_sequences_and_ownership() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_sequences CASCADE",
        "CREATE SCHEMA review_sequences",
        "CREATE SEQUENCE review_sequences.shared AS bigint START WITH 100 INCREMENT BY 7 MINVALUE 10 MAXVALUE 10000 CACHE 3 CYCLE",
        "CREATE TABLE review_sequences.alpha(id bigint DEFAULT nextval('review_sequences.shared'))",
        "CREATE TABLE review_sequences.beta(id bigint DEFAULT nextval('review_sequences.shared'))",
        "CREATE TABLE review_sequences.serials(id bigserial PRIMARY KEY)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_sequences").await;
    // Pin both nextval calls to one connection: CACHE reserves a separate
    // block per session, so a pooled second call may legitimately skip ahead.
    let scoped = db.clone();
    let (first, second) = db
        .transaction(move |_| {
            Box::pin(async move {
                let first = scoped
                    .query_raw_json(
                        "INSERT INTO review_sequences.alpha DEFAULT VALUES RETURNING id",
                    )
                    .await?;
                let second = scoped
                    .query_raw_json("INSERT INTO review_sequences.beta DEFAULT VALUES RETURNING id")
                    .await?;
                Ok((first, second))
            })
        })
        .await
        .unwrap();
    assert_eq!(first[0]["id"], 100);
    assert_eq!(second[0]["id"], 107);
    let seq = db.query_raw_json("SELECT seqincrement, seqmin, seqmax, seqcache, seqcycle FROM pg_sequence WHERE seqrelid='review_sequences.shared'::regclass").await.unwrap();
    assert_eq!(
        seq[0],
        serde_json::json!({"seqincrement":7,"seqmin":10,"seqmax":10000,"seqcache":3,"seqcycle":true})
    );
    let owned = db
        .query_raw_json("SELECT pg_get_serial_sequence('review_sequences.serials','id') AS name")
        .await
        .unwrap();
    assert_eq!(owned[0]["name"], "review_sequences.serials_id_seq");
    assert_eq!(
        db.query_raw_json("INSERT INTO review_sequences.serials DEFAULT VALUES RETURNING id")
            .await
            .unwrap()[0]["id"],
        1
    );
    db.exec_raw("DROP SCHEMA review_sequences CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_partition_default_overrides() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "CREATE SCHEMA review_partition_defaults",
        "CREATE TABLE review_partition_defaults.events(id INTEGER,val INTEGER DEFAULT 1,removed INTEGER DEFAULT 5) PARTITION BY RANGE(id)",
        "CREATE TABLE review_partition_defaults.leaf PARTITION OF review_partition_defaults.events FOR VALUES FROM(0) TO(10)",
        "ALTER TABLE review_partition_defaults.leaf ALTER COLUMN val SET DEFAULT 9",
        "ALTER TABLE review_partition_defaults.leaf ALTER COLUMN removed DROP DEFAULT",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_partition_defaults").await;
    let direct = db
        .query_raw_json(
            "INSERT INTO review_partition_defaults.leaf(id) VALUES(1) RETURNING val,removed",
        )
        .await
        .unwrap();
    assert_eq!(direct[0], serde_json::json!({"val":9,"removed":null}));
    let routed = db
        .query_raw_json(
            "INSERT INTO review_partition_defaults.events(id) VALUES(2) RETURNING val,removed",
        )
        .await
        .unwrap();
    assert_eq!(routed[0], serde_json::json!({"val":1,"removed":5}));
    db.exec_raw("DROP SCHEMA review_partition_defaults CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_upsert_requires_a_complete_unique_target() {
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    #[tideorm::model(table = "review_unique_target")]
    struct Upsert {
        #[tideorm(primary_key)]
        id: i64,
        label: String,
        amount: i64,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw(
        "CREATE TABLE review_unique_target(id BIGINT PRIMARY KEY,label VARCHAR(100),amount BIGINT)",
    )
    .await
    .unwrap();
    db.exec_raw("INSERT INTO review_unique_target VALUES(1,'same',10),(2,'same',20)")
        .await
        .unwrap();
    assert!(
        Upsert::on_conflict(vec!["label"])
            .update_columns(vec!["amount"])
            .insert(Upsert {
                id: 3,
                label: "same".into(),
                amount: 999
            })
            .await
            .is_err()
    );
    assert_eq!(
        db.query_raw_json("SELECT amount FROM review_unique_target ORDER BY id")
            .await
            .unwrap(),
        serde_json::json!([{"amount":10},{"amount":20}])
            .as_array()
            .unwrap()
            .clone()
    );
    db.exec_raw("DELETE FROM review_unique_target WHERE id=2")
        .await
        .unwrap();
    // Even a single match does not make a non-unique target valid.
    assert!(
        Upsert::on_conflict(vec!["label"])
            .insert(Upsert {
                id: 3,
                label: "same".into(),
                amount: 999
            })
            .await
            .is_err()
    );
    db.exec_raw("CREATE UNIQUE INDEX review_prefix ON review_unique_target(label(4))")
        .await
        .unwrap();
    assert!(
        Upsert::on_conflict(vec!["label"])
            .insert(Upsert {
                id: 3,
                label: "same".into(),
                amount: 999
            })
            .await
            .is_err()
    );
    db.exec_raw("DROP INDEX review_prefix ON review_unique_target")
        .await
        .unwrap();
    db.exec_raw("CREATE UNIQUE INDEX review_full ON review_unique_target(label)")
        .await
        .unwrap();
    let row = Upsert::on_conflict(vec!["label"])
        .update_columns(vec!["amount"])
        .insert(Upsert {
            id: 3,
            label: "same".into(),
            amount: 999,
        })
        .await
        .unwrap();
    assert_eq!((row.id, row.amount), (1, 999));
    assert!(
        Upsert::on_conflict(Vec::<&str>::new())
            .insert(Upsert {
                id: 0,
                label: "empty".into(),
                amount: 1
            })
            .await
            .is_err()
    );
    db.exec_raw("DROP TABLE review_unique_target")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_export_preserves_mixed_index_directions() {
    if backend::DATABASE_TYPE != DatabaseType::MySQL || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("DROP TABLE IF EXISTS review_index_directions")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_index_directions(id BIGINT PRIMARY KEY,a INTEGER,b INTEGER,label VARCHAR(50),INDEX review_mixed(a DESC,b ASC),INDEX review_prefixed(label(10) DESC,b ASC))").await.unwrap();
    let lookup = "SELECT INDEX_NAME AS name,SEQ_IN_INDEX AS position,COLLATION AS direction FROM information_schema.statistics WHERE TABLE_SCHEMA=DATABASE() AND TABLE_NAME='review_index_directions' ORDER BY INDEX_NAME,SEQ_IN_INDEX";
    let before = db.query_raw_json(lookup).await.unwrap();
    let sql = exported_schema("review_index_directions").await;
    db.exec_raw("DROP TABLE review_index_directions")
        .await
        .unwrap();
    for statement in export_statements(&sql)
        .into_iter()
        .filter(|s| s.contains("review_index_directions"))
    {
        db.exec_raw(statement)
            .await
            .unwrap_or_else(|e| panic!("{e}\n{statement}"));
    }
    assert_eq!(db.query_raw_json(lookup).await.unwrap(), before);
    db.exec_raw("DROP TABLE review_index_directions")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_rls_and_inheritance() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_security CASCADE",
        "CREATE SCHEMA review_security",
        "CREATE TABLE review_security.z_parent(tenant integer CHECK(tenant > 0))",
        "CREATE TABLE review_security.a_child(note text) INHERITS(review_security.z_parent)",
        "ALTER TABLE review_security.a_child ENABLE ROW LEVEL SECURITY",
        "ALTER TABLE review_security.a_child FORCE ROW LEVEL SECURITY",
        "CREATE POLICY tenant_only ON review_security.a_child AS RESTRICTIVE FOR ALL TO PUBLIC USING(tenant = 1) WITH CHECK(tenant = 1)",
        "CREATE POLICY readable ON review_security.a_child FOR SELECT TO PUBLIC USING(true)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let policies = "SELECT policyname, permissive, roles, cmd, qual, with_check FROM pg_policies WHERE schemaname='review_security' ORDER BY policyname";
    let flags = "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid='review_security.a_child'::regclass";
    let before_policies = db.query_raw_json(policies).await.unwrap();
    let before_flags = db.query_raw_json(flags).await.unwrap();
    replay_review_schema(&db, "review_security").await;
    assert_eq!(before_policies, db.query_raw_json(policies).await.unwrap());
    assert_eq!(before_flags, db.query_raw_json(flags).await.unwrap());
    // Disable security for the inheritance assertion, independent of whether
    // the integration role bypasses RLS.
    db.exec_raw("ALTER TABLE review_security.a_child DISABLE ROW LEVEL SECURITY")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_security.a_child VALUES(1, 'child')")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT * FROM review_security.z_parent")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        db.exec_raw("INSERT INTO review_security.a_child VALUES(-1, 'invalid')")
            .await
            .is_err()
    );
    db.exec_raw("DROP SCHEMA review_security CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_native_table_definitions_round_trip() {
    if backend::DATABASE_TYPE != DatabaseType::MySQL || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP TABLE IF EXISTS review_native_columns",
        "DROP TABLE IF EXISTS review_native_partition",
        "CREATE TABLE review_native_columns(id INT PRIMARY KEY, counter INT AUTO_INCREMENT UNIQUE, email VARCHAR(100) COLLATE utf8mb4_bin UNIQUE, secret INT INVISIBLE, UNIQUE KEY lower_email ((LOWER(email)))) DEFAULT CHARSET=utf8mb4",
        "CREATE TABLE review_native_partition(id INT PRIMARY KEY) PARTITION BY RANGE(id) (PARTITION p0 VALUES LESS THAN(10), PARTITION p1 VALUES LESS THAN MAXVALUE)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let exported = exported_schema("review_native").await;
    for table in ["review_native_columns", "review_native_partition"] {
        let show = format!("SHOW CREATE TABLE {table}");
        let before = db.query_raw_json(&show).await.unwrap();
        db.exec_raw(&format!("DROP TABLE {table}")).await.unwrap();
        for sql in export_statements(&exported)
            .into_iter()
            .filter(|sql| sql.contains(table))
        {
            db.exec_raw(sql).await.unwrap();
        }
        assert_eq!(before, db.query_raw_json(&show).await.unwrap());
    }
    db.exec_raw("INSERT INTO review_native_columns(id, email, secret) VALUES(1, 'Alice', 7)")
        .await
        .unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_native_columns(id, email) VALUES(2, 'alice')")
            .await
            .is_err()
    );
    db.exec_raw("ALTER TABLE review_native_columns DROP INDEX lower_email")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_native_columns(id, email) VALUES(2, 'alice')")
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT * FROM review_native_columns ORDER BY id")
        .await
        .unwrap();
    assert!(rows[0].get("secret").is_none());
    assert_eq!(rows[1]["counter"], 3); // The rejected insert consumes a counter.
    db.exec_raw("ALTER TABLE review_native_partition DROP PARTITION p0")
        .await
        .unwrap();
    db.exec_raw("DROP TABLE review_native_columns, review_native_partition")
        .await
        .unwrap();
}

// Array columns need sea-query's PostgreSQL arrays, which only the `postgres` feature enables.
#[cfg(feature = "postgres")]
#[tokio::test]
async fn review_postgres_jsonb_array_primitives_use_json_parameters() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    #[tideorm::model(table = "review_jsonb_elements")]
    struct JsonArray {
        #[tideorm(primary_key)]
        id: i64,
        items: Vec<serde_json::Value>,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("DROP TABLE IF EXISTS review_jsonb_elements")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_jsonb_elements(id BIGINT PRIMARY KEY, items JSONB[])")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_jsonb_elements VALUES(1, ARRAY[]::jsonb[])")
        .await
        .unwrap();
    let values = vec![
        serde_json::json!("hello"),
        serde_json::json!(12),
        serde_json::json!(true),
        serde_json::Value::Null,
    ];
    for value in &values {
        JsonArray::query()
            .where_eq("id", 1)
            .update_all()
            .array_append("items", value.clone())
            .execute()
            .await
            .unwrap();
        assert_eq!(
            JsonArray::query()
                .where_array_contains("items", vec![value.clone()])
                .count()
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            JsonArray::query()
                .where_array_overlaps("items", vec![value.clone()])
                .count()
                .await
                .unwrap(),
            1
        );
    }
    assert_eq!(
        JsonArray::query()
            .where_array_contained_by("items", values.clone())
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(JsonArray::find(1).await.unwrap().unwrap().items, values);
    for value in values {
        JsonArray::query()
            .where_eq("id", 1)
            .update_all()
            .array_remove("items", value)
            .execute()
            .await
            .unwrap();
    }
    assert!(JsonArray::find(1).await.unwrap().unwrap().items.is_empty());
    db.exec_raw("DROP TABLE review_jsonb_elements")
        .await
        .unwrap();
}

// Array columns need sea-query's PostgreSQL arrays, which only the `postgres` feature enables.
#[cfg(feature = "postgres")]
#[tokio::test]
async fn review_postgres_array_equality_uses_native_parameters() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    #[tideorm::model(table = "review_array_equality")]
    struct Arrays {
        #[tideorm(primary_key)]
        id: i64,
        ints: Vec<i32>,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("DROP TABLE IF EXISTS review_array_equality")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_array_equality(id BIGINT PRIMARY KEY, ints INTEGER[])")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_array_equality VALUES(1, ARRAY[1,2]), (2, ARRAY[]::integer[]), (3, ARRAY[2,1])").await.unwrap();
    assert_eq!(
        Arrays::query()
            .where_eq("ints", vec![1, 2])
            .first()
            .await
            .unwrap()
            .unwrap()
            .id,
        1
    );
    assert_eq!(
        Arrays::query()
            .where_eq("ints", Vec::<i32>::new())
            .first()
            .await
            .unwrap()
            .unwrap()
            .id,
        2
    );
    assert_eq!(
        Arrays::query()
            .where_in("ints", vec![vec![1, 2], vec![2, 1]])
            .count()
            .await
            .unwrap(),
        2
    );
    db.exec_raw("DROP TABLE review_array_equality")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_trigger_functions_and_states() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_trigger_functions CASCADE",
        "CREATE SCHEMA review_trigger_functions",
        "CREATE TABLE review_trigger_functions.items(x integer)",
        "CREATE FUNCTION review_trigger_functions.guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.x < 0 THEN RAISE EXCEPTION 'negative; forbidden'; END IF; RETURN NEW; END $$",
        "CREATE TRIGGER guard BEFORE INSERT ON review_trigger_functions.items FOR EACH ROW EXECUTE FUNCTION review_trigger_functions.guard()",
        "CREATE TRIGGER disabled_guard BEFORE UPDATE ON review_trigger_functions.items FOR EACH ROW EXECUTE FUNCTION review_trigger_functions.guard()",
        "ALTER TABLE review_trigger_functions.items DISABLE TRIGGER disabled_guard",
        "CREATE TRIGGER replica_guard BEFORE DELETE ON review_trigger_functions.items FOR EACH ROW EXECUTE FUNCTION review_trigger_functions.guard()",
        "ALTER TABLE review_trigger_functions.items ENABLE REPLICA TRIGGER replica_guard",
        "ALTER TABLE review_trigger_functions.items ENABLE ALWAYS TRIGGER guard",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let metadata = "SELECT tgname,tgenabled::text AS state,pg_get_triggerdef(oid) AS definition FROM pg_trigger WHERE tgrelid='review_trigger_functions.items'::regclass ORDER BY tgname";
    let before = db.query_raw_json(metadata).await.unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_trigger_functions.items VALUES(-1)")
            .await
            .is_err()
    );
    replay_review_schema(&db, "review_trigger_functions").await;
    assert_eq!(before, db.query_raw_json(metadata).await.unwrap());
    assert!(
        db.exec_raw("INSERT INTO review_trigger_functions.items VALUES(-1)")
            .await
            .is_err()
    );
    db.exec_raw("INSERT INTO review_trigger_functions.items VALUES(1)")
        .await
        .unwrap();
    db.exec_raw("UPDATE review_trigger_functions.items SET x=-1")
        .await
        .unwrap();
    db.exec_raw("DROP SCHEMA review_trigger_functions CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_restores_function_backed_defaults() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_function_defaults CASCADE",
        "CREATE SCHEMA review_function_defaults",
        "CREATE FUNCTION review_function_defaults.z_answer() RETURNS integer LANGUAGE SQL IMMUTABLE AS 'SELECT 42'",
        "CREATE FUNCTION review_function_defaults.a_answer() RETURNS integer LANGUAGE SQL IMMUTABLE RETURN review_function_defaults.z_answer()",
        "CREATE FUNCTION review_function_defaults.z_answer(n integer) RETURNS integer LANGUAGE SQL IMMUTABLE AS 'SELECT n+1'",
        "CREATE TABLE review_function_defaults.items(id integer, x integer DEFAULT review_function_defaults.a_answer(), y integer GENERATED ALWAYS AS (review_function_defaults.z_answer(id)) STORED)",
        "CREATE FUNCTION review_function_defaults.lookup() RETURNS integer LANGUAGE SQL AS 'SELECT COALESCE(max(x),42) FROM review_function_defaults.items'",
        "ALTER TABLE review_function_defaults.items ADD COLUMN z integer DEFAULT review_function_defaults.lookup()",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_function_defaults").await;
    db.exec_raw("INSERT INTO review_function_defaults.items(id) VALUES(5)")
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT x,y,z FROM review_function_defaults.items")
        .await
        .unwrap();
    assert_eq!(rows[0]["x"], 42);
    assert_eq!(rows[0]["y"], 6);
    assert_eq!(rows[0]["z"], 42);
    db.exec_raw("DROP SCHEMA review_function_defaults CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_inherited_ownership() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_inherited_ownership CASCADE",
        "CREATE SCHEMA review_inherited_ownership",
        "CREATE TABLE review_inherited_ownership.z_parent(x integer DEFAULT 1 CONSTRAINT positive CHECK(x>0))",
        "ALTER TABLE review_inherited_ownership.z_parent ADD CONSTRAINT under100 CHECK(x<100) NOT VALID",
        "CREATE TABLE review_inherited_ownership.y_parent(other integer)",
        "CREATE TABLE review_inherited_ownership.b_child(note text) INHERITS(review_inherited_ownership.z_parent, review_inherited_ownership.y_parent)",
        "ALTER TABLE review_inherited_ownership.b_child ALTER COLUMN x SET DEFAULT 2",
        "CREATE TABLE review_inherited_ownership.a_leaf() INHERITS(review_inherited_ownership.b_child)",
        "CREATE TABLE review_inherited_ownership.local_child(x integer CONSTRAINT positive CHECK(x>0)) INHERITS(review_inherited_ownership.z_parent)",
        "ALTER TABLE review_inherited_ownership.z_parent ADD CONSTRAINT not_seven CHECK(x<>7) NOT VALID",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let columns = "SELECT c.relname,a.attname,a.attislocal,a.attinhcount,pg_get_expr(d.adbin,d.adrelid) AS default_expr FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE n.nspname='review_inherited_ownership' AND a.attnum>0 AND NOT a.attisdropped ORDER BY c.relname,a.attnum";
    let checks = "SELECT t.relname,c.conname,c.conislocal,c.coninhcount,c.convalidated FROM pg_constraint c JOIN pg_class t ON t.oid=c.conrelid JOIN pg_namespace n ON n.oid=t.relnamespace WHERE n.nspname='review_inherited_ownership' ORDER BY t.relname,c.conname";
    let before_columns = db.query_raw_json(columns).await.unwrap();
    let before_checks = db.query_raw_json(checks).await.unwrap();
    replay_review_schema(&db, "review_inherited_ownership").await;
    assert_eq!(before_columns, db.query_raw_json(columns).await.unwrap());
    assert_eq!(before_checks, db.query_raw_json(checks).await.unwrap());
    db.exec_raw("ALTER TABLE review_inherited_ownership.z_parent DROP CONSTRAINT positive")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_inherited_ownership.a_leaf(x) VALUES(-1)")
        .await
        .unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_inherited_ownership.local_child(x) VALUES(-1)")
            .await
            .is_err()
    );
    db.exec_raw("DROP SCHEMA review_inherited_ownership CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_orders_tables_types_and_routines_together() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_object_order CASCADE",
        "CREATE SCHEMA review_object_order",
        "CREATE TABLE review_object_order.settings(value integer)",
        "CREATE FUNCTION review_object_order.setting() RETURNS integer LANGUAGE SQL STABLE RETURN (SELECT COALESCE(max(value),42) FROM review_object_order.settings)",
        "CREATE FUNCTION review_object_order.positive(n integer) RETURNS boolean LANGUAGE SQL IMMUTABLE RETURN n>0",
        "CREATE DOMAIN review_object_order.positive_int AS integer DEFAULT review_object_order.setting() CHECK(review_object_order.positive(VALUE))",
        "CREATE TABLE review_object_order.items(value review_object_order.positive_int)",
        "CREATE FUNCTION review_object_order.items() RETURNS SETOF review_object_order.items LANGUAGE SQL RETURN (SELECT i FROM review_object_order.items i)",
        "CREATE FUNCTION review_object_order.echo(i review_object_order.items) RETURNS review_object_order.items LANGUAGE SQL RETURN i",
        "CREATE FUNCTION review_object_order.array_echo(i review_object_order.items[]) RETURNS review_object_order.items[] LANGUAGE SQL RETURN i",
        // A default can refer to a parsed function that reads the very same
        // table. Deferring defaults breaks this otherwise valid dependency cycle.
        "ALTER TABLE review_object_order.settings ALTER COLUMN value SET DEFAULT review_object_order.setting()",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_object_order").await;
    db.exec_raw("INSERT INTO review_object_order.items DEFAULT VALUES")
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT value FROM review_object_order.items()")
        .await
        .unwrap();
    assert_eq!(rows[0]["value"], 42);
    assert!(
        db.exec_raw("INSERT INTO review_object_order.items VALUES(-1)")
            .await
            .is_err()
    );
    let rows = db.query_raw_json("SELECT (review_object_order.echo(ROW(7)::review_object_order.items)).value AS value, cardinality(review_object_order.array_echo(ARRAY[ROW(7)::review_object_order.items])) AS size").await.unwrap();
    assert_eq!(rows[0]["value"], 7);
    assert_eq!(rows[0]["size"], 1);
    db.exec_raw("INSERT INTO review_object_order.settings DEFAULT VALUES")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT value FROM review_object_order.settings")
            .await
            .unwrap()[0]["value"],
        42
    );
    db.exec_raw("DROP SCHEMA review_object_order CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_resolves_conflicting_inherited_defaults() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_default_conflicts CASCADE",
        "CREATE SCHEMA review_default_conflicts",
        "CREATE TABLE review_default_conflicts.a(x integer DEFAULT 1)",
        "CREATE TABLE review_default_conflicts.b(x integer DEFAULT 2)",
        "CREATE TABLE review_default_conflicts.child() INHERITS(review_default_conflicts.a)",
        "ALTER TABLE review_default_conflicts.child INHERIT review_default_conflicts.b",
        "CREATE TABLE review_default_conflicts.leaf() INHERITS(review_default_conflicts.child)",
        "ALTER TABLE review_default_conflicts.leaf ALTER COLUMN x DROP DEFAULT",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let metadata = "SELECT c.relname,a.attislocal,a.attinhcount,pg_get_expr(d.adbin,d.adrelid) AS default_expr FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE n.nspname='review_default_conflicts' AND a.attnum>0 ORDER BY c.relname";
    let before = db.query_raw_json(metadata).await.unwrap();
    replay_review_schema(&db, "review_default_conflicts").await;
    assert_eq!(before, db.query_raw_json(metadata).await.unwrap());
    for (table, expected) in [
        ("a", Some(1)),
        ("b", Some(2)),
        ("child", Some(1)),
        ("leaf", None),
    ] {
        let rows = db
            .query_raw_json(&format!(
                "INSERT INTO review_default_conflicts.{table} DEFAULT VALUES RETURNING x"
            ))
            .await
            .unwrap();
        assert_eq!(rows[0]["x"], serde_json::json!(expected));
    }
    db.exec_raw("DROP SCHEMA review_default_conflicts CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_routine_execution_acl() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "CREATE ROLE \"review Routine Guest\"",
        "CREATE ROLE \"review Routine Allowed\"",
        "CREATE SCHEMA review_routine_acl",
        "CREATE TABLE review_routine_acl.secret(value text)",
        "CREATE FUNCTION review_routine_acl.reveal() RETURNS text LANGUAGE SQL SECURITY DEFINER AS 'SELECT value FROM review_routine_acl.secret'",
        "REVOKE ALL ON FUNCTION review_routine_acl.reveal() FROM PUBLIC",
        "GRANT EXECUTE ON FUNCTION review_routine_acl.reveal() TO \"review Routine Allowed\" WITH GRANT OPTION",
        "CREATE FUNCTION review_routine_acl.reveal(n integer) RETURNS integer LANGUAGE SQL RETURN n",
        "CREATE PROCEDURE review_routine_acl.run(n integer) LANGUAGE SQL AS 'SELECT 1'",
        "REVOKE ALL ON PROCEDURE review_routine_acl.run(integer) FROM PUBLIC",
        "GRANT EXECUTE ON PROCEDURE review_routine_acl.run(integer) TO \"review Routine Allowed\"",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let acl = "SELECT p.proname,pg_get_function_identity_arguments(p.oid) AS args,CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END AS role,a.is_grantable FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) a WHERE n.nspname='review_routine_acl' ORDER BY p.proname,args,role";
    let before = db.query_raw_json(acl).await.unwrap();
    // Destination defaults must not reopen a restricted function to a new role.
    db.exec_raw("ALTER DEFAULT PRIVILEGES GRANT EXECUTE ON FUNCTIONS TO \"review Routine Guest\"")
        .await
        .unwrap();
    replay_review_schema(&db, "review_routine_acl").await;
    db.exec_raw(
        "ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM \"review Routine Guest\"",
    )
    .await
    .unwrap();
    assert_eq!(before, db.query_raw_json(acl).await.unwrap());
    db.exec_raw("GRANT USAGE ON SCHEMA review_routine_acl TO PUBLIC")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_routine_acl.secret VALUES('restricted')")
        .await
        .unwrap();
    let permissions = db.query_raw_json("SELECT has_function_privilege('review Routine Guest','review_routine_acl.reveal()','EXECUTE') AS guest,has_function_privilege('review Routine Allowed','review_routine_acl.reveal()','EXECUTE WITH GRANT OPTION') AS allowed").await.unwrap();
    assert_eq!(permissions[0]["guest"], false);
    assert_eq!(permissions[0]["allowed"], true);
    let denied = db
        .transaction(|_| {
            Box::pin(async {
                Database::execute("SET LOCAL ROLE \"review Routine Guest\"").await?;
                Database::raw_json("SELECT review_routine_acl.reveal()").await
            })
        })
        .await;
    assert!(
        denied
            .unwrap_err()
            .to_string()
            .contains("permission denied")
    );
    db.transaction(|_| {
        Box::pin(async {
            Database::execute("SET LOCAL ROLE \"review Routine Allowed\"").await?;
            assert_eq!(
                Database::raw_json("SELECT review_routine_acl.reveal() AS value").await?[0]
                    ["value"],
                "restricted"
            );
            Ok(())
        })
    })
    .await
    .unwrap();
    db.exec_raw("DROP SCHEMA review_routine_acl CASCADE")
        .await
        .unwrap();
    db.exec_raw("DROP ROLE \"review Routine Guest\", \"review Routine Allowed\"")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_domain_and_composite_collations_round_trip() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "CREATE SCHEMA review_type_collations",
        "CREATE COLLATION review_type_collations.fold(provider=icu,locale='und-u-ks-level2',deterministic=false)",
        "CREATE DOMAIN review_type_collations.folded AS text COLLATE review_type_collations.fold",
        "CREATE TYPE review_type_collations.label AS (name text COLLATE review_type_collations.fold)",
        "CREATE TABLE review_type_collations.domains(name review_type_collations.folded UNIQUE)",
        "CREATE TABLE review_type_collations.composites(label review_type_collations.label UNIQUE)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let metadata = "SELECT t.typname, co.collname FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace LEFT JOIN pg_collation co ON co.oid=t.typcollation WHERE n.nspname='review_type_collations' ORDER BY t.typname";
    let before = db.query_raw_json(metadata).await.unwrap();
    for restore in [false, true] {
        if restore {
            replay_review_schema(&db, "review_type_collations").await;
        }
        assert_eq!(before, db.query_raw_json(metadata).await.unwrap());
        db.exec_raw("INSERT INTO review_type_collations.domains VALUES('Alice')")
            .await
            .unwrap();
        assert!(
            db.exec_raw("INSERT INTO review_type_collations.domains VALUES('alice')")
                .await
                .is_err()
        );
        db.exec_raw("INSERT INTO review_type_collations.composites VALUES(ROW('Alice'))")
            .await
            .unwrap();
        assert!(
            db.exec_raw("INSERT INTO review_type_collations.composites VALUES(ROW('alice'))")
                .await
                .is_err()
        );
    }
    db.exec_raw("DROP SCHEMA review_type_collations CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_table_owner_keeps_definer_access() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let role = format!("review_table_owner_{}", std::process::id());
    db.exec_raw(&format!("CREATE ROLE {role}")).await.unwrap();
    for sql in [
        format!("CREATE SCHEMA review_table_owner AUTHORIZATION {role}"),
        "CREATE TABLE review_table_owner.secret(id serial,value integer)".into(),
        format!("ALTER TABLE review_table_owner.secret OWNER TO {role}"),
        "CREATE FUNCTION review_table_owner.read_secret() RETURNS integer LANGUAGE SQL SECURITY DEFINER RETURN (SELECT value FROM review_table_owner.secret LIMIT 1)".into(),
        format!("ALTER FUNCTION review_table_owner.read_secret() OWNER TO {role}"),
    ] {
        db.exec_raw(&sql).await.unwrap();
    }
    for restore in [false, true] {
        if restore {
            replay_review_schema(&db, "review_table_owner").await;
        }
        db.exec_raw("INSERT INTO review_table_owner.secret(value) VALUES(42)")
            .await
            .unwrap();
        assert_eq!(
            db.query_raw_json("SELECT review_table_owner.read_secret() AS value")
                .await
                .unwrap()[0]["value"],
            42
        );
        assert_eq!(
            db.query_raw_json(
                "SELECT pg_get_userbyid(relowner) AS owner FROM pg_class \
                 WHERE oid='review_table_owner.secret'::regclass"
            )
            .await
            .unwrap()[0]["owner"],
            role
        );
    }
    db.exec_raw("DROP SCHEMA review_table_owner CASCADE")
        .await
        .unwrap();
    db.exec_raw(&format!("DROP ROLE {role}")).await.unwrap();
}

#[tokio::test]
async fn review_postgres_view_dependent_functions_restore_in_order() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "CREATE SCHEMA review_views",
        "CREATE TABLE review_views.items(value integer)",
        "CREATE FUNCTION review_views.increment(x integer) RETURNS integer LANGUAGE SQL IMMUTABLE RETURN x+1",
        "CREATE VIEW review_views.z_base WITH (security_barrier=true) AS SELECT review_views.increment(value) AS value FROM review_views.items",
        "CREATE VIEW review_views.a_visible AS SELECT * FROM review_views.z_base",
        "CREATE FUNCTION review_views.read_visible() RETURNS SETOF review_views.a_visible LANGUAGE SQL RETURN (SELECT v FROM review_views.a_visible v)",
        "CREATE MATERIALIZED VIEW review_views.snapshot AS SELECT * FROM review_views.a_visible WITH NO DATA",
        "CREATE FUNCTION review_views.snapshot_key(row_value review_views.snapshot) RETURNS integer LANGUAGE SQL IMMUTABLE RETURN row_value.value",
        "CREATE UNIQUE INDEX snapshot_value ON review_views.snapshot(review_views.snapshot_key(ROW(value)::review_views.snapshot))",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    replay_review_schema(&db, "review_views").await;
    db.exec_raw("INSERT INTO review_views.items VALUES(6)")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT value FROM review_views.read_visible()")
            .await
            .unwrap()[0]["value"],
        7
    );
    db.exec_raw("REFRESH MATERIALIZED VIEW review_views.snapshot")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT value FROM review_views.snapshot")
            .await
            .unwrap()[0]["value"],
        7
    );
    assert_eq!(
        db.query_raw_json(
            "SELECT reloptions::text AS options FROM pg_class \
             WHERE oid='review_views.z_base'::regclass"
        )
        .await
        .unwrap()[0]["options"],
        "{security_barrier=true}"
    );
    db.exec_raw("DROP SCHEMA review_views CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_row_type_check_restores_after_its_function() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "CREATE SCHEMA review_row_check",
        "CREATE TABLE review_row_check.items(value integer)",
        "CREATE FUNCTION review_row_check.valid(row_value review_row_check.items) RETURNS boolean LANGUAGE SQL IMMUTABLE RETURN row_value.value > 0",
        "ALTER TABLE review_row_check.items ADD CONSTRAINT positive CHECK(review_row_check.valid(ROW(value)::review_row_check.items))",
        "CREATE TABLE review_row_check.child() INHERITS(review_row_check.items)",
        "CREATE TABLE review_row_check.local_child(value integer CONSTRAINT positive CHECK(review_row_check.valid(ROW(value)::review_row_check.items))) INHERITS(review_row_check.items)",
        "ALTER TABLE review_row_check.items ADD CONSTRAINT under_ten CHECK(review_row_check.valid(ROW(10-value)::review_row_check.items)) NOT VALID",
        "ALTER TABLE review_row_check.child VALIDATE CONSTRAINT under_ten",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let metadata = "SELECT t.relname,c.conname,c.conislocal,c.coninhcount,c.convalidated FROM pg_constraint c JOIN pg_class t ON t.oid=c.conrelid JOIN pg_namespace n ON n.oid=t.relnamespace WHERE n.nspname='review_row_check' ORDER BY t.relname,c.conname";
    let before = db.query_raw_json(metadata).await.unwrap();
    replay_review_schema(&db, "review_row_check").await;
    assert_eq!(before, db.query_raw_json(metadata).await.unwrap());
    for table in ["items", "child", "local_child"] {
        assert!(
            db.exec_raw(&format!("INSERT INTO review_row_check.{table} VALUES(-1)"))
                .await
                .is_err()
        );
        db.exec_raw(&format!("INSERT INTO review_row_check.{table} VALUES(1)"))
            .await
            .unwrap();
    }
    db.exec_raw("ALTER TABLE review_row_check.items DROP CONSTRAINT positive")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_row_check.child VALUES(-1)")
        .await
        .unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_row_check.local_child VALUES(-1)")
            .await
            .is_err()
    );
    db.exec_raw("DROP SCHEMA review_row_check CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_array_filters_never_match_scalar_documents() {
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    #[tideorm::model(table = "review_array_shape")]
    struct Document {
        #[tideorm(primary_key)]
        id: i64,
        payload: serde_json::Value,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("CREATE TABLE review_array_shape(id BIGINT PRIMARY KEY,payload JSON)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_array_shape VALUES(1,'[1]'),(2,'1'),(3,'{\"a\":1}'),(4,'null'),(5,'[]')").await.unwrap();
    assert_eq!(
        Document::query()
            .where_array_overlaps("payload", vec![1])
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Document::query()
            .where_array_contained_by("payload", vec![1])
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Document::query()
            .where_array_contains("payload", Vec::<i32>::new())
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(Document::query().count().await.unwrap(), 4);
    db.exec_raw("DROP TABLE review_array_shape").await.unwrap();
}

#[tokio::test]
async fn review_postgres_restores_domain_partition_rule_and_replication_behavior() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "CREATE SCHEMA review_fidelity",
        "CREATE DOMAIN review_fidelity.positive AS integer DEFAULT 7 CHECK(VALUE>0)",
        "ALTER DOMAIN review_fidelity.positive ADD CONSTRAINT under_ten CHECK(VALUE<10) NOT VALID",
        "CREATE TABLE review_fidelity.items(value review_fidelity.positive)",
        "CREATE FUNCTION review_fidelity.valid_value(v integer) RETURNS boolean LANGUAGE SQL STABLE RETURN v >= COALESCE((SELECT min(value) FROM review_fidelity.items),0)",
        "ALTER DOMAIN review_fidelity.positive ADD CONSTRAINT dynamic_minimum CHECK(review_fidelity.valid_value(VALUE))",
        "CREATE TABLE review_fidelity.log(value integer)",
        "CREATE RULE on_insert AS ON INSERT TO review_fidelity.items DO ALSO INSERT INTO review_fidelity.log VALUES(NEW.value)",
        "CREATE VIEW review_fidelity.visible AS SELECT value FROM review_fidelity.items",
        "ALTER VIEW review_fidelity.visible ALTER COLUMN value SET DEFAULT 8",
        "CREATE TABLE review_fidelity.parent(id integer,value integer,computed integer GENERATED ALWAYS AS(id*2) STORED) PARTITION BY RANGE(id)",
        "CREATE TABLE review_fidelity.child PARTITION OF review_fidelity.parent (computed WITH OPTIONS GENERATED ALWAYS AS(id*3) STORED) FOR VALUES FROM(0) TO(10)",
        "ALTER TABLE review_fidelity.child ALTER COLUMN value SET NOT NULL",
        "CREATE UNLOGGED TABLE review_fidelity.scratch(id integer) WITH(fillfactor=50,autovacuum_enabled=false)",
        "ALTER TABLE review_fidelity.items REPLICA IDENTITY FULL",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    // The rule evaluates the domain again after the row exists. Validate the
    // source fixture before testing the same behavior after schema replay.
    db.exec_raw("INSERT INTO review_fidelity.items DEFAULT VALUES; INSERT INTO review_fidelity.visible DEFAULT VALUES; TRUNCATE review_fidelity.items,review_fidelity.log")
        .await
        .unwrap();
    let metadata = "SELECT c.relname,c.relpersistence::text,c.relreplident::text,c.reloptions::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='review_fidelity' AND c.relkind IN ('r','p') ORDER BY c.relname";
    let before = db.query_raw_json(metadata).await.unwrap();
    replay_review_schema(&db, "review_fidelity").await;
    assert_eq!(before, db.query_raw_json(metadata).await.unwrap());
    assert!(
        db.exec_raw("INSERT INTO review_fidelity.items VALUES(-1)")
            .await
            .is_err()
    );
    assert!(
        db.exec_raw("INSERT INTO review_fidelity.items VALUES(10)")
            .await
            .is_err()
    );
    assert!(
        db.exec_raw("INSERT INTO review_fidelity.parent(id,value) VALUES(1,NULL)")
            .await
            .is_err()
    );
    db.exec_raw("INSERT INTO review_fidelity.parent(id,value) VALUES(2,1)")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT computed FROM review_fidelity.parent")
            .await
            .unwrap()[0]["computed"],
        6
    );
    db.exec_raw("INSERT INTO review_fidelity.items DEFAULT VALUES")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_fidelity.visible DEFAULT VALUES")
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT value FROM review_fidelity.log ORDER BY value")
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![
            serde_json::json!({"value":7}),
            serde_json::json!({"value":8})
        ]
    );
    assert_eq!(
        db.query_raw_json(
            "SELECT convalidated FROM pg_constraint \
             WHERE contypid='review_fidelity.positive'::regtype AND conname='under_ten'"
        )
        .await
        .unwrap()[0]["convalidated"],
        false
    );
    db.exec_raw("DROP SCHEMA review_fidelity CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_grant_chains_and_default_privileges_survive_export() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let grantor = format!("review_grantor_{}", std::process::id());
    let reader = format!("review_reader_{}", std::process::id());
    for sql in [
        format!("CREATE ROLE {grantor}"),
        format!("CREATE ROLE {reader}"),
        "CREATE SCHEMA review_acl_chain".to_owned(),
        "CREATE TABLE review_acl_chain.items(id integer,secret text)".to_owned(),
        format!("GRANT USAGE ON SCHEMA review_acl_chain TO {grantor},{reader}"),
        format!("GRANT SELECT ON review_acl_chain.items TO {grantor} WITH GRANT OPTION"),
        format!(
            "SET ROLE {grantor}; GRANT SELECT ON review_acl_chain.items TO {reader}; RESET ROLE"
        ),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA review_acl_chain GRANT SELECT ON TABLES TO {reader}"
        ),
    ] {
        db.exec_raw(&sql).await.unwrap();
    }
    db.exec_raw(&format!(
        "REVOKE USAGE ON SCHEMA review_acl_chain FROM {grantor}"
    ))
    .await
    .unwrap();
    replay_review_schema(&db, "review_acl_chain").await;
    assert_eq!(
        db.query_raw_json(&format!(
            "SELECT has_schema_privilege('{grantor}','review_acl_chain','USAGE') AS allowed"
        ))
        .await
        .unwrap()[0]["allowed"],
        false
    );
    db.exec_raw("CREATE TABLE review_acl_chain.future(id integer)")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json(&format!(
            "SELECT has_table_privilege('{reader}','review_acl_chain.future','SELECT') AS allowed"
        ))
        .await
        .unwrap()[0]["allowed"],
        true
    );
    db.exec_raw(&format!(
        "REVOKE SELECT ON review_acl_chain.items FROM {grantor} CASCADE"
    ))
    .await
    .unwrap();
    assert_eq!(
        db.query_raw_json(&format!(
            "SELECT has_table_privilege('{reader}','review_acl_chain.items','SELECT') AS allowed"
        ))
        .await
        .unwrap()[0]["allowed"],
        false
    );
    db.exec_raw("DROP SCHEMA review_acl_chain CASCADE")
        .await
        .unwrap();
    db.exec_raw(&format!("DROP ROLE {grantor}; DROP ROLE {reader}"))
        .await
        .unwrap();
}

#[tokio::test]
async fn review_mysql_complete_export_is_portable_and_preserves_stored_objects() {
    use futures_util::FutureExt;
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    let admin = tideorm::require_db().unwrap();
    let source_name = format!("review_source_{}", uuid::Uuid::new_v4().simple());
    let destination_name = format!("review_destination_{}", uuid::Uuid::new_v4().simple());
    for name in [&source_name, &destination_name] {
        admin
            .exec_raw(&format!("CREATE DATABASE `{name}`"))
            .await
            .unwrap();
    }
    let result = std::panic::AssertUnwindSafe(async {
        let mut url = url::Url::parse(backend::database_url()).unwrap();
        url.set_path(&source_name);
        let source = Database::builder()
            .url(url.as_str())
            .max_connections(1)
            .build()
            .await
            .unwrap();
        url.set_path(&destination_name);
        let destination = Database::builder()
            .url(url.as_str())
            .max_connections(1)
            .build()
            .await
            .unwrap();
        let fold_names = source
            .query_raw_json("SELECT @@lower_case_table_names AS fold_names")
            .await
            .unwrap()[0]["fold_names"]
            .as_i64()
            .unwrap()
            != 0;
        let referenced_source = if fold_names {
            source_name.to_ascii_uppercase()
        } else {
            source_name.clone()
        };
        for sql in [
            "ALTER DATABASE COLLATE utf8mb4_bin".to_string(),
            "CREATE FUNCTION equal_text(a VARCHAR(10),b VARCHAR(10)) RETURNS BOOLEAN DETERMINISTIC RETURN a=b".to_string(),
            "CREATE TABLE a(id INTEGER)".to_string(),
            "CREATE TABLE b(id INTEGER)".to_string(),
            "INSERT INTO a VALUES(1)".to_string(),
            "INSERT INTO b VALUES(2)".to_string(),
            format!("CREATE PROCEDURE wipe() DELETE x,y FROM `{referenced_source}`.a x, `{referenced_source}`.b y"),
            format!("CREATE PROCEDURE wipe_comment() BEGIN --\tdon't parse this apostrophe\nDELETE FROM `{referenced_source}`.a; END"),
            format!("CREATE PROCEDURE wipe_target() DELETE `{referenced_source}`.a FROM `{referenced_source}`.a"),
            format!("CREATE PROCEDURE insert_short() INSERT LOW_PRIORITY /* table */ `{referenced_source}`.a VALUES(31)"),
            format!("CREATE PROCEDURE replace_short() REPLACE LOW_PRIORITY `{referenced_source}`.b VALUES(32)"),
            format!("CREATE PROCEDURE insert_into() INSERT IGNORE INTO `{referenced_source}`.a VALUES(33)"),
            format!("CREATE PROCEDURE replace_into() REPLACE INTO `{referenced_source}`.b VALUES(34)"),
            "CREATE FUNCTION incremented(n INTEGER) RETURNS INTEGER DETERMINISTIC RETURN n+1".to_string(),
            "CREATE VIEW z_base AS SELECT incremented(id) AS id FROM a".to_string(),
            "CREATE VIEW a_visible AS SELECT id FROM z_base".to_string(),
            "CREATE TRIGGER positive BEFORE INSERT ON a FOR EACH ROW BEGIN IF NEW.id<0 THEN SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='positive id required'; END IF; END".to_string(),
            "SET SESSION time_zone='+05:00'".to_string(),
            "CREATE EVENT reminder ON SCHEDULE AT '2035-01-01 12:00:00' DISABLE DO SELECT 1".to_string(),
            "SET SESSION sql_mode='NO_BACKSLASH_ESCAPES'".to_string(),
            r"CREATE TABLE notes(id INTEGER,note VARCHAR(30) DEFAULT 'end\')".to_string(),
        ] {
            source.exec_raw(&sql).await.unwrap();
        }
        Database::set_global(source.clone()).unwrap();
        let script = exported_schema(&source_name).await;
        destination
            .exec_raw("SET SESSION time_zone='+00:00'; SET SESSION sql_mode='NO_BACKSLASH_ESCAPES'")
            .await
            .unwrap();
        for statement in export_statements(&script) {
            destination
                .exec_raw(statement)
                .await
                .unwrap_or_else(|e| panic!("{e}\n{statement}"));
        }

        async fn count(db: &Database, sql: &str) -> serde_json::Value {
            db.query_raw_json(sql).await.unwrap()[0]["n"].clone()
        }
        const ROWS_IN_A: &str = "SELECT count(*) AS n FROM a";
        const ROWS_IN_BOTH: &str = "SELECT (SELECT count(*) FROM a)+(SELECT count(*) FROM b) AS n";
        assert_eq!(
            destination
                .query_raw_json("SELECT @@SESSION.time_zone AS zone,@@SESSION.sql_mode AS mode")
                .await
                .unwrap()[0],
            serde_json::json!({"zone": "+00:00", "mode": "NO_BACKSLASH_ESCAPES"})
        );
        assert_eq!(
            destination
                .query_raw_json("SELECT CAST(equal_text('a','A') AS SIGNED) AS equal_values")
                .await
                .unwrap()[0]["equal_values"],
            0
        );
        assert!(destination.exec_raw("INSERT INTO a VALUES(-1)").await.is_err());
        destination
            .exec_raw("INSERT INTO a VALUES(11); INSERT INTO b VALUES(22)")
            .await
            .unwrap();
        assert_eq!(
            destination
                .query_raw_json("SELECT id FROM a_visible")
                .await
                .unwrap()[0]["id"],
            12
        );
        for routine in ["wipe_comment", "wipe_target"] {
            destination
                .exec_raw(&format!("CALL {routine}()"))
                .await
                .unwrap();
            assert_eq!(count(&source, ROWS_IN_A).await, 1, "{routine}");
            assert_eq!(count(&destination, ROWS_IN_A).await, 0, "{routine}");
            destination.exec_raw("INSERT INTO a VALUES(11)").await.unwrap();
        }
        destination.exec_raw("CALL wipe()").await.unwrap();
        assert_eq!(count(&source, ROWS_IN_BOTH).await, 2);
        assert_eq!(count(&destination, ROWS_IN_BOTH).await, 0);
        for routine in ["insert_short", "replace_short", "insert_into", "replace_into"] {
            destination
                .exec_raw(&format!("CALL {routine}()"))
                .await
                .unwrap();
        }
        assert_eq!(count(&source, ROWS_IN_BOTH).await, 2);
        assert_eq!(count(&destination, ROWS_IN_BOTH).await, 4);
        let event = "SELECT time_zone AS zone,CAST(execute_at AS CHAR) AS at_time \
                     FROM information_schema.events \
                     WHERE event_schema=DATABASE() AND event_name='reminder'";
        assert_eq!(
            source.query_raw_json(event).await.unwrap(),
            destination.query_raw_json(event).await.unwrap()
        );
        let notes = "SELECT HEX(note) AS bytes FROM notes";
        source.exec_raw("INSERT INTO notes(id) VALUES(1)").await.unwrap();
        destination
            .exec_raw("INSERT INTO notes(id) VALUES(1)")
            .await
            .unwrap();
        assert_eq!(
            source.query_raw_json(notes).await.unwrap(),
            destination.query_raw_json(notes).await.unwrap()
        );
    })
    .catch_unwind()
    .await;
    Database::set_global(admin.clone()).unwrap();
    for name in [&source_name, &destination_name] {
        admin
            .exec_raw(&format!("DROP DATABASE `{name}`"))
            .await
            .unwrap();
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
async fn review_postgres_raw_select_mutations_flush_cached_queries() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    #[tideorm::model(table = "review_raw_mutation")]
    struct Number {
        #[tideorm(primary_key)]
        id: i64,
        amount: i64,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("CREATE TABLE review_raw_mutation(id BIGINT PRIMARY KEY,amount BIGINT); INSERT INTO review_raw_mutation VALUES(1,1)").await.unwrap();
    db.exec_raw("CREATE FUNCTION review_raw_mutate() RETURNS BIGINT LANGUAGE SQL VOLATILE AS 'UPDATE review_raw_mutation SET amount=amount+1 RETURNING amount'").await.unwrap();
    db.exec_raw("CREATE VIEW review_raw_view AS SELECT review_raw_mutate() AS amount")
        .await
        .unwrap();
    let cache = tideorm::QueryCache::global();
    cache.enable();
    cache.clear();
    for (sql, expected) in [
        ("SELECT review_raw_mutate()", 2),
        ("SELECT * FROM review_raw_view", 3),
    ] {
        let query = Number::query()
            .where_eq("id", 1)
            .cache(Duration::from_secs(60));
        assert_eq!(
            query.clone().first().await.unwrap().unwrap().amount,
            expected - 1
        );
        db.query_raw_json(sql).await.unwrap();
        assert_eq!(query.first().await.unwrap().unwrap().amount, expected);
    }
    cache.disable();
    cache.clear();
    db.exec_raw("DROP VIEW review_raw_view; DROP FUNCTION review_raw_mutate(); DROP TABLE review_raw_mutation").await.unwrap();
}

#[tokio::test]
async fn review_postgres_export_preserves_namespace_context_and_collation_owner() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = Database::builder()
        .url(backend::database_url())
        .max_connections(1)
        .build()
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    let role = format!("review_collation_{}", uuid::Uuid::new_v4().simple());
    for sql in [
        "DROP SCHEMA IF EXISTS pgapp_review CASCADE".to_string(),
        format!("CREATE ROLE {role}"),
        "CREATE SCHEMA pgapp_review".to_string(),
        "CREATE TYPE pgapp_review.status AS ENUM ('ready')".to_string(),
        "CREATE COLLATION pgapp_review.ordered (provider=libc, locale='C')".to_string(),
        format!("ALTER COLLATION pgapp_review.ordered OWNER TO {role}"),
        "CREATE TABLE pgapp_review.items (state pgapp_review.status DEFAULT 'ready', label text COLLATE pgapp_review.ordered)".to_string(),
        "SET search_path=pgapp_review,public".to_string(),
    ] {
        db.exec_raw(&sql).await.unwrap();
    }
    let script = exported_schema(&role).await;
    assert_eq!(
        db.query_raw_json("SELECT current_setting('search_path') AS path")
            .await
            .unwrap()[0]["path"],
        "pgapp_review, public"
    );
    db.exec_raw("SET search_path=public; DROP SCHEMA pgapp_review CASCADE")
        .await
        .unwrap();
    for statement in export_statements(&script)
        .into_iter()
        .filter(|s| s.contains("pgapp_review"))
    {
        db.exec_raw(statement)
            .await
            .unwrap_or_else(|e| panic!("{e}\n{statement}"));
    }
    db.exec_raw("INSERT INTO pgapp_review.items(label) VALUES('ok')")
        .await
        .unwrap();
    assert_eq!(
        db.query_raw_json("SELECT state::text AS state FROM pgapp_review.items")
            .await
            .unwrap()[0]["state"],
        "ready"
    );
    assert_eq!(
        db.query_raw_json(
            "SELECT pg_get_userbyid(collowner) AS owner FROM pg_collation \
             WHERE oid='pgapp_review.ordered'::regcollation"
        )
        .await
        .unwrap()[0]["owner"],
        role
    );
    db.exec_raw(&format!(
        "DROP SCHEMA pgapp_review CASCADE; DROP ROLE {role}"
    ))
    .await
    .unwrap();
}

#[tokio::test]
async fn review_postgres18_export_preserves_not_null_no_inherit() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let version = db
        .query_raw_json("SELECT current_setting('server_version_num')::integer AS version")
        .await
        .unwrap()[0]["version"]
        .as_i64()
        .unwrap();
    if version < 180000 {
        println!("requires PostgreSQL 18 NOT NULL constraints");
        return;
    }
    db.exec_raw("DROP SCHEMA IF EXISTS review_noinherit CASCADE; CREATE SCHEMA review_noinherit; CREATE TABLE review_noinherit.parent(value integer CONSTRAINT required_value NOT NULL NO INHERIT); CREATE TABLE review_noinherit.child() INHERITS(review_noinherit.parent)").await.unwrap();
    let script = exported_schema("review_noinherit").await;
    db.exec_raw("DROP SCHEMA review_noinherit CASCADE")
        .await
        .unwrap();
    for statement in export_statements(&script)
        .into_iter()
        .filter(|s| s.contains("review_noinherit"))
    {
        db.exec_raw(statement)
            .await
            .unwrap_or_else(|e| panic!("{e}\n{statement}"));
    }
    db.exec_raw("INSERT INTO review_noinherit.child VALUES(NULL)")
        .await
        .unwrap();
    assert!(
        db.exec_raw("INSERT INTO review_noinherit.parent VALUES(NULL)")
            .await
            .is_err()
    );
    db.exec_raw("DROP SCHEMA review_noinherit CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_replica_identity_finds_renamed_partition_index() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_replica_partition CASCADE",
        "CREATE SCHEMA review_replica_partition",
        "CREATE TABLE review_replica_partition.parent(id integer NOT NULL) PARTITION BY RANGE(id)",
        "CREATE TABLE review_replica_partition.middle PARTITION OF review_replica_partition.parent FOR VALUES FROM(0) TO(100) PARTITION BY RANGE(id)",
        "CREATE TABLE review_replica_partition.child PARTITION OF review_replica_partition.middle FOR VALUES FROM(0) TO(50)",
        "CREATE TABLE review_replica_partition.other_child PARTITION OF review_replica_partition.middle FOR VALUES FROM(50) TO(100)",
        "CREATE UNIQUE INDEX identity_root ON review_replica_partition.parent(id)",
        "ALTER TABLE review_replica_partition.parent ADD CONSTRAINT primary_root PRIMARY KEY(id)",
        "ALTER INDEX review_replica_partition.other_child_pkey RENAME TO special_primary",
        "ALTER TABLE review_replica_partition.other_child REPLICA IDENTITY USING INDEX special_primary",
        "ALTER INDEX review_replica_partition.child_id_idx RENAME TO special_replica",
        "ALTER TABLE review_replica_partition.child REPLICA IDENTITY USING INDEX special_replica",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let script = exported_schema("review_replica_partition").await;
    db.exec_raw("DROP SCHEMA review_replica_partition CASCADE")
        .await
        .unwrap();
    for statement in export_statements(&script)
        .into_iter()
        .filter(|s| s.contains("review_replica_partition"))
    {
        db.exec_raw(statement)
            .await
            .unwrap_or_else(|e| panic!("{e}\n{statement}"));
    }
    let rows = db.query_raw_json("SELECT i.indisunique AS unique_index, i.indisprimary AS primary_index, c.relreplident::text AS identity FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid WHERE i.indrelid='review_replica_partition.child'::regclass AND i.indisreplident").await.unwrap();
    assert_eq!(
        rows,
        vec![serde_json::json!({"unique_index":true,"primary_index":false,"identity":"i"})]
    );
    assert_eq!(
        db.query_raw_json(
            "SELECT indisprimary AS primary_index FROM pg_index \
             WHERE indrelid='review_replica_partition.other_child'::regclass AND indisreplident"
        )
        .await
        .unwrap(),
        vec![serde_json::json!({"primary_index":true})]
    );
    db.exec_raw("DROP SCHEMA review_replica_partition CASCADE")
        .await
        .unwrap();
}

#[tokio::test]
async fn review_postgres_domain_default_qualifies_its_sequence() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = Database::builder()
        .url(backend::database_url())
        .max_connections(1)
        .build()
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    for sql in [
        "DROP SCHEMA IF EXISTS review_domain_default CASCADE",
        "DROP SEQUENCE IF EXISTS public.review_domain_sequence CASCADE",
        "CREATE SCHEMA review_domain_default",
        "CREATE SEQUENCE public.review_domain_sequence START 900",
        "SET search_path=review_domain_default,public",
        "CREATE SEQUENCE review_domain_sequence START 10",
        "CREATE DOMAIN d AS integer DEFAULT nextval('review_domain_sequence')",
        "CREATE TABLE items(n d)",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    assert_eq!(
        db.query_raw_json(
            "INSERT INTO review_domain_default.items DEFAULT VALUES RETURNING n::integer AS n"
        )
        .await
        .unwrap()[0]["n"],
        10
    );
    let script = exported_schema("review_domain_default").await;
    db.exec_raw("SET search_path=public; DROP SCHEMA review_domain_default CASCADE; DROP SEQUENCE public.review_domain_sequence").await.unwrap();
    for statement in export_statements(&script)
        .into_iter()
        .filter(|s| s.contains("review_domain"))
    {
        db.exec_raw(statement)
            .await
            .unwrap_or_else(|e| panic!("{e}\n{statement}"));
    }
    assert_eq!(
        db.query_raw_json(
            "INSERT INTO review_domain_default.items DEFAULT VALUES RETURNING n::integer AS n"
        )
        .await
        .unwrap()[0]["n"],
        10
    );
    assert_eq!(
        db.query_raw_json("SELECT nextval('public.review_domain_sequence') AS n")
            .await
            .unwrap()[0]["n"],
        900
    );
    db.exec_raw(
        "DROP SCHEMA review_domain_default CASCADE; DROP SEQUENCE public.review_domain_sequence",
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn review_postgres_unsupported_dependencies_fail_before_writing_export() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    for (fixture, object) in [
        (
            "CREATE FOREIGN DATA WRAPPER review_fdw; CREATE SERVER review_foreign FOREIGN DATA WRAPPER review_fdw",
            "foreign server",
        ),
        (
            "CREATE FOREIGN DATA WRAPPER review_fdw; CREATE SERVER review_foreign FOREIGN DATA WRAPPER review_fdw; CREATE FOREIGN TABLE review_unsupported.remote_items(id integer) SERVER review_foreign",
            "remote_items",
        ),
        (
            "CREATE AGGREGATE review_unsupported.total(integer) (SFUNC=int4pl,STYPE=integer,INITCOND='0'); CREATE TABLE review_unsupported.items(n integer); CREATE VIEW review_unsupported.v AS SELECT review_unsupported.total(n) FROM review_unsupported.items",
            "total",
        ),
        (
            "CREATE FUNCTION review_unsupported.same(integer,integer) RETURNS boolean LANGUAGE sql IMMUTABLE AS 'SELECT $1=$2'; CREATE OPERATOR review_unsupported.=== (LEFTARG=integer,RIGHTARG=integer,FUNCTION=review_unsupported.same); CREATE TABLE review_unsupported.t(n integer CHECK(n OPERATOR(review_unsupported.===) 1))",
            "===",
        ),
        (
            "CREATE TEXT SEARCH CONFIGURATION review_unsupported.english (COPY=pg_catalog.english); CREATE TABLE review_unsupported.t(body text); CREATE INDEX review_ts ON review_unsupported.t USING gin(to_tsvector('review_unsupported.english',body))",
            "english",
        ),
    ] {
        db.exec_raw(
            "DROP SCHEMA IF EXISTS review_unsupported CASCADE; CREATE SCHEMA review_unsupported",
        )
        .await
        .unwrap();
        db.exec_raw(fixture).await.unwrap();
        let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("review_unsupported.sql");
        std::fs::write(&path, "existing export").unwrap();
        let error = SchemaWriter::write_schema(&path)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("does not support") && error.contains(object),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing export");
        std::fs::remove_file(path).unwrap();
        db.exec_raw("DROP SCHEMA review_unsupported CASCADE; DROP SERVER IF EXISTS review_foreign CASCADE; DROP FOREIGN DATA WRAPPER IF EXISTS review_fdw")
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn review_mysql_upsert_refuses_natural_and_composite_primary_key_changes() {
    if !matches!(
        backend::DATABASE_TYPE,
        DatabaseType::MySQL | DatabaseType::MariaDB
    ) || !backend::connect().await
    {
        return;
    }
    #[tideorm::model(table = "review_upsert_key")]
    struct Natural {
        #[tideorm(primary_key)]
        id: String,
        email: String,
        name: String,
    }
    #[tideorm::model(table = "review_upsert_composite")]
    struct Composite {
        #[tideorm(primary_key)]
        tenant: i64,
        #[tideorm(primary_key)]
        id: String,
        email: String,
        name: String,
    }
    let db = tideorm::require_db().unwrap();
    db.exec_raw("CREATE TABLE review_upsert_key(id VARCHAR(50) PRIMARY KEY,email VARCHAR(50) UNIQUE,name TEXT); CREATE TABLE review_upsert_composite(tenant BIGINT,id VARCHAR(50),email VARCHAR(50) UNIQUE,name TEXT,PRIMARY KEY(tenant,id))").await.unwrap();
    db.exec_raw("INSERT INTO review_upsert_key VALUES('target_key','target','original'),('victim','other','untouched'); INSERT INTO review_upsert_composite VALUES(1,'target_key','target','original'),(2,'victim','other','untouched')").await.unwrap();
    for key in ["victim", "unused"] {
        assert!(
            Natural::on_conflict(vec!["email"])
                .update_columns(vec!["id", "name"])
                .insert(Natural {
                    id: key.into(),
                    email: "target".into(),
                    name: "changed".into()
                })
                .await
                .unwrap_err()
                .to_string()
                .contains("primary key")
        );
        assert!(
            Composite::on_conflict(vec!["email"])
                .update_columns(vec!["tenant", "id", "name"])
                .insert(Composite {
                    tenant: 2,
                    id: key.into(),
                    email: "target".into(),
                    name: "changed".into()
                })
                .await
                .unwrap_err()
                .to_string()
                .contains("primary key")
        );
    }
    for table in ["review_upsert_key", "review_upsert_composite"] {
        assert_eq!(
            db.query_raw_json(&format!("SELECT email,name FROM {table} ORDER BY email"))
                .await
                .unwrap(),
            vec![
                serde_json::json!({"email":"other","name":"untouched"}),
                serde_json::json!({"email":"target","name":"original"})
            ]
        );
        db.exec_raw(&format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// A one-connection pool of the backend under test with a fresh table, so the
/// transaction tests below run on a connection of their own.
async fn private_table(prefix: &str) -> (Database, String) {
    let db = Database::builder()
        .url(backend::database_url())
        .max_connections(1)
        .build()
        .await
        .unwrap();
    let table = format!("{prefix}_{}", uuid::Uuid::new_v4().simple());
    db.exec_raw(&format!("CREATE TABLE {table}(id BIGINT PRIMARY KEY)"))
        .await
        .unwrap();
    (db, table)
}

async fn table_ids(db: &Database, table: &str) -> Vec<i64> {
    db.query_raw_json(&format!("SELECT id FROM {table} ORDER BY id"))
        .await
        .unwrap()
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

#[tideorm::model(table = "review_abort_rows")]
struct ReviewAbortRow {
    #[tideorm(primary_key)]
    id: i64,
}

/// After a caught failure PostgreSQL refuses the SAVEPOINT a nested
/// transaction or `insert_all` opens, and the driver then rolls the enclosing
/// transaction back: it must fail at commit, not commit what followed.
#[tokio::test]
async fn review_postgres_savepoint_after_a_failed_statement_aborts_the_transaction() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let (db, table) = private_table("review_failed_savepoint").await;
    db.exec_raw("CREATE TABLE IF NOT EXISTS review_abort_rows(id BIGINT PRIMARY KEY)")
        .await
        .unwrap();
    for nested_insert_all in [false, true] {
        db.exec_raw(&format!("DELETE FROM {table}")).await.unwrap();
        let (handle, name) = (db.clone(), table.clone());
        let result: tideorm::Result<()> = db
            .transaction(move |_| {
                Box::pin(async move {
                    handle
                        .exec_raw(&format!("INSERT INTO {name} VALUES (1)"))
                        .await?;
                    let duplicate = handle
                        .exec_raw(&format!("INSERT INTO {name} VALUES (1)"))
                        .await;
                    assert!(duplicate.is_err());
                    let nested = if nested_insert_all {
                        ReviewAbortRow::insert_all(vec![
                            ReviewAbortRow { id: 5 },
                            ReviewAbortRow { id: 6 },
                        ])
                        .await
                        .map(|_| ())
                    } else {
                        let inner_handle = handle.clone();
                        handle
                            .transaction(move |_| {
                                Box::pin(async move {
                                    inner_handle.exec_raw("SELECT 1").await?;
                                    Ok(())
                                })
                            })
                            .await
                    };
                    assert!(nested.is_err(), "the nested savepoint opened: {nested:?}");
                    let after = handle
                        .exec_raw(&format!("INSERT INTO {name} VALUES (3)"))
                        .await;
                    assert!(after.is_err(), "a statement ran after the abort: {after:?}");
                    Ok(())
                })
            })
            .await;
        assert!(result.is_err(), "the aborted transaction committed");
        assert_eq!(table_ids(&db, &table).await, Vec::<i64>::new());
    }
    assert_eq!(table_ids(&db, "review_abort_rows").await, Vec::<i64>::new());
    db.exec_raw(&format!("DROP TABLE {table}")).await.unwrap();
    db.exec_raw("DROP TABLE review_abort_rows").await.unwrap();
}

/// A statement that fails on a transaction marks that transaction, even when
/// it runs through an outer database's handle inside another database's
/// transaction: PostgreSQL's aborted transaction must then refuse to commit
/// rather than roll back silently, and the inner one still commits.
#[tokio::test]
async fn review_postgres_failure_through_an_outer_handle_marks_its_own_transaction() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let (outer, table) = private_table("review_outer_failure").await;
    let (inner, inner_table) = private_table("review_inner_commit").await;
    let (outer_handle, inner_handle) = (outer.clone(), inner.clone());
    let (name, inner_name) = (table.clone(), inner_table.clone());
    let result: tideorm::Result<()> = outer
        .transaction(move |_| {
            Box::pin(async move {
                outer_handle
                    .exec_raw(&format!("INSERT INTO {name} VALUES (1)"))
                    .await?;
                let (failing, writer) = (outer_handle.clone(), inner_handle.clone());
                inner_handle
                    .transaction(move |_| {
                        Box::pin(async move {
                            let missing = failing
                                .exec_raw("SELECT * FROM review_outer_failure_missing")
                                .await;
                            assert!(missing.is_err());
                            writer
                                .exec_raw(&format!("INSERT INTO {inner_name} VALUES (2)"))
                                .await?;
                            Ok(())
                        })
                    })
                    .await
            })
        })
        .await;
    assert!(result.is_err(), "the aborted transaction committed");
    assert_eq!(table_ids(&outer, &table).await, Vec::<i64>::new());
    assert_eq!(table_ids(&inner, &inner_table).await, vec![2]);
    outer
        .exec_raw(&format!("DROP TABLE {table}"))
        .await
        .unwrap();
    inner
        .exec_raw(&format!("DROP TABLE {inner_table}"))
        .await
        .unwrap();
}

/// A value of a type no typed decoder reads comes back as PostgreSQL prints
/// it, an array of one with its `NULL`s and dimensions, and never as `null`;
/// a value nothing can read fails the read.
#[tokio::test]
async fn review_postgres_raw_rows_read_types_without_a_typed_decoder() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    // Each value beside `format('%s', v)`, the server's own text for it, in
    // one transaction holding the settings that text follows.
    let checks: [(&str, &[&str]); 10] = [
        (
            "interval",
            &[
                "1 year 2 months 3 days 04:05:06.789",
                "-1 year -2 months +3 days -04:05:06",
                "0",
                "-0.000001 seconds",
                "100 hours",
                "1 mon -1 day",
            ],
        ),
        (
            "timetz",
            &[
                "10:30:00+02",
                "10:30:00.25-05:30",
                "12:00:00+05:30:15",
                "24:00:00-01",
            ],
        ),
        (
            "inet",
            &[
                "192.168.1.5",
                "192.168.1.0/24",
                "::1",
                "::1.2.3.4",
                "::ffff:1.2.3.4",
                "2001:db8:0:0:1:0:0:1",
                "2001:db8::1/64",
            ],
        ),
        ("cidr", &["10.0.0.0/8", "192.168.1.5/32", "2001:db8::/32"]),
        ("int4range", &["[1,10)", "empty", "(,5]", "(,)"]),
        ("int8range", &["[9000000000,9000000001]"]),
        (
            "numrange",
            &["[1.5,2.25)", "(,1234567890123456789012345678901.5]"],
        ),
        (
            "daterange",
            &[
                "[2020-01-01,2020-01-31]",
                "[2020-01-01,infinity)",
                "[0044-03-15 BC,0001-01-01)",
            ],
        ),
        (
            "tsrange",
            &[
                "[2020-01-01 10:00,2020-01-01 10:30:00.5)",
                "[2020-01-01,infinity)",
            ],
        ),
        ("tstzrange", &["[2020-01-01 10:00+02,2020-01-02 00:00+00)"]),
    ];
    let db = tideorm::require_db().unwrap();
    let rows = db
        .transaction(move |_| {
            Box::pin(async move {
                Database::execute("SET LOCAL intervalstyle = 'iso_8601'").await?;
                Database::execute("SET LOCAL timezone = 'UTC'").await?;
                Database::execute("SET LOCAL datestyle = 'ISO, MDY'").await?;
                let mut rows = Vec::new();
                for (type_name, values) in checks {
                    let values = values
                        .iter()
                        .map(|value| format!("('{value}'::{type_name})"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    rows.extend(
                        Database::raw_json(&format!(
                            "SELECT v, format('%s', v) AS text FROM (VALUES {values}) AS t(v)"
                        ))
                        .await?,
                    );
                }
                // A MONEY is its amount, which the server prints by locale.
                rows.extend(
                    Database::raw_json(
                        "SELECT v, v::numeric::text AS text FROM (VALUES ('1234.56'::money), \
                         ('-0.01'::money), ('92233720368547758.07'::money)) AS t(v)",
                    )
                    .await?,
                );
                Ok(rows)
            })
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 36);
    for row in &rows {
        assert!(row["v"].is_string(), "{row}");
        assert_eq!(row["v"], row["text"], "{row}");
    }

    let row = Database::raw_json(
        "SELECT ARRAY['2020-01-01', NULL, 'infinity']::date[] AS dates, \
         ARRAY[['2020-01-01 10:00', '2020-01-02'], [NULL, '-infinity']]::timestamp[] AS stamps, \
         ARRAY['2020-01-01 10:00+00']::timestamptz[] AS zoned, \
         ARRAY['10:00:00.5', '24:00:00']::time[] AS times, \
         ARRAY['10:30:00+02', NULL]::timetz[] AS zoned_times, \
         ARRAY['1 day', NULL]::interval[] AS intervals, \
         ARRAY['10.0.0.1', '::1/64']::inet[] AS addresses, \
         ARRAY['1.50']::money[] AS amounts, \
         ARRAY['[1,2)', 'empty']::int4range[] AS ranges, \
         ARRAY[1, NULL]::int4[] AS numbers, \
         date '2020-01-01' AS date, timestamp '2020-01-01 10:00' AS stamp, \
         timestamp '2020-01-02' AS midnight, timestamptz '2020-01-01 10:00+00' AS zoned_stamp, \
         NULL::point AS no_point",
    )
    .await
    .unwrap()
    .remove(0);
    let expected = serde_json::json!({
        "dates": [row["date"], null, "infinity"],
        "stamps": [[row["stamp"], row["midnight"]], [null, "-infinity"]],
        "zoned": [row["zoned_stamp"]],
        "times": ["10:00:00.500", "24:00:00"],
        "zoned_times": ["10:30:00+02", null],
        "intervals": ["P1D", null],
        "addresses": ["10.0.0.1", "::1/64"],
        "amounts": ["1.50"],
        "ranges": ["[1,2)", "empty"],
        "numbers": [1, null],
        "no_point": null,
    });
    for (column, value) in expected.as_object().unwrap() {
        assert_eq!(&row[column], value, "{column}: {row}");
    }

    // An enum's labels, which only their text names; the type lives and dies
    // inside the transaction.
    let rows = db
        .transaction(|_| {
            Box::pin(async {
                Database::execute("CREATE TYPE review_decoded_mood AS ENUM ('happy', 'sad')")
                    .await?;
                let rows = Database::raw_json(
                    "SELECT 'sad'::review_decoded_mood AS mood, \
                     ARRAY['happy', NULL]::review_decoded_mood[] AS moods",
                )
                .await?;
                Database::execute("DROP TYPE review_decoded_mood").await?;
                Ok(rows)
            })
        })
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![serde_json::json!({"mood": "sad", "moods": ["happy", null]})]
    );

    let error = Database::raw_json("SELECT point(1, 2) AS p")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("column 'p' of type POINT holds a value TideORM cannot read as JSON"),
        "{error}"
    );
}

#[tideorm::model(table = "review_special_moments")]
struct ReviewSpecialMoment {
    #[tideorm(primary_key)]
    id: i64,
    at: chrono::DateTime<chrono::Utc>,
}

/// sqlx panics on a date or timestamp holding `infinity` and reads `TIME
/// '24:00:00'` as midnight; JSON reads give PostgreSQL's own text for them,
/// raw and through a model, and other values as before.
#[tokio::test]
async fn review_postgres_special_dates_and_times_read_as_postgres_prints_them() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let rows = db
        .query_raw_json(
            "SELECT 'infinity'::date AS d, '-infinity'::timestamp AS t, \
             'infinity'::timestamptz AS tz, '24:00:00'::time AS tm, \
             '2024-01-02'::date AS normal_date, '10:30:00'::time AS normal_time",
        )
        .await
        .unwrap();
    assert_eq!(
        rows[0],
        serde_json::json!({
            "d": "infinity",
            "t": "-infinity",
            "tz": "infinity",
            "tm": "24:00:00",
            "normal_date": "2024-01-02",
            "normal_time": "10:30:00",
        })
    );

    db.exec_raw("DROP TABLE IF EXISTS review_special_moments")
        .await
        .unwrap();
    db.exec_raw(
        "CREATE TABLE review_special_moments(id BIGINT PRIMARY KEY, at TIMESTAMPTZ NOT NULL); \
         INSERT INTO review_special_moments VALUES (1, 'infinity'), (2, '2024-01-02 03:04:05+00')",
    )
    .await
    .unwrap();
    let rows = ReviewSpecialMoment::query_with(&db)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["at"], "infinity");
    assert_eq!(rows[1]["at"], "2024-01-02T03:04:05Z");

    // A typed read cannot hold the value: an error, not the driver's panic.
    for result in [
        ReviewSpecialMoment::find_with(1, &db).await.map(drop),
        ReviewSpecialMoment::query_with(&db).get().await.map(drop),
    ] {
        let error = result.unwrap_err();
        assert!(
            matches!(error, tideorm::Error::Conversion { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("'at'"), "{error}");
    }
    assert_eq!(
        ReviewSpecialMoment::find_with(2, &db)
            .await
            .unwrap()
            .unwrap()
            .at
            .to_rfc3339(),
        "2024-01-02T03:04:05+00:00"
    );
    db.exec_raw("DROP TABLE review_special_moments")
        .await
        .unwrap();
}

/// sqlx cannot read PostgreSQL's multirange types; the error says so and how
/// to read one, and the text of one reads.
#[tokio::test]
async fn review_postgres_multiranges_fail_with_a_way_to_read_them() {
    if backend::DATABASE_TYPE != DatabaseType::Postgres || !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().unwrap();
    let error = db
        .query_raw_json("SELECT '{[1,3)}'::int4multirange AS m")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("multirange"), "{error}");
    assert!(error.contains("::text"), "{error}");
    let rows = db
        .query_raw_json("SELECT '{[1,3)}'::int4multirange::text AS m")
        .await
        .unwrap();
    assert_eq!(rows[0]["m"], "{[1,3)}");
}
