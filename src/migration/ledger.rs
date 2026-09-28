//! The tables the migrator and the seeder record their runs in.

use std::fmt;

use crate::config::DatabaseType;
use crate::database::Database;
use crate::error::Result;
use crate::internal::sql_safety::quote_ident;
use crate::internal::{
    ConnectionTrait, Value, build_statement_with_values, push_param, translate_error,
};

/// A table recording which migrations or seeds have run.
///
/// Every ledger has the same shape: a monotonic `id`, a unique key column
/// naming each entry, any further text columns, and a timestamp. Existing
/// databases carry these tables, and the CLI creates and writes the
/// migrations ledger itself, so the shape is fixed.
pub(crate) struct Ledger<'a> {
    table: &'a str,
    /// The column naming an entry: a migration's version, a seed's name.
    key_column: &'static str,
    /// Further text columns written alongside the key.
    detail_columns: &'static [&'static str],
    /// The column stamped with the time an entry was written.
    recorded_at_column: &'static str,
}

impl<'a> Ledger<'a> {
    /// The migrator's ledger. `table` must already be a safe identifier.
    pub(crate) fn migrations(table: &'a str) -> Self {
        Self {
            table,
            key_column: "version",
            detail_columns: &["name"],
            recorded_at_column: "applied_at",
        }
    }

    /// The seeder's ledger.
    pub(crate) fn seeds() -> Self {
        Self {
            table: "_seeds",
            key_column: "name",
            detail_columns: &[],
            recorded_at_column: "executed_at",
        }
    }

