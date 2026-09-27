use std::sync::OnceLock;

static MYSQL_DATABASE_URL: OnceLock<String> = OnceLock::new();

pub fn mysql_database_url() -> &'static str {
    MYSQL_DATABASE_URL.get_or_init(|| {
        let _ = dotenvy::dotenv();

        std::env::var("MYSQL_DATABASE_URL")
            .unwrap_or_else(|_| "mysql://root:@localhost:3306/test_tide_orm".to_string())
    })
}

/// MySQL suites are opt-in via `RUN_MYSQL_TESTS` or `MYSQL_DATABASE_URL`;
/// `SKIP_MYSQL_TESTS` wins over both, so a URL kept in `.env` can be muted.
pub fn should_run_mysql_tests() -> bool {
    let _ = dotenvy::dotenv();
    if std::env::var_os("SKIP_MYSQL_TESTS").is_some() {
        return false;
    }
    std::env::var_os("RUN_MYSQL_TESTS").is_some()
        || std::env::var_os("MYSQL_DATABASE_URL").is_some()
}
