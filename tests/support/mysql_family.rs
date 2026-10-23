//! Scenarios for MySQL and MariaDB only, which both of their targets run: how
//! raw SQL rows decode, and JSON functions the other backends lack.
//!
//! The including target supplies `mod backend` with `DATABASE_TYPE` and
//! `async fn connect() -> bool`, and the shared scenarios as `mod parity`.

use tideorm::Database;
use tideorm::config::DatabaseType;
use tideorm::prelude::*;

use super::{backend, parity};

/// `change_column` keeps what MySQL's `MODIFY COLUMN` would drop — NOT NULL,
/// the default, AUTO_INCREMENT, the comment — as PostgreSQL's `ALTER TYPE`
/// keeps them.
#[tokio::test]
async fn change_column_keeps_the_column_attributes() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS `changed_columns`")
        .await
        .expect("failed to drop changed_columns");
    Database::execute(
        "CREATE TABLE `changed_columns` (
            `id` INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
            `age` INT NOT NULL DEFAULT 5 COMMENT 'years',
            `note` VARCHAR(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL
        ) ENGINE=InnoDB",
    )
    .await
    .expect("failed to create changed_columns");

    tideorm::migration::Schema::new(backend::DATABASE_TYPE)
        .alter_table("changed_columns", |t| {
            t.change_column("id", tideorm::migration::ColumnType::BigInteger)
                .change_column("age", tideorm::migration::ColumnType::BigInteger)
                .change_column("note", tideorm::migration::ColumnType::Text);
        })
        .await
        .expect("change_column failed");

    let columns = Database::raw_json(
        "SELECT COLUMN_NAME AS name, DATA_TYPE AS data_type, IS_NULLABLE AS nullable, \
         COLUMN_DEFAULT AS default_value, EXTRA AS extra, COLUMN_COMMENT AS comment \
         FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'changed_columns' \
         ORDER BY ORDINAL_POSITION",
    )
    .await
    .expect("failed to read the columns");
    let column = |name: &str| {
        columns
            .iter()
            .find(|column| column["name"] == name)
            .unwrap_or_else(|| panic!("no column {name}: {columns:?}"))
            .clone()
    };
    let (id, age, note) = (column("id"), column("age"), column("note"));

    assert_eq!(id["data_type"], "bigint");
    assert_eq!(id["nullable"], "NO");
    assert!(
        id["extra"]
            .as_str()
            .unwrap_or_default()
            .contains("auto_increment"),
        "{id}"
    );
    assert_eq!(age["data_type"], "bigint");
    assert_eq!(age["nullable"], "NO");
    // MariaDB quotes a literal default in `COLUMN_DEFAULT`, MySQL does not.
    assert!(
        matches!(age["default_value"].as_str(), Some("5" | "'5'")),
        "{age}"
    );
    assert_eq!(age["comment"], "years");
    assert_eq!(
        note["data_type"], "longtext",
        "`ColumnType::Text` is LONGTEXT here"
    );
    assert_eq!(note["nullable"], "YES");
}

