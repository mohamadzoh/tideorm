#![cfg(all(
    feature = "mysql",
    feature = "runtime-tokio",
    feature = "entity-manager"
))]

#[path = "support/mysql_test_config.rs"]
mod test_config;

mod backend {
    use std::sync::Arc;

    use tideorm::Database;

    use super::test_config::{mysql_database_url, should_run_mysql_tests};

    pub async fn connect() -> tideorm::Result<Option<Arc<Database>>> {
        if !should_run_mysql_tests() {
            println!(
                "Skipping MySQL entity-manager test: set RUN_MYSQL_TESTS or MYSQL_DATABASE_URL (SKIP_MYSQL_TESTS overrides both)"
            );
            return Ok(None);
        }

        let db = Arc::new(Database::connect(mysql_database_url()).await?);
        Database::set_global(db.as_ref().clone())?;
        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
