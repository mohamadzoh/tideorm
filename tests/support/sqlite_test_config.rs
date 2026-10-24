#![allow(dead_code)]

use std::sync::OnceLock;

static SQLITE_DATABASE_URL: OnceLock<String> = OnceLock::new();

/// `SQLITE_DATABASE_URL`, or a file in the working directory. Only the
/// entity-manager suite connects with it.
pub fn sqlite_database_url() -> &'static str {
    SQLITE_DATABASE_URL.get_or_init(|| {
        let _ = dotenvy::dotenv();

        std::env::var("SQLITE_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://./test_tide_orm.db?mode=rwc".to_string())
    })
}

/// The SQLite suites are opt-out: they run unless `SKIP_SQLITE_TESTS` is set.
pub fn should_run_sqlite_tests() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var("SKIP_SQLITE_TESTS").is_err()
}
