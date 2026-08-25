use std::sync::Arc;

use crate::database::{Database, require_db};
use crate::error::{Error, Result};
use crate::internal::sql_safety::is_safe_identifier_segment;
use crate::internal::{
    ConnectionTrait, OrmTransaction, QueryResult, TransactionTrait, Value, build_statement,
    build_statement_with_values,
};
use crate::tide_warn;

use super::{
    DatabaseType, Migration, MigrationInfo, MigrationResult, MigrationStatus, Schema,
    detect_database_type, log_migration_complete, log_migration_rollback, log_migration_start,
    migration_parameter_list, migration_parameter_placeholder, quote_migration_identifier,
};

/// Ledger table a migrator uses unless it is pointed at another one.
///
/// The CLI exposes the same setting as `[migration] table` in `tideorm.toml`. A
/// project that renames it there must call [`Migrator::migrations_table`] with
/// the same name, or the CLI and the runtime keep two independent ledgers and
/// every migration ends up applied twice.
const DEFAULT_MIGRATIONS_TABLE: &str = "_migrations";

/// Key used for the PostgreSQL advisory lock that serializes migrators.
const MIGRATION_LOCK_KEY: i64 = 0x0054_4944_454F_524D;

/// Name used for the MySQL/MariaDB named lock that serializes migrators.
const MIGRATION_LOCK_NAME: &str = "tideorm_migrations";

/// How long a migrator waits for the MySQL/MariaDB named lock, in seconds.
///
/// `GET_LOCK` requires a timeout, unlike the PostgreSQL advisory lock which
/// simply waits. It is generous on purpose: the wait covers however long the
/// migrator that holds the lock takes to finish its own migrations.
const MIGRATION_LOCK_TIMEOUT_SECONDS: i64 = 300;

/// Whether the backend keeps DDL inside a transaction.
///
/// PostgreSQL and SQLite roll DDL back with the surrounding transaction, so a
/// migration and its ledger row can be committed together. MySQL and MariaDB
/// implicitly commit around every DDL statement, so wrapping a migration there
/// would only pretend to be atomic - they run unwrapped instead.
fn supports_transactional_ddl(db_type: DatabaseType) -> bool {
    matches!(db_type, DatabaseType::Postgres | DatabaseType::SQLite)
}

/// Read a lock-function result column as an integer, whatever width the driver
/// decoded it at.
fn lock_result_flag(row: &QueryResult) -> Option<i64> {
    if let Ok(value) = row.try_get_by_index::<i64>(0) {
        return Some(value);
    }

    if let Ok(value) = row.try_get_by_index::<i32>(0) {
        return Some(i64::from(value));
    }

    if let Ok(value) = row.try_get_by_index::<u64>(0) {
        return i64::try_from(value).ok();
    }

    None
}

/// Cross-process guard that serializes concurrent migrators.
///
/// Without it, N replicas booting together each run every pending `up()` at the
/// same time. The lock is held by a dedicated transaction so it stays pinned to
/// one pooled connection; the migrations themselves keep running on the pool.
///
/// **The pool therefore needs at least two connections.** With `max_connections(1)`
/// — reasonable for a migration-only process or a constrained test harness — the
/// lock holds the only connection and the first DDL statement blocks until
/// `acquire_timeout` elapses, then fails with a pool-exhaustion error that says
/// nothing about the lock. Size the pool accordingly.
struct MigrationLock {
    handle: Option<OrmTransaction>,
    db_type: DatabaseType,
}

