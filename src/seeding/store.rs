use super::*;
use crate::internal::push_param;

impl Seeder {
    /// Ensure the seeds table exists
    pub(super) async fn ensure_seeds_table(&self) -> Result<()> {
        let database = require_db()?;
        let db_type = detect_database_type(&database);

        let sql = match db_type {
            DatabaseType::Postgres => {
                r#"
                CREATE TABLE IF NOT EXISTS "_seeds" (
                    "id" SERIAL PRIMARY KEY,
                    "name" VARCHAR(255) NOT NULL UNIQUE,
                    "executed_at" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                r#"
                CREATE TABLE IF NOT EXISTS `_seeds` (
                    `id` INT AUTO_INCREMENT PRIMARY KEY,
                    `name` VARCHAR(255) NOT NULL UNIQUE,
                    `executed_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
            DatabaseType::SQLite => {
                r#"
                CREATE TABLE IF NOT EXISTS "_seeds" (
                    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                    "name" TEXT NOT NULL UNIQUE,
                    "executed_at" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
        };

        database
            .__internal_connection()?
            .execute_unprepared(sql)
            .await
            .map_err(|error| Error::query(error.to_string()))?;

        Ok(())
    }

    /// Get list of executed seed names
    pub(super) async fn get_executed_seeds(&self) -> Result<Vec<String>> {
        let database = require_db()?;

        use crate::internal::build_statement;

        let backend = crate::internal::Backend::from(
            database.__internal_connection()?.get_database_backend(),
        );
        let quote = |identifier: &str| quote_ident_for_backend(backend, identifier);
        // Order by the monotonic `id` rather than `executed_at`: the timestamp has
        // whole-second resolution on MySQL and is plain TEXT on SQLite, so ties would
        // roll back in an arbitrary (potentially FK-violating) order.
        let sql = format!(
            "SELECT {} FROM {} ORDER BY {} ASC",
            quote("name"),
            quote("_seeds"),
            quote("id")
        );
        let statement = build_statement(backend, sql);

        let results = database
            .__internal_connection()?
            .query_all_raw(statement)
            .await
            .map_err(|error| Error::query(error.to_string()))?;

        let mut names = Vec::new();
        for row in results {
            let name: String = row
                .try_get("", "name")
                .map_err(|error| Error::query(error.to_string()))?;
            names.push(name);
        }

        Ok(names)
    }

    /// Record a seed as executed
    pub(super) async fn record_seed(&self, name: &str) -> Result<()> {
        let database = require_db()?;
        let backend = crate::internal::Backend::from(
            database.__internal_connection()?.get_database_backend(),
        );
        let quote = |identifier: &str| quote_ident_for_backend(backend, identifier);

        let mut params = Vec::new();
        let placeholder = push_param(
            detect_database_type(&database),
            &mut params,
            Value::String(Some(name.to_string())),
        );

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote("_seeds"),
            quote("name"),
            placeholder
        );

        database.__execute_with_params(&sql, params).await?;

        Ok(())
    }

    /// Remove a seed record
    pub(super) async fn remove_seed_record(&self, name: &str) -> Result<()> {
        let database = require_db()?;
        let backend = crate::internal::Backend::from(
            database.__internal_connection()?.get_database_backend(),
        );
        let quote = |identifier: &str| quote_ident_for_backend(backend, identifier);

        let mut params = Vec::new();
        let placeholder = push_param(
            detect_database_type(&database),
            &mut params,
            Value::String(Some(name.to_string())),
        );

        let sql = format!(
            "DELETE FROM {} WHERE {} = {}",
            quote("_seeds"),
            quote("name"),
            placeholder
        );

        database.__execute_with_params(&sql, params).await?;

        Ok(())
    }
}

fn detect_database_type(database: &Database) -> DatabaseType {
    database.backend()
}
