//! MariaDB integration tests: the shared scenarios in
//! `support/integration_parity.rs` plus the MySQL-family ones in
//! `support/mysql_family.rs`, against a MariaDB server reached through a
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

#[path = "support/integration_parity.rs"]
mod parity;

#[path = "support/mysql_family.rs"]
mod mysql_family;