impl MigrationLock {
    async fn acquire(db: &Database, db_type: DatabaseType) -> Result<Self> {
        // SQLite serializes writers itself, and its pool is frequently limited
        // to a single connection - taking a second one here would deadlock.
        if matches!(db_type, DatabaseType::SQLite) {
            return Ok(Self {
                handle: None,
                db_type,
            });
        }

        let backend = db.__internal_backend()?;
        let transaction = db
            .__internal_connection()?
            .begin()
            .await
            .map_err(|error| Error::transaction(error.to_string()))?;

        match db_type {
            DatabaseType::Postgres => {
                // Transaction-scoped, so it is released by commit, by rollback,
                // and by the connection dropping if this process dies.
                let statement = build_statement_with_values(
                    backend,
                    "SELECT pg_advisory_xact_lock($1)",
                    vec![Value::BigInt(Some(MIGRATION_LOCK_KEY))],
                );
                transaction
                    .query_one_raw(statement)
                    .await
                    .map_err(|error| Error::query(error.to_string()))?;
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                let statement = build_statement_with_values(
                    backend,
                    "SELECT GET_LOCK(?, ?)",
                    vec![
                        Value::String(Some(MIGRATION_LOCK_NAME.to_string())),
                        Value::BigInt(Some(MIGRATION_LOCK_TIMEOUT_SECONDS)),
                    ],
                );
                let acquired = transaction
                    .query_one_raw(statement)
                    .await
                    .map_err(|error| Error::query(error.to_string()))?
                    .as_ref()
                    .and_then(lock_result_flag);

                if acquired != Some(1) {
                    return Err(Error::transaction(format!(
                        "Timed out after {}s waiting for the '{}' migration lock; another migrator is still running",
                        MIGRATION_LOCK_TIMEOUT_SECONDS, MIGRATION_LOCK_NAME
                    )));
                }
            }
            DatabaseType::SQLite => {
                unreachable!("SQLite returns before opening a lock transaction")
            }
        }

        Ok(Self {
            handle: Some(transaction),
            db_type,
        })
    }

    async fn release(mut self) -> Result<()> {
        let Some(transaction) = self.handle.take() else {
            return Ok(());
        };

        // MySQL named locks are session-scoped, so committing is not enough.
        if matches!(self.db_type, DatabaseType::MySQL | DatabaseType::MariaDB) {
            let statement = build_statement_with_values(
                transaction.get_database_backend(),
                "SELECT RELEASE_LOCK(?)",
                vec![Value::String(Some(MIGRATION_LOCK_NAME.to_string()))],
            );
            transaction
                .query_one_raw(statement)
                .await
                .map_err(|error| Error::query(error.to_string()))?;
        }

        transaction
            .commit()
            .await
            .map_err(|error| Error::transaction(error.to_string()))
    }
}

/// Migration runner
///
/// Manages and executes database migrations.
pub struct Migrator {
    /// Migrations are held behind `Arc` so a single migration can be moved into
    /// the transaction closure that applies it, which must own `'static` data.
    migrations: Vec<Arc<dyn Migration>>,
    /// Name of the ledger table that records which migrations have been applied.
    table: String,
}

impl Migrator {
    /// Create a new migrator
    pub fn new() -> Self {
        Self {
            migrations: Vec::new(),
            table: DEFAULT_MIGRATIONS_TABLE.to_string(),
        }
    }

