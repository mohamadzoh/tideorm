#![allow(dead_code)]

use std::sync::OnceLock;

static POSTGRESQL_DATABASE_URL: OnceLock<String> = OnceLock::new();

/// The PostgreSQL suites are opt-in, like the MySQL ones: they run when
/// `RUN_POSTGRES_TESTS`, `TEST_DATABASE_URL` or `POSTGRESQL_DATABASE_URL` is
/// set, and `SKIP_POSTGRES_TESTS` wins over all three. Once enabled, an
/// unreachable server fails the run.
pub fn should_run_postgres_tests() -> bool {
    let _ = dotenvy::dotenv();
    if std::env::var_os("SKIP_POSTGRES_TESTS").is_some() {
        return false;
    }
    [
        "RUN_POSTGRES_TESTS",
        "TEST_DATABASE_URL",
        "POSTGRESQL_DATABASE_URL",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some())
}

/// Printed by a PostgreSQL test that [`should_run_postgres_tests`] turned off.
pub const SKIPPED: &str = "Skipping PostgreSQL test: set POSTGRESQL_DATABASE_URL, TEST_DATABASE_URL or RUN_POSTGRES_TESTS (SKIP_POSTGRES_TESTS overrides them)";

pub fn test_database_url() -> &'static str {
    POSTGRESQL_DATABASE_URL.get_or_init(|| {
        let _ = dotenvy::dotenv();

        std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("POSTGRESQL_DATABASE_URL"))
            .unwrap_or_else(|_| {
                "postgres://postgres:postgres@localhost:5432/test_tide_orm".to_string()
            })
    })
}
