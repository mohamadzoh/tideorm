use super::{AlterTableBuilder, DatabaseType, TableBuilder, ddl};
use crate::error::{ErrorContext, Result};
use crate::internal::sql_safety::quote_ident;
use crate::tide_debug;

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

        for index_sql in builder.build_indexes(if_not_exists) {
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

    /// Run `sql` on the ambient connection rather than the global pool: on
    /// backends with transactional DDL the migrator wraps each migration in a
    /// transaction, and a pooled connection would run the DDL outside it.
    async fn execute(&mut self, sql: &str) -> Result<()> {
        if crate::logging::query_logging_enabled() {
            tide_debug!("Migration SQL: {}", sql);
        }

        let db = crate::database::__current_db()?;
        crate::logging::logged_by_caller(db.exec_raw(sql))
            .await
            .map_err(|error| error.with_context(ErrorContext::new().query(sql)))?;

        Ok(())
    }
}