    /// Add a migration
    #[allow(clippy::should_implement_trait)]
    pub fn add<M: Migration + 'static>(mut self, migration: M) -> Self {
        self.migrations.push(Arc::new(migration));
        self
    }

    /// Add a boxed migration (used internally by TideConfig)
    #[doc(hidden)]
    pub fn add_boxed(mut self, migration: Box<dyn Migration>) -> Self {
        self.migrations.push(Arc::from(migration));
        self
    }

    /// Record applied migrations in `name` instead of the default `_migrations`.
    ///
    /// This must match the CLI's `[migration] table` setting in `tideorm.toml`.
    /// If the two disagree the CLI and the application each keep their own
    /// ledger, so migrations the CLI already applied look pending to the
    /// application and are applied a second time.
    ///
    /// The name is interpolated into DDL, so it may only contain ASCII letters,
    /// numbers, and underscores. An invalid name is reported the first time the
    /// ledger is touched, not here - a builder method has nowhere to return the
    /// error, and quietly falling back to the default would hand the caller the
    /// very second ledger this setting exists to avoid.
    pub fn migrations_table(mut self, name: impl Into<String>) -> Self {
        self.table = name.into();
        self
    }

    /// Name of the ledger table this migrator records applied migrations in.
    pub fn migrations_table_name(&self) -> &str {
        &self.table
    }

    /// Run all pending migrations
    ///
    /// Concurrent migrators are serialized with a backend lock, so replicas
    /// booting together apply each migration once instead of racing. On
    /// backends with transactional DDL the migration and its ledger row are
    /// committed together, so a statement failing halfway cannot leave an
    /// applied change with no ledger row.
    pub async fn run(&self) -> Result<MigrationResult> {
        let db = require_db()?;
        let db_type = detect_database_type(&db);

        let lock = MigrationLock::acquire(&db, db_type).await?;
        let outcome = self.run_locked(&db, db_type).await;
        let released = lock.release().await;

        let result = outcome?;
        released?;
        Ok(result)
    }

    async fn run_locked(&self, db: &Database, db_type: DatabaseType) -> Result<MigrationResult> {
        self.ensure_migrations_table().await?;

        let table = self.ledger_table()?;
        let applied = self.get_applied_migrations().await?;
        let mut result = MigrationResult::new();

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

            apply_migration(db, db_type, Arc::clone(migration), table.to_string()).await?;

            result.applied.push(MigrationInfo {
                version: version.to_string(),
                name: migration.name().to_string(),
            });

            log_migration_complete(version, migration.name());
        }

        Ok(result)
    }

    /// Rollback the last migration
    ///
    /// Takes the same lock as [`Migrator::run`], and reverts plus un-records the
    /// migration in one transaction where the backend allows it.
    pub async fn rollback(&self) -> Result<MigrationResult> {
        let db = require_db()?;
        let db_type = detect_database_type(&db);

        let lock = MigrationLock::acquire(&db, db_type).await?;
        let outcome = self.rollback_locked(&db, db_type).await;
        let released = lock.release().await;

        let result = outcome?;
        released?;
        Ok(result)
    }

    async fn rollback_locked(
        &self,
        db: &Database,
        db_type: DatabaseType,
    ) -> Result<MigrationResult> {
        self.ensure_migrations_table().await?;

        let table = self.ledger_table()?.to_string();
        // `get_applied_migrations` is ordered by the ledger's insertion id, so
        // the last entry is the migration applied most recently - not the one
        // with the highest version. After a long-lived branch merges those are
        // routinely different migrations.
        let applied = self.get_applied_migrations().await?;
        let mut result = MigrationResult::new();

        let Some(last_version) = applied.last() else {
            return Ok(result);
        };

        let Some(migration) = self
            .migrations
            .iter()
            .find(|migration| migration.version() == last_version)
        else {
            return Ok(result);
        };

        log_migration_rollback(last_version, migration.name());

        revert_migration(db, db_type, Arc::clone(migration), last_version, table).await?;

        result.rolled_back.push(MigrationInfo {
            version: migration.version().to_string(),
            name: migration.name().to_string(),
        });

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
    ///
    /// Only migrations still registered on this migrator can be reverted, since
    /// a ledger row for a version that no longer exists in code has no `down()`
    /// left to run. Those rows are reported and deliberately left in place
    /// rather than dropped, so the reset is not silently partial.
    pub async fn reset(&self) -> Result<MigrationResult> {
        self.ensure_migrations_table().await?;

        let recorded = self.recorded_versions().await?;
        let registered = self.registered_versions();
        let unknown: Vec<&str> = recorded
            .iter()
            .filter(|version| !registered.contains(*version))
            .map(String::as_str)
            .collect();

        if !unknown.is_empty() {
            tide_warn!(
                "Migration reset is leaving {} applied migration(s) recorded because they are no longer registered in code: {}. Their down() cannot be run, so the schema is not fully reset.",
                unknown.len(),
                unknown.join(", ")
            );
        }

        self.rollback_steps(recorded.len() - unknown.len()).await
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

    /// The ledger table name, checked before it reaches any SQL string.
    ///
    /// Validation lives here rather than in [`Migrator::migrations_table`]
    /// because every path that touches the ledger already returns `Result`,
    /// while the builder method has nowhere to report a bad name.
    fn ledger_table(&self) -> Result<&str> {
        if is_safe_identifier_segment(&self.table) {
            return Ok(&self.table);
        }

        Err(Error::configuration(format!(
            "invalid migrations table name '{}': expected ASCII letters, numbers, and underscores",
            self.table
        )))
    }

    async fn ensure_migrations_table(&self) -> Result<()> {
        let db = require_db()?;
        let db_type = detect_database_type(&db);
        let sql = create_ledger_table_sql(db_type, self.ledger_table()?);

        db.__internal_connection()?
            .execute_unprepared(&sql)
            .await
            .map_err(|error| Error::query(error.to_string()))?;

        Ok(())
    }

    /// Versions of the migrations registered on this migrator.
    fn registered_versions(&self) -> std::collections::HashSet<String> {
        self.migrations
            .iter()
            .map(|migration| migration.version().to_string())
            .collect()
    }

    /// Every version present in the ledger, oldest applied first.
    async fn recorded_versions(&self) -> Result<Vec<String>> {
        let db = require_db()?;
        let backend = db.__internal_backend()?;
        let db_type = detect_database_type(&db);
        let statement = build_statement(
            backend,
            recorded_versions_sql(db_type, self.ledger_table()?),
        );

        let results = db
            .__internal_connection()?
            .query_all_raw(statement)
            .await
            .map_err(|error| Error::query(error.to_string()))?;

        let mut versions = Vec::with_capacity(results.len());
        for row in results {
            let version: String = row
                .try_get("", "version")
                .map_err(|error| Error::query(error.to_string()))?;
            versions.push(version);
        }

        Ok(versions)
    }

    /// Recorded versions this migrator still knows how to revert, in the order
    /// they were applied.
    async fn get_applied_migrations(&self) -> Result<Vec<String>> {
        let registered = self.registered_versions();

        Ok(self
            .recorded_versions()
            .await?
            .into_iter()
            .filter(|version| registered.contains(version))
            .collect())
    }
}

/// DDL that creates the ledger table if it is missing.
fn create_ledger_table_sql(db_type: DatabaseType, table: &str) -> String {
    let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);
    let columns = match db_type {
        DatabaseType::Postgres => format!(
            "{} SERIAL PRIMARY KEY, {} VARCHAR(255) NOT NULL UNIQUE, {} VARCHAR(255) NOT NULL, {} TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP",
            quote("id"),
            quote("version"),
            quote("name"),
            quote("applied_at")
        ),
        DatabaseType::MySQL | DatabaseType::MariaDB => format!(
            "{} INT AUTO_INCREMENT PRIMARY KEY, {} VARCHAR(255) NOT NULL UNIQUE, {} VARCHAR(255) NOT NULL, {} TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP",
            quote("id"),
            quote("version"),
            quote("name"),
            quote("applied_at")
        ),
        DatabaseType::SQLite => format!(
            "{} INTEGER PRIMARY KEY AUTOINCREMENT, {} TEXT NOT NULL UNIQUE, {} TEXT NOT NULL, {} TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
            quote("id"),
            quote("version"),
            quote("name"),
            quote("applied_at")
        ),
    };

    format!("CREATE TABLE IF NOT EXISTS {} ({})", quote(table), columns)
}

