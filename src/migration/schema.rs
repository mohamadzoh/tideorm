use super::{AlterTableBuilder, DatabaseType, TableBuilder, ddl};
use crate::error::{ErrorContext, Result};
use crate::internal::sql_safety::quote_ident;

/// Schema manipulation context for migrations
///
/// Provides methods to create, alter, and drop database objects.
pub struct Schema {
    database_type: DatabaseType,
    /// Whether the migration runs with SQLite's foreign keys off (see
    /// [`Migration::foreign_keys_off`](super::Migration::foreign_keys_off)).
    foreign_keys_off: bool,
}

impl Schema {
    /// Create a new schema context
    pub fn new(database_type: DatabaseType) -> Self {
        Self {
            database_type,
            foreign_keys_off: false,
        }
    }

    /// This context, for a migration that runs with SQLite's foreign keys off.
    pub(crate) fn with_foreign_keys_off(mut self, off: bool) -> Self {
        self.foreign_keys_off = off;
        self
    }

    /// Create a new table
    pub async fn create_table<F>(&mut self, name: &str, build: F) -> Result<()>
    where
        F: FnOnce(&mut TableBuilder),
    {
        self.create(name, false, build).await
    }

    /// Create a table if it doesn't exist
    pub async fn create_table_if_not_exists<F>(&mut self, name: &str, build: F) -> Result<()>
    where
        F: FnOnce(&mut TableBuilder),
    {
        self.create(name, true, build).await
    }

    async fn create<F>(&mut self, name: &str, if_not_exists: bool, build: F) -> Result<()>
    where
        F: FnOnce(&mut TableBuilder),
    {
        let mut builder = TableBuilder::new(name, self.database_type);
        build(&mut builder);
        self.execute(&builder.build_create(if_not_exists)).await?;

        for (index, index_sql) in builder.build_indexes(if_not_exists) {
            // MySQL has no `CREATE INDEX IF NOT EXISTS`, so a re-run, after a
            // migration its non-transactional DDL left half applied, asks the
            // catalog instead of failing on the index it already made.
            if if_not_exists
                && self.database_type == DatabaseType::MySQL
                && self.mysql_index_exists(name, index).await?
            {
                continue;
            }
            self.execute(&index_sql).await?;
        }

        Ok(())
    }

    /// Whether the current MySQL database's `table` has an index named `index`.
    async fn mysql_index_exists(&self, table: &str, index: &str) -> Result<bool> {
        let connection = crate::database::__current_db()?.__get_connection()?;
        super::ddl::mysql_index_exists(&connection.executor(), None, table, index).await
    }

    /// Alter an existing table
    pub async fn alter_table<F>(&mut self, name: &str, build: F) -> Result<()>
    where
        F: FnOnce(&mut AlterTableBuilder),
    {
        let mut builder = AlterTableBuilder::new(name, self.database_type);
        build(&mut builder);
        if builder.changes_mysql_column_types() {
            let create_table = self.show_create_table(name).await?;
            builder.keep_column_attributes(&create_table);
        }
        let dropped = builder.sqlite_dropped_columns();
        if self.database_type == DatabaseType::SQLite {
            let (schema, indexes) = self.sqlite_column_indexes(name, &dropped).await?;
            builder.drop_sqlite_column_indexes(schema, indexes);
        }

        for sql in builder.build()? {
            self.execute(&sql).await?;
        }

        Ok(())
    }

    /// Drop a table
    pub async fn drop_table(&mut self, name: &str) -> Result<()> {
        let sql = format!("DROP TABLE {}", quote_ident(self.database_type, name));
        self.execute(&sql).await
    }

    /// Drop a table if it exists
    pub async fn drop_table_if_exists(&mut self, name: &str) -> Result<()> {
        let sql = format!(
            "DROP TABLE IF EXISTS {}",
            quote_ident(self.database_type, name)
        );
        self.execute(&sql).await
    }

    /// Rename a table
    ///
    /// Its rows, columns and indexes move with it. Foreign keys in other
    /// tables follow the rename on every backend, including SQLite 3.26 and
    /// later.
    pub async fn rename_table(&mut self, from: &str, to: &str) -> Result<()> {
        self.execute(&ddl::rename_table(self.database_type, from, to))
            .await
    }

    /// Create an index
    pub async fn create_index(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
        unique: bool,
    ) -> Result<()> {
        let sql = ddl::create_index(
            self.database_type,
            name,
            &quote_ident(self.database_type, table),
            columns,
            unique,
            false,
        );
        self.execute(&sql).await
    }

