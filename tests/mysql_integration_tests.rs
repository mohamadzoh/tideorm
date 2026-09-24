//! MySQL integration tests: the shared scenarios in
//! `support/integration_parity.rs` plus the MySQL-family ones in
//! `support/mysql_family.rs`.
//!
//! Opt-in: set `RUN_MYSQL_TESTS` or `MYSQL_DATABASE_URL`; `SKIP_MYSQL_TESTS`
//! turns them off again. Once enabled, an unreachable server fails the run.
//!
//! Run with:
//! cargo test --test mysql_integration_tests --no-default-features --features mysql,runtime-tokio

#[path = "support/mysql_test_config.rs"]
mod test_config;

mod backend {
    use tideorm::TideConfig;
    use tideorm::config::DatabaseType;

    use super::test_config::{mysql_database_url, should_run_mysql_tests};

    pub const DATABASE_TYPE: DatabaseType = DatabaseType::MySQL;

    pub async fn connect() -> bool {
        if !should_run_mysql_tests() {
            println!(
                "Skipping MySQL test: set RUN_MYSQL_TESTS or MYSQL_DATABASE_URL (SKIP_MYSQL_TESTS overrides both)"
            );
            return false;
        }
        TideConfig::init()
            .database_type(DatabaseType::MySQL)
            .database(mysql_database_url())
            .max_connections(5)
            .connect()
            .await
            .expect("failed to connect to MySQL");
        true
    }
}

#[path = "support/integration_parity.rs"]
mod parity;

#[path = "support/mysql_family.rs"]
mod mysql_family;
