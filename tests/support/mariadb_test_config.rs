use std::sync::OnceLock;

static MARIADB_DATABASE_URL: OnceLock<String> = OnceLock::new();

pub fn mariadb_database_url() -> &'static str {
    MARIADB_DATABASE_URL.get_or_init(|| {
        let _ = dotenvy::dotenv();

        std::env::var("MARIADB_DATABASE_URL")
            .unwrap_or_else(|_| "mysql://root:@localhost:3306/test_tide_orm".to_string())
    })
}

/// MariaDB suites are opt-in via `RUN_MARIADB_TESTS` or `MARIADB_DATABASE_URL`;
/// `SKIP_MARIADB_TESTS` wins over both, so a URL kept in `.env` can be muted.
pub fn should_run_mariadb_tests() -> bool {
    let _ = dotenvy::dotenv();
    if std::env::var_os("SKIP_MARIADB_TESTS").is_some() {
        return false;
    }
    std::env::var_os("RUN_MARIADB_TESTS").is_some()
        || std::env::var_os("MARIADB_DATABASE_URL").is_some()
}
