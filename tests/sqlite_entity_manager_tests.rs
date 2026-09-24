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

    pub async fn connect() -> tideorm::Result<Option<Arc<Database>>> {
        if !should_run_sqlite_tests() {
            println!("Skipping SQLite entity-manager test (SKIP_SQLITE_TESTS is set)");
            return Ok(None);
        }

        let db = Arc::new(Database::connect(sqlite_database_url()).await?);
        Database::set_global(db.as_ref().clone())?;
        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
