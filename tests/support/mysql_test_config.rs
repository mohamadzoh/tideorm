use std::sync::OnceLock;

static MYSQL_DATABASE_URL: OnceLock<String> = OnceLock::new();

pub fn mysql_database_url() -> &'static str {
    MYSQL_DATABASE_URL.get_or_init(|| {
        let _ = dotenvy::dotenv();

        std::env::var("MYSQL_DATABASE_URL")
            .unwrap_or_else(|_| "mysql://root:@localhost:3306/test_tide_orm".to_string())
    })
}

pub fn should_run_mysql_tests() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var("RUN_MYSQL_TESTS").is_ok() || std::env::var("MYSQL_DATABASE_URL").is_ok()
}