    /// Drop an index
    pub async fn drop_index(&mut self, table: &str, name: &str) -> Result<()> {
        self.execute(&ddl::drop_index(
            self.database_type,
            None,
            table,
            name,
            false,
        ))
        .await
    }

    /// Execute raw SQL
    ///
    /// On SQLite, `PRAGMA foreign_keys` does nothing inside the transaction a
    /// migration runs in, and outside one it changes whichever pooled
    /// connection runs it, so turning foreign keys off is refused: see
    /// [`Migration::foreign_keys_off`](super::Migration::foreign_keys_off),
    /// whose migration runs with them off. SQL scripts are checked before any
    /// of their statements execute.
    pub async fn raw(&mut self, sql: &str) -> Result<()> {
        if self.database_type == DatabaseType::SQLite
            && !self.foreign_keys_off
            && super::sqlite_foreign_keys::disables_foreign_keys(sql)
        {
            return Err(crate::Error::query(
                "SQLite ignores `PRAGMA foreign_keys` inside a transaction, which a migration \
                 runs in, and outside one it turns them off on one connection of a pool only: \
                 return true from `Migration::foreign_keys_off()` to run a migration with \
                 foreign keys off",
            ));
        }
        self.execute(sql).await
    }

    /// Explicit indexes over just a dropped column. Inspect column membership
    /// rather than guessing names: SQLite keeps index names after renames.
    async fn sqlite_column_indexes(
        &self,
        table: &str,
        dropped: &[&str],
    ) -> Result<(String, Vec<(String, String)>)> {
        let db = crate::database::__current_db()?;
        // Match SQLite's unqualified table resolution: temp, main, then
        // attached databases in attachment order. Index names resolve separately.
        const OWNER_SQL: &str = "SELECT t.schema AS schema_name FROM pragma_table_list AS t \
            JOIN pragma_database_list AS d ON d.name = t.schema \
            WHERE t.name = ? COLLATE NOCASE \
            ORDER BY CASE t.schema WHEN 'temp' THEN -1 ELSE d.seq END LIMIT 1";
        let owners = db
            .__raw_json_with_params(OWNER_SQL, vec![table.into()])
            .await?;
        let schema = owners
            .first()
            .and_then(|row| row["schema_name"].as_str())
            .unwrap_or("main")
            .to_string();
        const SQL: &str = "SELECT list.name FROM pragma_index_list(?, ?) AS list \
                           WHERE list.origin = 'c' AND list.partial = 0 \
                           AND (SELECT COUNT(*) FROM pragma_index_info(list.name, ?)) = 1 \
                           AND (SELECT name FROM pragma_index_info(list.name, ?)) = ?";
        let mut found = Vec::new();
        for column in dropped {
            let rows = db
                .__raw_json_with_params(
                    SQL,
                    vec![
                        table.into(),
                        schema.as_str().into(),
                        schema.as_str().into(),
                        schema.as_str().into(),
                        (*column).into(),
                    ],
                )
                .await
                .map_err(|error| error.with_context(ErrorContext::new().query(SQL)))?;
            for row in rows {
                if let Some(index) = row.get("name").and_then(serde_json::Value::as_str) {
                    found.push(((*column).to_string(), index.to_string()));
                }
            }
        }
        Ok((schema, found))
    }

    /// The table's `SHOW CREATE TABLE`, on MySQL or MariaDB.
    async fn show_create_table(&self, name: &str) -> Result<String> {
        let sql = format!(
            "SHOW CREATE TABLE {}",
            quote_ident(self.database_type, name)
        );
        let rows = crate::database::__current_db()?
            .__raw_json_with_params(&sql, Vec::new())
            .await
            .map_err(|error| error.with_context(ErrorContext::new().query(&sql)))?;
        rows.first()
            .and_then(|row| row.get("Create Table"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| crate::Error::query(format!("{} returned no table definition", sql)))
    }

    /// Run `sql` on the ambient connection rather than the global pool: on
    /// backends with transactional DDL the migrator wraps each migration in a
    /// transaction, and a pooled connection would run the DDL outside it.
    ///
    /// The engine logs the statement once it has run, to `TIDE_LOG_QUERIES`
    /// and the [`QueryLogger`](crate::logging::QueryLogger), as it logs raw SQL.
    async fn execute(&mut self, sql: &str) -> Result<()> {
        let db = crate::database::__current_db()?;
        db.exec_raw(sql)
            .await
            .map_err(|error| error.with_context(ErrorContext::new().query(sql)))?;

        Ok(())
    }
}
