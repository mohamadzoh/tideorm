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

    pub async fn connect() -> tideorm::Result<Option<Arc<Database>>> {
        if !super::test_config::should_run_postgres_tests() {
            println!("{}", super::test_config::SKIPPED);
            return Ok(None);
        }

        let db = Arc::new(Database::connect(test_database_url()).await?);
        Database::set_global(db.as_ref().clone())?;
        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
