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

#[path = "support/mariadb_test_config.rs"]
mod test_config;

mod backend {
    use tideorm::TideConfig;
    use tideorm::config::DatabaseType;

    use super::test_config::{mariadb_database_url, should_run_mariadb_tests};

    pub const DATABASE_TYPE: DatabaseType = DatabaseType::MariaDB;

    pub async fn connect() -> bool {
        if !should_run_mariadb_tests() {
            println!(
                "Skipping MariaDB test: set RUN_MARIADB_TESTS or MARIADB_DATABASE_URL (SKIP_MARIADB_TESTS overrides both)"
            );
            return false;
        }
        TideConfig::init()
            .database(mariadb_database_url())
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
