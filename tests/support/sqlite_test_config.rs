use std::sync::OnceLock;

static SQLITE_DATABASE_URL: OnceLock<String> = OnceLock::new();

pub fn sqlite_database_url() -> &'static str {
    SQLITE_DATABASE_URL.get_or_init(|| {
        let _ = dotenvy::dotenv();

        std::env::var("SQLITE_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://./test_tide_orm.db?mode=rwc".to_string())
    })
}

pub fn should_run_sqlite_tests() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var("SKIP_SQLITE_TESTS").is_err()
}
