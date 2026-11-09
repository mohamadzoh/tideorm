//! MariaDB integration tests: the scenarios every backend runs, in
//! `support/integration_parity.rs`; those of the server backends, in
//! `support/review_backend.rs`; and the MySQL-family ones, in
//! `support/mysql_family.rs`. They reach the MariaDB server through a
//! `mysql://` URL, which `connect()` has to recognize.
//!
//! Opt-in: set `RUN_MARIADB_TESTS` or `MARIADB_DATABASE_URL`;
//! `SKIP_MARIADB_TESTS` turns them off again. Once enabled, an unreachable
//! server fails the run.
//!
//! Run with:
//! cargo test --test mariadb_integration_tests --no-default-features --features mysql,runtime-tokio

#[path = "support/mysql_family_test_config.rs"]
mod test_config;

static SERVER: test_config::Server = test_config::Server::new("MariaDB", "MARIADB");

mod backend {
    use tideorm::TideConfig;
    use tideorm::config::DatabaseType;

    use super::SERVER;

    pub const DATABASE_TYPE: DatabaseType = DatabaseType::MariaDB;

    pub fn database_url() -> &'static str {
        SERVER.database_url()
    }

    pub async fn connect() -> bool {
        if !SERVER.enabled() {
            println!("{}", SERVER.skipped("test"));
            return false;
        }
        TideConfig::init()
            .database(SERVER.database_url())
            .max_connections(5)
            .connect()
            .await
            .expect("failed to connect to MariaDB");
        true
    }
}

#[path = "support/schema_script.rs"]
mod schema_script;

#[path = "support/integration_parity.rs"]
mod parity;

#[path = "support/catalog_parity.rs"]
mod catalog_parity;

#[path = "support/review_backend.rs"]
mod review_backend;

#[path = "support/mysql_family.rs"]
mod mysql_family;

/// MariaDB's `UUID` type is text on the wire, named `UUID` only by the
/// server's extended column metadata: a model writes, filters and reads it as
/// a `Uuid`, and raw SQL reads it as one too.
mod uuid_columns {
    use tideorm::Database;
    use tideorm::prelude::*;

    use super::backend;

    #[tideorm::model(table = "mariadb_uuid_tokens")]
    struct UuidToken {
        #[tideorm(primary_key)]
        id: i64,
        token: uuid::Uuid,
        spare: Option<uuid::Uuid>,
    }

    #[tokio::test]
    async fn uuid_columns_read_as_uuids() {
        if !backend::connect().await {
            return;
        }
        Database::execute("DROP TABLE IF EXISTS `mariadb_uuid_tokens`")
            .await
            .expect("failed to drop mariadb_uuid_tokens");
        Database::execute(
            "CREATE TABLE `mariadb_uuid_tokens` (
                `id` BIGINT NOT NULL PRIMARY KEY,
                `token` UUID NOT NULL,
                `spare` UUID NULL
            )",
        )
        .await
        .expect("failed to create mariadb_uuid_tokens");

        let token = uuid::Uuid::parse_str("0193a5b0-7c1e-7d2f-8a3b-4c5d6e7f8091").unwrap();
        let created = UuidToken::create(UuidToken {
            id: 1,
            token,
            spare: None,
        })
        .await
        .expect("failed to create a UUID row");
        assert_eq!(created.token, token);
        Database::execute(
            "INSERT INTO `mariadb_uuid_tokens` VALUES (2, '0193a5b0-7c1e-7d2f-8a3b-4c5d6e7f8092', NULL)",
        )
        .await
        .expect("failed to insert a UUID as text");

        let found = UuidToken::query()
            .where_eq("token", token)
            .get()
            .await
            .expect("failed to filter by a UUID");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, 1);
        let typed = UuidToken::find(2)
            .await
            .expect("failed to find a row")
            .expect("row 2 exists");
        assert_eq!(
            typed.token.to_string(),
            "0193a5b0-7c1e-7d2f-8a3b-4c5d6e7f8092"
        );
        assert_eq!(typed.spare, None);

        let rows =
            Database::raw_json("SELECT `token`, `spare` FROM `mariadb_uuid_tokens` ORDER BY `id`")
                .await
                .expect("failed to read raw rows");
        assert_eq!(
            rows,
            vec![
                serde_json::json!({ "token": token, "spare": null }),
                serde_json::json!({
                    "token": "0193a5b0-7c1e-7d2f-8a3b-4c5d6e7f8092",
                    "spare": null,
                }),
            ]
        );
    }
}