    pub(crate) fn table(&self) -> &'a str {
        self.table
    }

    /// Create the ledger table if it is missing.
    ///
    /// Inside a transaction this runs on it on PostgreSQL and SQLite, whose DDL
    /// is transactional: SQLite's pool may hold nothing but the transaction's
    /// connection, and waiting for a second one would never end. MySQL and
    /// MariaDB commit an open transaction before any DDL, so there a missing
    /// table is created on a pooled connection instead, after the ambient
    /// connection found it missing: a transaction holding the pool's only
    /// connection then waits for another only the first time.
    pub(crate) async fn ensure(&self, db: &Database) -> Result<()> {
        let db_type = db.execution_backend();
        let sql = self.create_table_sql(db_type);
        if matches!(db_type, DatabaseType::MySQL | DatabaseType::MariaDB) {
            let connection = db.__get_connection()?;
            let executor = connection.executor();
            let probe = build_statement_with_values(
                executor.get_database_backend(),
                "SELECT COUNT(*) > 0 FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_name = ?",
                vec![self.table.into()],
            );
            let present = match executor
                .query_one_raw(probe)
                .await
                .map_err(translate_error)?
            {
                Some(row) => crate::sync::decode_table_exists(&row, self.table)?,
                None => false,
            };
            if !present {
                db.__internal_connection()?
                    .execute_unprepared(&sql)
                    .await
                    .map_err(translate_error)?;
            }
        } else {
            db.__get_connection()?
                .executor()
                .execute_unprepared(&sql)
                .await
                .map_err(translate_error)?;
        }

        Ok(())
    }

    /// Every recorded key, in the order the entries were written, read on the
    /// ambient connection, so an entry recorded earlier in the same transaction
    /// is seen.
    pub(crate) async fn keys(&self, db: &Database) -> Result<Vec<String>> {
        let rows = db
            .fetch_rows(&self.keys_sql(db.execution_backend()), Vec::new())
            .await?;

        rows.iter()
            .map(|row| row.try_get("", self.key_column).map_err(translate_error))
            .collect()
    }

    /// Record an entry under `key`, with one value per detail column.
    ///
    /// This runs on the ambient connection, so an entry recorded inside the
    /// transaction that applied it commits or rolls back with it.
    pub(crate) async fn record(&self, db: &Database, key: &str, details: &[&str]) -> Result<()> {
        let (sql, params) = self.insert_sql(db.execution_backend(), key, details);
        db.__execute_with_params(&sql, params).await?;

        Ok(())
    }

    /// Remove the entry recorded under `key`, on the ambient connection.
    pub(crate) async fn remove(&self, db: &Database, key: &str) -> Result<()> {
        let (sql, params) = self.delete_sql(db.execution_backend(), key);
        db.__execute_with_params(&sql, params).await?;

        Ok(())
    }

    pub(super) fn create_table_sql(&self, db_type: DatabaseType) -> String {
        let (id_type, text_type, timestamp_type) = match db_type {
            DatabaseType::Postgres => ("SERIAL PRIMARY KEY", "VARCHAR(255)", "TIMESTAMP"),
            DatabaseType::MySQL | DatabaseType::MariaDB => (
                "INT AUTO_INCREMENT PRIMARY KEY",
                "VARCHAR(255)",
                "TIMESTAMP",
            ),
            DatabaseType::SQLite => ("INTEGER PRIMARY KEY AUTOINCREMENT", "TEXT", "TEXT"),
        };
        let quote = |identifier: &str| quote_ident(db_type, identifier);

        let mut columns = vec![
            format!("{} {}", quote("id"), id_type),
            format!("{} {} NOT NULL UNIQUE", quote(self.key_column), text_type),
        ];
        columns.extend(
            self.detail_columns
                .iter()
                .map(|column| format!("{} {} NOT NULL", quote(column), text_type)),
        );
        columns.push(format!(
            "{} {} NOT NULL DEFAULT CURRENT_TIMESTAMP",
            quote(self.recorded_at_column),
            timestamp_type
        ));

        format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            quote(self.table),
            columns.join(", ")
        )
    }

    /// The recorded keys in application order.
    ///
    /// Ordering is by the monotonic `id`, not by the key and not by the
    /// timestamp: a branch that merges late applies an older migration version
    /// last, and the timestamp has whole-second resolution on MySQL and is plain
    /// TEXT on SQLite. Only the insertion id says which entry really ran last,
    /// which is what a rollback has to revert.
    pub(super) fn keys_sql(&self, db_type: DatabaseType) -> String {
        format!(
            "SELECT {} FROM {} ORDER BY {} ASC",
            quote_ident(db_type, self.key_column),
            quote_ident(db_type, self.table),
            quote_ident(db_type, "id")
        )
    }

    pub(super) fn insert_sql(
        &self,
        db_type: DatabaseType,
        key: &str,
        details: &[&str],
    ) -> (String, Vec<Value>) {
        debug_assert_eq!(details.len(), self.detail_columns.len());

        let mut columns = vec![self.key_column];
        columns.extend_from_slice(self.detail_columns);

        let mut params = Vec::with_capacity(columns.len());
        let placeholders: Vec<String> = std::iter::once(key)
            .chain(details.iter().copied())
            .map(|value| push_param(db_type, &mut params, Value::String(Some(value.to_string()))))
            .collect();

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quote_ident(db_type, self.table),
            crate::internal::sql_safety::column_list(db_type, &columns, ""),
            placeholders.join(", ")
        );

        (sql, params)
    }

    pub(super) fn delete_sql(&self, db_type: DatabaseType, key: &str) -> (String, Vec<Value>) {
        let mut params = Vec::with_capacity(1);
        let placeholder = push_param(db_type, &mut params, Value::String(Some(key.to_string())));

        let sql = format!(
            "DELETE FROM {} WHERE {} = {}",
            quote_ident(db_type, self.table),
            quote_ident(db_type, self.key_column),
            placeholder
        );

        (sql, params)
    }
}

/// Write one section of a run report, or nothing when `entries` is empty.
pub(crate) fn write_report_section<T>(
    f: &mut fmt::Formatter<'_>,
    heading: &str,
    marker: &str,
    entries: &[T],
    label: impl Fn(&T) -> String,
) -> fmt::Result {
    if entries.is_empty() {
        return Ok(());
    }

    writeln!(f, "{}:", heading)?;
    for entry in entries {
        writeln!(f, "  {} {}", marker, label(entry))?;
    }

    Ok(())
}