/// Query that lists the ledger in application order.
///
/// Ordering is by the monotonic `id`, not by `version` and not by `applied_at`:
/// a branch that merges late applies an older version last, and `applied_at`
/// has whole-second resolution on MySQL and is plain TEXT on SQLite. Only the
/// insertion id says which migration really ran last, which is what a rollback
/// has to revert.
fn recorded_versions_sql(db_type: DatabaseType, table: &str) -> String {
    let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);

    format!(
        "SELECT {} FROM {} ORDER BY {} ASC",
        quote("version"),
        quote(table),
        quote("id")
    )
}

/// Statement that records one applied migration.
fn insert_ledger_row_sql(db_type: DatabaseType, table: &str) -> String {
    let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);

    format!(
        "INSERT INTO {} ({}, {}) VALUES ({})",
        quote(table),
        quote("version"),
        quote("name"),
        migration_parameter_list(db_type, 2)
    )
}

/// Statement that un-records one migration.
fn delete_ledger_row_sql(db_type: DatabaseType, table: &str) -> String {
    let quote = |identifier: &str| quote_migration_identifier(identifier, db_type);

    format!(
        "DELETE FROM {} WHERE {} = {}",
        quote(table),
        quote("version"),
        migration_parameter_placeholder(db_type, 1)
    )
}

/// Apply one migration and record it in the ledger.
///
/// On a backend with transactional DDL both happen in one transaction, so a
/// statement failing partway cannot leave schema changes behind with no ledger
/// row. `Schema` resolves its connection from the ambient scope, which is what
/// puts the DDL inside that transaction. Elsewhere the two run unwrapped,
/// because the backend would implicitly commit the DDL anyway.
async fn apply_migration(
    db: &Database,
    db_type: DatabaseType,
    migration: Arc<dyn Migration>,
    table: String,
) -> Result<()> {
    let version = migration.version().to_string();
    let name = migration.name().to_string();

    if supports_transactional_ddl(db_type) {
        return db
            .transaction(move |_| {
                Box::pin(async move {
                    let mut schema = Schema::new(db_type);
                    migration.up(&mut schema).await?;
                    record_migration(&table, &version, &name).await
                })
            })
            .await;
    }

    let mut schema = Schema::new(db_type);
    migration.up(&mut schema).await?;
    record_migration(&table, &version, &name).await
}

