#![cfg(all(
    feature = "postgres",
    feature = "runtime-tokio",
    feature = "entity-manager"
))]

#[path = "support/postgres_test_config.rs"]
mod test_config;

mod backend {
    use std::sync::Arc;

    use tideorm::Database;

    use super::test_config::test_database_url;

    pub async fn setup_database() -> tideorm::Result<Option<Arc<Database>>> {
        if std::env::var_os("SKIP_POSTGRES_TESTS").is_some() {
            println!("Skipping Postgres entity-manager tests (SKIP_POSTGRES_TESTS is set)");
            return Ok(None);
        }

        let db = match Database::connect(test_database_url()).await {
            Ok(db) => Arc::new(db),
            Err(error) => {
                println!("Skipping Postgres entity-manager tests (connection failed: {error})");
                return Ok(None);
            }
        };
        Database::set_global(db.as_ref().clone())?;

        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_posts CASCADE").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_users CASCADE").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_code_posts CASCADE").await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_code_users CASCADE").await?;
        Database::execute(
            "DROP TABLE IF EXISTS entity_manager_backend_aggregate_post_tags CASCADE",
        )
        .await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_tags CASCADE")
            .await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_profiles CASCADE")
            .await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_posts CASCADE")
            .await?;
        Database::execute("DROP TABLE IF EXISTS entity_manager_backend_aggregate_users CASCADE")
            .await?;

        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_users (
                id BIGSERIAL PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_posts (
                id BIGSERIAL PRIMARY KEY,
                user_id BIGINT NOT NULL,
                title VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_code_users (
                id BIGSERIAL PRIMARY KEY,
                code VARCHAR(255) NOT NULL UNIQUE,
                name VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_code_posts (
                id BIGSERIAL PRIMARY KEY,
                user_code VARCHAR(255) NOT NULL,
                title VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_users (
                id BIGSERIAL PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_posts (
                id BIGSERIAL PRIMARY KEY,
                user_id BIGINT NOT NULL,
                title VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_profiles (
                id BIGSERIAL PRIMARY KEY,
                user_id BIGINT NOT NULL UNIQUE,
                bio VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_tags (
                id BIGSERIAL PRIMARY KEY,
                name VARCHAR(255) NOT NULL
            )
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE entity_manager_backend_aggregate_post_tags (
                id BIGSERIAL PRIMARY KEY,
                post_id BIGINT NOT NULL,
                tag_id BIGINT NOT NULL
            )
            "#,
        )
        .await?;

        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
