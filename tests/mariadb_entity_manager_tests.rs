#![cfg(all(
    feature = "mysql",
    feature = "runtime-tokio",
    feature = "entity-manager"
))]

#[path = "support/mysql_family_test_config.rs"]
mod test_config;

static SERVER: test_config::Server = test_config::Server::new("MariaDB", "MARIADB");

mod backend {
    use std::sync::Arc;

    use tideorm::Database;

    use super::SERVER;

    pub async fn connect() -> tideorm::Result<Option<Arc<Database>>> {
        if !SERVER.enabled() {
            println!("{}", SERVER.skipped("entity-manager test"));
            return Ok(None);
        }

        let db = Arc::new(Database::connect(SERVER.database_url()).await?);
        Database::set_global(db.as_ref().clone())?;
        Ok(Some(db))
    }
}

#[path = "support/entity_manager_backend_parity.rs"]
mod parity;
