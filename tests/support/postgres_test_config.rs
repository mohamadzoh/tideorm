use std::sync::OnceLock;

static POSTGRESQL_DATABASE_URL: OnceLock<String> = OnceLock::new();

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