/// Revert one migration and remove its ledger row, with the same transaction
/// rules as [`apply_migration`].
async fn revert_migration(
    db: &Database,
    db_type: DatabaseType,
    migration: Arc<dyn Migration>,
    version: &str,
    table: String,
) -> Result<()> {
    let version = version.to_string();

    if supports_transactional_ddl(db_type) {
        return db
            .transaction(move |_| {
                Box::pin(async move {
                    let mut schema = Schema::new(db_type);
                    migration.down(&mut schema).await?;
                    remove_migration_record(&table, &version).await
                })
            })
            .await;
    }

    let mut schema = Schema::new(db_type);
    migration.down(&mut schema).await?;
    remove_migration_record(&table, &version).await
}

async fn record_migration(table: &str, version: &str, name: &str) -> Result<()> {
    let db = require_db()?;
    let sql = insert_ledger_row_sql(detect_database_type(&db), table);

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

async fn remove_migration_record(table: &str, version: &str) -> Result<()> {
    let db = require_db()?;
    let sql = delete_ledger_row_sql(detect_database_type(&db), table);

    db.__execute_with_params(&sql, vec![Value::String(Some(version.to_string()))])
        .await?;

    Ok(())
}

impl Default for Migrator {
    fn default() -> Self {
        Self::new()
    }
}

// These cover the ledger SQL, which is private to this module, so they live
// here rather than in `tests/unit/migration_tests.rs`.
#[cfg(test)]
mod ledger_tests {
    use super::*;

    const BACKENDS: [DatabaseType; 4] = [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::MariaDB,
        DatabaseType::SQLite,
    ];

    #[test]
    fn ledger_table_defaults_to_underscore_migrations() {
        assert_eq!(Migrator::new().migrations_table_name(), "_migrations");
        assert_eq!(Migrator::default().migrations_table_name(), "_migrations");
    }

    #[test]
    fn ledger_table_is_configurable() {
        let migrator = Migrator::new().migrations_table("schema_migrations");

        assert_eq!(migrator.migrations_table_name(), "schema_migrations");
        assert_eq!(
            migrator.ledger_table().expect("valid table name"),
            "schema_migrations"
        );
    }

    #[test]
    fn ledger_sql_uses_the_configured_table_on_every_backend() {
        for db_type in BACKENDS {
            let quoted = quote_migration_identifier("schema_migrations", db_type);

            for sql in [
                create_ledger_table_sql(db_type, "schema_migrations"),
                recorded_versions_sql(db_type, "schema_migrations"),
                insert_ledger_row_sql(db_type, "schema_migrations"),
                delete_ledger_row_sql(db_type, "schema_migrations"),
            ] {
                assert!(
                    sql.contains(&quoted),
                    "{:?} statement should target the configured ledger. Got: {}",
                    db_type,
                    sql
                );
                assert!(
                    !sql.contains(&quote_migration_identifier(
                        DEFAULT_MIGRATIONS_TABLE,
                        db_type
                    )),
                    "{:?} statement should not fall back to the default ledger. Got: {}",
                    db_type,
                    sql
                );
            }
        }
    }

    #[test]
    fn ledger_table_name_is_validated_before_it_reaches_sql() {
        for name in ["schema migrations", "users\"; DROP TABLE users --", "1bad"] {
            let error = Migrator::new()
                .migrations_table(name)
                .ledger_table()
                .expect_err("unsafe ledger table names must be rejected");

            assert!(
                error.to_string().contains(name),
                "Error should name the offending table. Got: {}",
                error
            );
        }
    }

    #[test]
    fn recorded_versions_are_ordered_by_insertion_id() {
        for db_type in BACKENDS {
            let sql = recorded_versions_sql(db_type, "_migrations");
            let id = quote_migration_identifier("id", db_type);
            let version = quote_migration_identifier("version", db_type);

            assert!(
                sql.ends_with(&format!("ORDER BY {} ASC", id)),
                "Rollback order must follow the ledger id, not the version. Got: {}",
                sql
            );
            assert!(
                !sql.contains(&format!("ORDER BY {}", version)),
                "Ordering by version reverts the wrong migration after a late merge. Got: {}",
                sql
            );
        }
    }

    #[test]
    fn ledger_table_ddl_keeps_the_monotonic_id_column() {
        for db_type in BACKENDS {
            let sql = create_ledger_table_sql(db_type, "_migrations");

            assert!(
                sql.contains(&quote_migration_identifier("id", db_type)),
                "{:?} ledger needs the id rollback ordering depends on. Got: {}",
                db_type,
                sql
            );
            assert!(
                sql.contains(&quote_migration_identifier("applied_at", db_type)),
                "{:?} ledger should keep applied_at. Got: {}",
                db_type,
                sql
            );
        }
    }
}
