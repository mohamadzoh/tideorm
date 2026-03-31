#![cfg(all(
    feature = "sqlite",
    feature = "runtime-tokio",
    feature = "entity-manager"
))]

#[path = "support/sqlite_test_config.rs"]
mod test_config;

mod backend {
    use std::sync::Arc;

    use tideorm::Database;

    use super::test_config::{should_run_sqlite_tests, sqlite_database_url};

    pub async fn setup_database() -> tideorm::Result<Option<Arc<Database>>> {
        if !should_run_sqlite_tests() {
            println!("⏭️  Skipping SQLite entity-manager tests (SKIP_SQLITE_TESTS is set)");
            return Ok(None);
        }

        let db = match Database::connect(sqlite_database_url()).await {
            Ok(db) => Arc::new(db),
            Err(error) => {
                println!("⏭️  Skipping SQLite entity-manager tests (connection failed: {error})");
                return Ok(None);
            }
        };
        Database::set_global(db.as_ref().clone())?;

        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_posts").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_users").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_code_posts").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_code_users").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_post_tags")
            .await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_tags").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_profiles").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_posts").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_users").await?;

        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_posts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                title TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_code_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                code TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_code_posts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_code TEXT NOT NULL,
                title TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_posts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                title TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL UNIQUE,
                bio TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_post_tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                post_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL
            )
            "#,
        )
        .await?;

        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
