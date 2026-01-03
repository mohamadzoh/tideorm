//! Test configuration utilities
//!
//! Provides helpers for loading test database configuration from .env file

use std::sync::OnceLock;

static POSTGRESQL_DATABASE_URL: OnceLock<String> = OnceLock::new();

/// Get the test database URL from environment or .env file
///
/// Loads from POSTGRESQL_DATABASE_URL environment variable, falling back to .env file
pub fn test_database_url() -> &'static str {
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
