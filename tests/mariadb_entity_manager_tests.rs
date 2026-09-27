#![cfg(all(
    feature = "mysql",
    feature = "runtime-tokio",
    feature = "entity-manager"
))]

#[path = "support/mariadb_test_config.rs"]
mod test_config;

mod backend {
    use std::sync::Arc;

    use tideorm::Database;

    use super::test_config::{mariadb_database_url, should_run_mariadb_tests};

    pub async fn connect() -> tideorm::Result<Option<Arc<Database>>> {
        if !should_run_mariadb_tests() {
            println!(
                "Skipping MariaDB entity-manager test: set RUN_MARIADB_TESTS or MARIADB_DATABASE_URL (SKIP_MARIADB_TESTS overrides both)"
            );
            return Ok(None);
        }

        let db = Arc::new(Database::connect(mariadb_database_url()).await?);
        Database::set_global(db.as_ref().clone())?;
        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
