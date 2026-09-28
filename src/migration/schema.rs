use super::{AlterTableBuilder, DatabaseType, TableBuilder, ddl};
use crate::error::{ErrorContext, Result};
use crate::internal::sql_safety::quote_ident;

/// Schema manipulation context for migrations
///
/// Provides methods to create, alter, and drop database objects.
pub struct Schema {
    database_type: DatabaseType,
}

impl Schema {
    /// Create a new schema context
    pub fn new(database_type: DatabaseType) -> Self {
        Self { database_type }
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
        let db_type = self.database_type;
        let sql = match db_type {
            DatabaseType::MySQL | DatabaseType::MariaDB => format!(
                "DROP INDEX {} ON {}",
                quote_ident(db_type, name),
                quote_ident(db_type, table)
            ),
            _ => format!("DROP INDEX {}", quote_ident(db_type, name)),
        };
        self.execute(&sql).await
    }

    /// Execute raw SQL
    pub async fn raw(&mut self, sql: &str) -> Result<()> {
        self.execute(sql).await
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
