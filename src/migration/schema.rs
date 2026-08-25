use super::{
    AlterTableBuilder, DatabaseType, TableBuilder, log_migration_sql, quote_identifier_for_db,
};
use crate::database::{__current_connection, ConnectionRef};
use crate::error::{Error, Result};
use crate::internal::ConnectionTrait;

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
        let mut builder = TableBuilder::new(name, self.database_type);
        build(&mut builder);
        let sql = builder.build_create();
        self.execute(&sql).await?;

        for index_sql in builder.build_indexes() {
            self.execute(&index_sql).await?;
        }

        Ok(())
    }

    /// Create a table if it doesn't exist
    pub async fn create_table_if_not_exists<F>(&mut self, name: &str, build: F) -> Result<()>
    where
        F: FnOnce(&mut TableBuilder),
    {
        let mut builder = TableBuilder::new(name, self.database_type);
        build(&mut builder);
        let sql = builder.build_create_if_not_exists();
        self.execute(&sql).await?;

        for index_sql in builder.build_indexes_if_not_exists() {
            self.execute(&index_sql).await?;
        }

        Ok(())
    }

    /// Alter an existing table
    pub async fn alter_table<F>(&mut self, name: &str, build: F) -> Result<()>
    where
        F: FnOnce(&mut AlterTableBuilder),
    {
        let mut builder = AlterTableBuilder::new(name, self.database_type);
        build(&mut builder);

        for sql in builder.build()? {
            self.execute(&sql).await?;
        }

        Ok(())
    }

    /// Drop a table
    pub async fn drop_table(&mut self, name: &str) -> Result<()> {
        let sql = format!("DROP TABLE {}", self.quote_identifier(name));
        self.execute(&sql).await
    }

    /// Drop a table if it exists
    pub async fn drop_table_if_exists(&mut self, name: &str) -> Result<()> {
        let sql = format!("DROP TABLE IF EXISTS {}", self.quote_identifier(name));
        self.execute(&sql).await
    }

    /// Rename a table
    pub async fn rename_table(&mut self, from: &str, to: &str) -> Result<()> {
        let sql = match self.database_type {
            DatabaseType::MySQL | DatabaseType::MariaDB => format!(
                "RENAME TABLE {} TO {}",
                self.quote_identifier(from),
                self.quote_identifier(to)
            ),
            _ => format!(
                "ALTER TABLE {} RENAME TO {}",
                self.quote_identifier(from),
                self.quote_identifier(to)
            ),
        };
        self.execute(&sql).await
    }

    /// Create an index
    pub async fn create_index(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
        unique: bool,
    ) -> Result<()> {
        let index_type = if unique { "UNIQUE INDEX" } else { "INDEX" };
        let columns: Vec<String> = columns
            .iter()
            .map(|column| self.quote_identifier(column))
            .collect();

        let sql = format!(
            "CREATE {} {} ON {} ({})",
            index_type,
            self.quote_identifier(name),
            self.quote_identifier(table),
            columns.join(", ")
        );
        self.execute(&sql).await
    }

    /// Drop an index
    pub async fn drop_index(&mut self, table: &str, name: &str) -> Result<()> {
        let sql = match self.database_type {
            DatabaseType::MySQL | DatabaseType::MariaDB => format!(
                "DROP INDEX {} ON {}",
                self.quote_identifier(name),
                self.quote_identifier(table)
            ),
            _ => format!("DROP INDEX {}", self.quote_identifier(name)),
        };
        self.execute(&sql).await
    }

    /// Execute raw SQL
    pub async fn raw(&mut self, sql: &str) -> Result<()> {
        self.execute(sql).await
    }

    async fn execute(&mut self, sql: &str) -> Result<()> {
        log_migration_sql(sql);

        // Resolve the connection through the ambient scope instead of the global
        // pool: on backends with transactional DDL the migrator wraps a
        // migration in a transaction, and `require_db()` would hand back a
        // pooled connection that silently runs the DDL outside it.
        let outcome = match __current_connection()? {
            ConnectionRef::Database(connection) => {
                connection.connection().execute_unprepared(sql).await
            }
            ConnectionRef::Transaction(transaction) => {
                transaction.as_ref().execute_unprepared(sql).await
            }
        };

        outcome.map_err(|error| {
            Error::query_with_context(
                error.to_string(),
                crate::error::ErrorContext::new().query(sql.to_string()),
            )
        })?;

        Ok(())
    }

    pub(crate) fn quote_identifier(&self, name: &str) -> String {
        quote_identifier_for_db(name, self.database_type)
    }

    /// Get the database type
    pub fn database_type(&self) -> DatabaseType {
        self.database_type
    }
}
