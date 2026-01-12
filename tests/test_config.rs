//! Test configuration utilities
//!
//! Provides helpers for loading test database configuration from .env file

#![allow(dead_code)]

use std::sync::OnceLock;

static POSTGRESQL_DATABASE_URL: OnceLock<String> = OnceLock::new();
static MYSQL_DATABASE_URL: OnceLock<String> = OnceLock::new();
static SQLITE_DATABASE_URL: OnceLock<String> = OnceLock::new();

/// Get the PostgreSQL test database URL from environment or .env file
///
/// Loads from POSTGRESQL_DATABASE_URL environment variable, falling back to .env file
pub fn test_database_url() -> &'static str {
    postgres_database_url()
}

/// Get the PostgreSQL test database URL
pub fn postgres_database_url() -> &'static str {
    POSTGRESQL_DATABASE_URL.get_or_init(|| {
        // Load .env file if it exists
        let _ = dotenvy::dotenv();
        
        // Prefer TEST_DATABASE_URL for isolation, fall back to POSTGRESQL_DATABASE_URL
        std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("POSTGRESQL_DATABASE_URL"))
            .unwrap_or_else(|_| {
                // Fallback to default for backwards compatibility
                "postgres://postgres:postgres@localhost:5432/test_tide_orm".to_string()
            })
    })
}

/// Get the MySQL test database URL
pub fn mysql_database_url() -> &'static str {
    MYSQL_DATABASE_URL.get_or_init(|| {
        // Load .env file if it exists
        let _ = dotenvy::dotenv();
        
        std::env::var("MYSQL_DATABASE_URL")
            .unwrap_or_else(|_| {
                "mysql://root:@localhost:3306/test_tide_orm".to_string()
            })
    })
}

/// Get the SQLite test database URL
pub fn sqlite_database_url() -> &'static str {
    SQLITE_DATABASE_URL.get_or_init(|| {
        // Load .env file if it exists
        let _ = dotenvy::dotenv();
        
        std::env::var("SQLITE_DATABASE_URL")
            .unwrap_or_else(|_| {
                "sqlite://./test_tide_orm.db?mode=rwc".to_string()
            })
    })
}

/// Check if PostgreSQL tests should run
pub fn should_run_postgres_tests() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var("SKIP_POSTGRES_TESTS").is_err()
}

/// Check if MySQL tests should run
pub fn should_run_mysql_tests() -> bool {
    let _ = dotenvy::dotenv();
    std::env::var("RUN_MYSQL_TESTS").is_ok() || std::env::var("MYSQL_DATABASE_URL").is_ok()
}

/// Check if SQLite tests should run
pub fn should_run_sqlite_tests() -> bool {
    let _ = dotenvy::dotenv();
    // SQLite tests run by default since no external DB needed
    std::env::var("SKIP_SQLITE_TESTS").is_err()
}
