use crate::database::require_db;
use crate::error::{Error, Result};
use crate::internal::{ConnectionTrait, Value, build_statement};

use super::{
    DatabaseType, Migration, MigrationInfo, MigrationResult, MigrationStatus, Schema,
    detect_database_type, log_migration_complete, log_migration_rollback, log_migration_start,
    migration_parameter_list, migration_parameter_placeholder, quote_migration_identifier,
};

/// Migration runner
///
/// Manages and executes database migrations.
pub struct Migrator {
    migrations: Vec<Box<dyn Migration>>,
}

impl Migrator {
    /// Create a new migrator
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
        }
    }

    /// Add a migration
    #[allow(clippy::should_implement_trait)]
    pub fn add<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Box::new(migration));
        self
    }

    /// Add a boxed migration (used internally by TideConfig)
    #[doc(hidden)]
    pub fn add_boxed(mut self, migration: Box<dyn Migration>) -> Self {
        self.migrations.push(migration);
        self
    }

    /// Run all pending migrations
    pub async fn run(&self) -> Result<MigrationResult> {
        self.ensure_migrations_table().await?;

        let applied = self.get_applied_migrations().await?;
        let mut result = MigrationResult::new();

        let db = require_db()?;
        let db_type = detect_database_type(&db);

        let mut migrations: Vec<_> = self.migrations.iter().collect();
        migrations.sort_by_key(|migration| migration.version());

        for migration in migrations {
            let version = migration.version();

            if applied.contains(&version.to_string()) {
                result.skipped.push(MigrationInfo {
                    version: version.to_string(),
                    name: migration.name().to_string(),
                });
                continue;
            }

            log_migration_start(version, migration.name());

            let mut schema = Schema::new(db_type);
            migration.up(&mut schema).await?;

            self.record_migration(version, migration.name()).await?;

            result.applied.push(MigrationInfo {
                version: version.to_string(),
                name: migration.name().to_string(),
            });

            log_migration_complete(version, migration.name());
        }

        Ok(result)
    }

    /// Rollback the last migration
    pub async fn rollback(&self) -> Result<MigrationResult> {
        self.ensure_migrations_table().await?;

        let applied = self.get_applied_migrations().await?;
        let mut result = MigrationResult::new();

        if applied.is_empty() {
            return Ok(result);
        }

        let last_version = match applied.last() {
            Some(version) => version,
            None => return Ok(result),
        };

        let db = require_db()?;
        let db_type = detect_database_type(&db);

        for migration in &self.migrations {
            if migration.version() == last_version {
                log_migration_rollback(last_version, migration.name());

                let mut schema = Schema::new(db_type);
                migration.down(&mut schema).await?;

                self.remove_migration_record(last_version).await?;

                result.rolled_back.push(MigrationInfo {
                    version: migration.version().to_string(),
                    name: migration.name().to_string(),
                });

                break;
            }
        }

        Ok(result)
    }

    /// Rollback multiple migrations
    pub async fn rollback_steps(&self, steps: usize) -> Result<MigrationResult> {
        let mut result = MigrationResult::new();

        for _ in 0..steps {
            let step_result = self.rollback().await?;
            if step_result.rolled_back.is_empty() {
                break;
            }
            result.rolled_back.extend(step_result.rolled_back);
        }

        Ok(result)
    }

    /// Reset all migrations (rollback all)
    pub async fn reset(&self) -> Result<MigrationResult> {
        let applied = self.get_applied_migrations().await?;
        self.rollback_steps(applied.len()).await
    }

    /// Refresh migrations (reset + run)
    pub async fn refresh(&self) -> Result<MigrationResult> {
        let reset_result = self.reset().await?;
        let run_result = self.run().await?;

        Ok(MigrationResult {
            applied: run_result.applied,
            skipped: run_result.skipped,
            rolled_back: reset_result.rolled_back,
        })
    }

    /// Get migration status
    pub async fn status(&self) -> Result<Vec<MigrationStatus>> {
        self.ensure_migrations_table().await?;

        let applied = self.get_applied_migrations().await?;
        let mut status = Vec::new();

        let mut migrations: Vec<_> = self.migrations.iter().collect();
        migrations.sort_by_key(|migration| migration.version());

        for migration in migrations {
            let is_applied = applied.contains(&migration.version().to_string());
            status.push(MigrationStatus {
                version: migration.version().to_string(),
                name: migration.name().to_string(),
                applied: is_applied,
            });
        }

        Ok(status)
    }

    async fn ensure_migrations_table(&self) -> Result<()> {
        let db = require_db()?;
        let db_type = detect_database_type(&db);

        let sql = match db_type {
            DatabaseType::Postgres => {
                r#"
                CREATE TABLE IF NOT EXISTS "_migrations" (
                    "id" SERIAL PRIMARY KEY,
                    "version" VARCHAR(255) NOT NULL UNIQUE,
                    "name" VARCHAR(255) NOT NULL,
                    "applied_at" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                r#"
                CREATE TABLE IF NOT EXISTS `_migrations` (
                    `id` INT AUTO_INCREMENT PRIMARY KEY,
                    `version` VARCHAR(255) NOT NULL UNIQUE,
                    `name` VARCHAR(255) NOT NULL,
                    `applied_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
            DatabaseType::SQLite => {
                r#"
                CREATE TABLE IF NOT EXISTS "_migrations" (
                    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
                    "version" TEXT NOT NULL UNIQUE,
                    "name" TEXT NOT NULL,
                    "applied_at" TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
                "#
            }
        };

        db.__internal_connection()?
            .execute_unprepared(sql)
            .await
            .map_err(|error| Error::query(error.to_string()))?;

        Ok(())
    }

    async fn get_applied_migrations(&self) -> Result<Vec<String>> {
        let db = require_db()?;
        let backend = db.__internal_backend()?;
        let db_type = detect_database_type(&db);
        let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);
        let sql = format!(
            "SELECT {} FROM {} ORDER BY {} ASC",
            quote("version"),
            quote("_migrations"),
            quote("version")
        );
        let statement = build_statement(backend, sql);

        let results = db
            .__internal_connection()?
            .query_all_raw(statement)
            .await
            .map_err(|error| Error::query(error.to_string()))?;

        let registered_versions: std::collections::HashSet<_> = self
            .migrations
            .iter()
            .map(|migration| migration.version().to_string())
            .collect();

        let mut versions = Vec::new();
        for row in results {
            let version: String = row
                .try_get("", "version")
                .map_err(|error| Error::query(error.to_string()))?;
            if registered_versions.contains(&version) {
                versions.push(version);
            }
        }

        Ok(versions)
    }

    async fn record_migration(&self, version: &str, name: &str) -> Result<()> {
        let db = require_db()?;
        let db_type = detect_database_type(&db);
        let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);
        let placeholders = migration_parameter_list(db_type, 2);

        let sql = format!(
            "INSERT INTO {} ({}, {}) VALUES ({})",
            quote("_migrations"),
            quote("version"),
            quote("name"),
            placeholders
        );

        db.__execute_with_params(
            &sql,
            vec![
                Value::String(Some(version.to_string())),
                Value::String(Some(name.to_string())),
            ],
        )
        .await?;

        Ok(())
    }

    async fn remove_migration_record(&self, version: &str) -> Result<()> {
        let db = require_db()?;
        let db_type = detect_database_type(&db);
        let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);
        let placeholder = migration_parameter_placeholder(db_type, 1);

        let sql = format!(
            "DELETE FROM {} WHERE {} = {}",
            quote("_migrations"),
            quote("version"),
            placeholder
        );

        db.__execute_with_params(&sql, vec![Value::String(Some(version.to_string()))])
            .await?;

        Ok(())
    }
}

impl Default for Migrator {
    fn default() -> Self {
        Self::new()
    }
}
