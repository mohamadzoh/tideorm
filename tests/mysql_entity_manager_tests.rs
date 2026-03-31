#![cfg(all(feature = "mysql", feature = "runtime-tokio", feature = "entity-manager"))]

#[path = "support/mysql_test_config.rs"]
mod test_config;

mod backend {
    use std::sync::Arc;

    use tideorm::Database;

    use super::test_config::{mysql_database_url, should_run_mysql_tests};

    pub async fn setup_database() -> tideorm::Result<Option<Arc<Database>>> {
        if !should_run_mysql_tests() {
            println!(
                "⏭️  Skipping MySQL entity-manager tests (MYSQL_DATABASE_URL not set or SKIP_MYSQL_TESTS is set)"
            );
            return Ok(None);
        }

        let db = match Database::connect(mysql_database_url()).await {
            Ok(db) => Arc::new(db),
            Err(error) => {
                println!("⏭️  Skipping MySQL entity-manager tests (connection failed: {error})");
                return Ok(None);
            }
        };
        Database::set_global(db.as_ref().clone())?;

        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_posts`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_users`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_code_posts`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_code_users`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_aggregate_post_tags`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_aggregate_tags`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_aggregate_profiles`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_aggregate_posts`").await?;
        Database::execute("DROP TABLE IF EXISTS `entity_manager_backend_aggregate_users`").await?;

        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_users` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `name` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_posts` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `user_id` BIGINT NOT NULL,
                `title` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_code_users` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `code` VARCHAR(255) NOT NULL UNIQUE,
                `name` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_code_posts` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `user_code` VARCHAR(255) NOT NULL,
                `title` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_aggregate_users` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `name` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_aggregate_posts` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `user_id` BIGINT NOT NULL,
                `title` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_aggregate_profiles` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `user_id` BIGINT NOT NULL UNIQUE,
                `bio` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_aggregate_tags` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `name` VARCHAR(255) NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;
        Database::execute(
            r#"
            CREATE TABLE `entity_manager_backend_aggregate_post_tags` (
                `id` BIGINT AUTO_INCREMENT PRIMARY KEY,
                `post_id` BIGINT NOT NULL,
                `tag_id` BIGINT NOT NULL
            ) ENGINE=InnoDB
            "#,
        )
        .await?;

        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;