#[tokio::test]
async fn raw_json_decodes_each_column_by_its_declaration() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS `test_raw_json_types`")
        .await
        .expect("failed to drop test_raw_json_types");
    Database::execute(
        "CREATE TABLE `test_raw_json_types` (
            `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
            `enabled` BOOLEAN NOT NULL,
            `payload` JSON NOT NULL,
            `amount` DECIMAL(10,2) NOT NULL,
            `created_at` DATETIME NOT NULL,
            `code` VARCHAR(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
            `raw` VARBINARY(8) NOT NULL
        ) ENGINE=InnoDB",
    )
    .await
    .expect("failed to create test_raw_json_types");

    let db = tideorm::require_db().expect("database should be available");
    db.__execute_with_params(
        "INSERT INTO `test_raw_json_types` (`enabled`, `payload`, `amount`, `created_at`, `code`, `raw`) VALUES (?, ?, ?, ?, ?, ?)",
        vec![
            tideorm::internal::Value::Bool(Some(true)),
            tideorm::internal::Value::Json(Some(Box::new(serde_json::json!({
                "kind": "probe",
                "count": 2
            })))),
            tideorm::internal::Value::String(Some("12.34".to_string())),
            tideorm::internal::Value::String(Some("2026-03-21 10:11:12".to_string())),
            tideorm::internal::Value::String(Some("AbC-01".to_string())),
            tideorm::internal::Value::Bytes(Some(vec![0, 1, 255])),
        ],
    )
    .await
    .expect("typed raw-json probe insert should succeed");

    let rows = db
        .__raw_json_with_params(
            "SELECT `enabled`, `payload`, `amount`, `created_at`, `code`, `raw` FROM `test_raw_json_types` ORDER BY `id` ASC",
            vec![],
        )
        .await
        .expect("typed raw-json probe query should succeed");

    // MariaDB declares a JSON column as text, so raw SQL reads its text.
    let payload = match backend::DATABASE_TYPE {
        DatabaseType::MariaDB => serde_json::json!(r#"{"count":2,"kind":"probe"}"#),
        _ => serde_json::json!({ "kind": "probe", "count": 2 }),
    };
    assert_eq!(
        rows,
        vec![serde_json::json!({
            "enabled": true,
            "payload": payload,
            "amount": serde_json::to_value(
                rust_decimal::Decimal::from_str_exact("12.34")
                    .expect("decimal literal should parse")
            ).expect("decimal should serialize to JSON"),
            "created_at": serde_json::to_value(
                chrono::NaiveDateTime::parse_from_str("2026-03-21 10:11:12", "%Y-%m-%d %H:%M:%S")
                    .expect("datetime literal should parse")
            ).expect("datetime should serialize to JSON"),
            // Text with a binary collation is still text; binary data is bytes.
            "code": "AbC-01",
            "raw": [0, 1, 255],
        })]
    );
}

/// A `TIME` is a signed duration of up to 838 hours rather than a time of
/// day, and reads as one; a time of day reads as a `NaiveTime` writes it.
#[tokio::test]
async fn raw_time_values_keep_their_sign_and_hours() {
    if !backend::connect().await {
        return;
    }
    let rows = Database::raw_json(
        "SELECT CAST('-01:30:00' AS TIME) AS negative, CAST('25:00:00' AS TIME) AS past_a_day, \
         CAST('838:59:59' AS TIME) AS longest, CAST('-838:59:59' AS TIME) AS shortest, \
         CAST('-00:00:01.000001' AS TIME(6)) AS micros, CAST('10:20:30.5' AS TIME(6)) AS half, \
         CAST('12:00:00' AS TIME) AS noon, CAST(NULL AS TIME) AS missing",
    )
    .await
    .expect("failed to read the times");
    assert_eq!(
        rows,
        vec![serde_json::json!({
            "negative": "-01:30:00",
            "past_a_day": "25:00:00",
            "longest": "838:59:59",
            "shortest": "-838:59:59",
            "micros": "-00:00:01.000001",
            "half": serde_json::to_value(chrono::NaiveTime::from_hms_milli_opt(10, 20, 30, 500))
                .unwrap(),
            "noon": "12:00:00",
            "missing": null,
        })]
    );

    // A model's `TIME` column, which the driver reads without its sign.
    #[tideorm::model(table = "test_time_spans")]
    struct TimeSpan {
        #[tideorm(primary_key)]
        id: i64,
        spent: Option<chrono::NaiveTime>,
    }
    Database::execute("DROP TABLE IF EXISTS `test_time_spans`")
        .await
        .expect("failed to drop test_time_spans");
    Database::execute(
        "CREATE TABLE `test_time_spans` (`id` BIGINT PRIMARY KEY, `spent` TIME(6) NULL); \
         INSERT INTO `test_time_spans` VALUES (1, '-01:30:00'), (2, '100:00:00.25'), (3, '08:15:00')",
    )
    .await
    .expect("failed to create test_time_spans");
    let spans = TimeSpan::query()
        .order_asc("id")
        .pluck::<String>("spent")
        .await
        .expect("failed to read the spans");
    assert_eq!(spans, ["-01:30:00", "100:00:00.250", "08:15:00"]);
    Database::execute("DROP TABLE `test_time_spans`")
        .await
        .expect("failed to drop test_time_spans");
}

#[tokio::test]
async fn json_extract_filters_on_a_json_column() {
    if !parity::setup().await {
        return;
    }

    for (name, brand) in [("Laptop", "TechCorp"), ("Phone", "MobileCo")] {
        parity::TestProduct {
            id: 0,
            name: name.to_string(),
            category: "Electronics".to_string(),
            price: 100,
            metadata: Some(serde_json::json!({ "brand": brand })),
        }
        .save()
        .await
        .expect("Failed to save product");
    }

    let rows = Database::raw_json(
        "SELECT `name` FROM `test_products` WHERE JSON_UNQUOTE(JSON_EXTRACT(`metadata`, '$.brand')) = 'TechCorp'",
    )
    .await
    .expect("JSON_EXTRACT query should run");
    assert_eq!(rows, vec![serde_json::json!({ "name": "Laptop" })]);
}

/// A migrator cancelled while it holds the migration lock frees the lock. The
/// named lock belongs to a session, whose pooled connection used to go back to
/// the pool still holding it, so every later migrator waited out its timeout.
#[tokio::test]
async fn a_cancelled_migration_run_frees_the_migration_lock() {
    use std::time::{Duration, Instant};
    use tideorm::migration::{Migration, Migrator, Schema};

    struct Stalls;

    #[async_trait]
    impl Migration for Stalls {
        fn version(&self) -> &str {
            "20260926_001"
        }

        fn name(&self) -> &str {
            "stalls"
        }

        async fn up(&self, _schema: &mut Schema) -> tideorm::Result<()> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        }

        async fn down(&self, _schema: &mut Schema) -> tideorm::Result<()> {
            Ok(())
        }
    }

    if !backend::connect().await {
        return;
    }
    let ledger = "cancelled_lock_migrations";
    let migrator = Migrator::new().migrations_table(ledger).add(Stalls);
    assert!(
        tokio::time::timeout(Duration::from_millis(500), migrator.run())
            .await
            .is_err(),
        "the stalled migration should still be running"
    );

    let lock_is_free = || async {
        let rows = Database::raw_json("SELECT IS_FREE_LOCK('tideorm_migrations') AS free")
            .await
            .expect("IS_FREE_LOCK failed");
        rows[0]["free"] == serde_json::json!(1)
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !lock_is_free().await {
        assert!(
            Instant::now() < deadline,
            "the cancelled migrator still holds the migration lock"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // A run that completes takes the lock and gives it back.
    struct Quick;

    #[async_trait]
    impl Migration for Quick {
        fn version(&self) -> &str {
            "20260926_002"
        }

        fn name(&self) -> &str {
            "quick"
        }

        async fn up(&self, _schema: &mut Schema) -> tideorm::Result<()> {
            Ok(())
        }

        async fn down(&self, _schema: &mut Schema) -> tideorm::Result<()> {
            Ok(())
        }
    }
    let result = Migrator::new()
        .migrations_table(ledger)
        .add(Quick)
        .run()
        .await
        .expect("a migrator after the cancelled one should run");
    assert_eq!(result.applied.len(), 1);
    assert!(lock_is_free().await, "a finished run releases the lock");

    Database::execute(&format!("DROP TABLE IF EXISTS `{ledger}`"))
        .await
        .expect("failed to drop the ledger");
}

/// A database opened with `Database::connect`, which reads no configuration,
/// knows whether it reached MariaDB, as `TideConfig::connect` does: it
/// rendered MySQL's SQL for MariaDB, such as `CAST(? AS JSON)`, which MariaDB
/// lacks.
#[tokio::test]
async fn a_database_opened_directly_knows_its_server() {
    if !backend::connect().await {
        return;
    }
    let db = Database::connect(backend::database_url())
        .await
        .expect("connect failed");
    assert_eq!(db.backend(), backend::DATABASE_TYPE);
}

/// A seeder inside a transaction that holds the pool's only connection finds
/// its ledger on that connection. The ledger was created, `IF NOT EXISTS`,
/// on a second pooled connection every time, which never came, so the seeder
/// waited out the pool's timeout even with the ledger already there.
#[tokio::test]
async fn a_seeder_in_a_transaction_on_a_one_connection_pool_uses_its_ledger() {
    use std::time::Duration;
    use tideorm::seeding::Seeder;

    if !backend::connect().await {
        return;
    }
    Seeder::new()
        .run()
        .await
        .expect("creating the ledger failed");

    tideorm::TideConfig::init()
        .database(backend::database_url())
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("connect failed");
    let db = tideorm::require_db().expect("a global database");
    let seeded = db
        .transaction(|_| Box::pin(async { Seeder::new().run().await }))
        .await;
    assert!(seeded.is_ok(), "{seeded:?}");
}

#[tideorm::model(table = "dialect_documents")]
pub struct DialectDocument {
    #[tideorm(primary_key)]
    pub id: i64,
    pub data: serde_json::Value,
}

/// The SQL a statement renders follows the pool it runs on, which asked the
/// server what it is when it opened, never the configured backend: inside a
/// transaction on a pool that is not the global one, and under a
/// configuration naming the other server.
#[tokio::test]
async fn json_equality_follows_the_pool_not_the_configuration() {
    if !backend::connect().await {
        return;
    }
    Database::execute("DROP TABLE IF EXISTS `dialect_documents`")
        .await
        .expect("failed to drop dialect_documents");
    Database::execute(
        "CREATE TABLE `dialect_documents` (`id` BIGINT NOT NULL PRIMARY KEY, `data` JSON NOT NULL)",
    )
    .await
    .expect("failed to create dialect_documents");
    Database::execute(r#"INSERT INTO `dialect_documents` VALUES (1, '{"a": 1}'), (2, '{"a": 2}')"#)
        .await
        .expect("failed to insert dialect_documents");
    let pool = Database::connect(backend::database_url())
        .await
        .expect("failed to open a second pool");
    let other = match backend::DATABASE_TYPE {
        DatabaseType::MariaDB => DatabaseType::MySQL,
        _ => DatabaseType::MariaDB,
    };
    let matching =
        || DialectDocument::query().where_eq(DialectDocument::DATA, serde_json::json!({ "a": 2 }));

    for configured in [None, Some(other)] {
        let config = tideorm::TideConfig::init();
        match configured {
            Some(database_type) => config.database_type(database_type).apply(),
            None => config.apply(),
        }
        let in_transaction = pool
            .transaction(|_| Box::pin(async move { matching().get().await }))
            .await
            .unwrap_or_else(|error| panic!("configured {configured:?}: {error}"));
        assert_eq!(in_transaction.len(), 1, "configured {configured:?}");
        assert_eq!(in_transaction[0].id, 2);

        let with_handle = DialectDocument::query_with(&pool)
            .where_eq(DialectDocument::DATA, serde_json::json!({ "a": 2 }))
            .get()
            .await
            .unwrap_or_else(|error| panic!("configured {configured:?}: {error}"));
        assert_eq!(with_handle.len(), 1, "configured {configured:?}");
    }
    tideorm::TideConfig::reset();
}
