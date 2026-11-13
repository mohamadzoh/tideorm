use super::{AlterTableBuilder, DatabaseType, TableBuilder, ddl};
use crate::error::{ErrorContext, Result};
use crate::internal::ConnectionTrait;

/// Schema manipulation context for migrations
///
/// Provides methods to create, alter, and drop database objects. A table
/// may be named `schema.table`: PostgreSQL's schema, or MySQL's and
/// MariaDB's database. SQLite keeps the tables in its main database, as
/// schema sync does, and ignores the schema part. Index and foreign key
/// names derive from the table alone.
pub struct Schema {
    database_type: DatabaseType,
    /// Whether the migration runs with SQLite's foreign keys off (see
    /// [`Migration::foreign_keys_off`](super::Migration::foreign_keys_off)).
    foreign_keys_off: bool,
    /// The statements a pretended migration would run, which `execute`
    /// collects instead of running them.
    pretended: Option<Vec<String>>,
}

impl Schema {
    /// Create a new schema context
    pub fn new(database_type: DatabaseType) -> Self {
        Self {
            database_type,
            foreign_keys_off: false,
            pretended: None,
        }
    }

    /// A context that collects its statements instead of running them, for
    /// [`Migrator::pretend`](super::Migrator::pretend).
    pub(super) fn pretending(mut self) -> Self {
        self.pretended = Some(Vec::new());
        self
    }

    /// The statements a [`pretending`](Self::pretending) context collected.
    pub(super) fn into_pretended(self) -> Vec<String> {
        self.pretended.unwrap_or_default()
    }

    /// The backend the migration runs on, MariaDB told apart from MySQL, for
    /// a migration whose SQL differs by backend.
    pub fn database_type(&self) -> DatabaseType {
        self.database_type
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

    /// Whether the MySQL `table` has an index named `index`.
    async fn mysql_index_exists(&self, table: &str, index: &str) -> Result<bool> {
        let connection = crate::database::__current_db()?.__get_connection()?;
        let (schema, table) = ddl::table_parts(self.database_type, table);
        crate::schema::catalog::mysql_index_exists(&connection.executor(), schema, table, index)
            .await
    }

    /// Whether the table `name` exists: in the schema `name` names, else in
    /// the connection's current schema on PostgreSQL, its database on MySQL
    /// and MariaDB, and the main database on SQLite. A migration that runs
    /// in a transaction sees the tables it has created.
    pub async fn has_table(&self, name: &str) -> Result<bool> {
        let connection = crate::database::__current_db()?.__get_connection()?;
        let executor = connection.executor();
        let backend = executor.get_database_backend();
        let (schema, table) = ddl::table_parts(self.database_type, name);
        crate::schema::catalog::table_exists(&executor, backend, schema, table).await
    }

    /// Whether the table `table` has the column `column`, looked for where
    /// [`has_table`](Self::has_table) looks for the table. PostgreSQL tells
    /// column names apart by case; MySQL, MariaDB and SQLite do not.
    pub async fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let connection = crate::database::__current_db()?.__get_connection()?;
        let executor = connection.executor();
        let backend = executor.get_database_backend();
        let (schema, table) = ddl::table_parts(self.database_type, table);
        let columns =
            crate::schema::catalog::column_names(&executor, backend, schema, table).await?;
        Ok(columns.iter().any(|name| match backend {
            DatabaseType::Postgres => name == column,
            DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                name.eq_ignore_ascii_case(column)
            }
        }))
    }

    /// Alter an existing table
    pub async fn alter_table<F>(&mut self, name: &str, build: F) -> Result<()>
    where
        F: FnOnce(&mut AlterTableBuilder),
    {
        let mut builder = AlterTableBuilder::new(name, self.database_type);
        build(&mut builder);
        if builder.restates_mysql_columns() {
            let create_table = self.show_create_table(name).await?;
            builder.keep_column_attributes(&create_table);
        }
        let dropped = builder.sqlite_dropped_columns();
        if self.database_type == DatabaseType::SQLite {
            let table = ddl::table_parts(self.database_type, name).1;
            let (schema, indexes) = self.sqlite_column_indexes(table, &dropped).await?;
            builder.drop_sqlite_column_indexes(schema, indexes);
        }

        for sql in builder.build()? {
            self.execute(&sql).await?;
        }

        Ok(())
    }

    /// Drop a table
    pub async fn drop_table(&mut self, name: &str) -> Result<()> {
        let sql = format!("DROP TABLE {}", ddl::quote_table(self.database_type, name));
        self.execute(&sql).await
    }

    /// Drop a table if it exists
    pub async fn drop_table_if_exists(&mut self, name: &str) -> Result<()> {
        let sql = format!(
            "DROP TABLE IF EXISTS {}",
            ddl::quote_table(self.database_type, name)
        );
        self.execute(&sql).await
    }

    /// Rename a table
    ///
    /// Its rows, columns and indexes move with it. Foreign keys in other
    /// tables follow the rename on every backend, including SQLite 3.26 and
    /// later. MySQL and MariaDB move a table to the database `to` names
    /// (`other.name`), and keep it in its own for an unqualified `to`;
    /// PostgreSQL renames a table within its schema only.
    pub async fn rename_table(&mut self, from: &str, to: &str) -> Result<()> {
        if self.database_type == DatabaseType::Postgres
            && let (Some(to_schema), _) = ddl::table_parts(self.database_type, to)
            && ddl::table_parts(self.database_type, from).0 != Some(to_schema)
        {
            return Err(crate::Error::query(format!(
                "PostgreSQL renames table '{from}' within its schema, which '{to}' leaves: \
                 move a table to another schema with `ALTER TABLE .. SET SCHEMA` through `raw`"
            )));
        }
        self.execute(&ddl::rename_table(self.database_type, from, to))
            .await
    }

    /// Index `columns` of `table` under `name`
    pub async fn create_index(&mut self, table: &str, name: &str, columns: &[&str]) -> Result<()> {
        self.add_index(table, name, columns, false, false).await
    }

    /// A unique index on `columns` of `table`, named `name`
    pub async fn create_unique_index(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
    ) -> Result<()> {
        self.add_index(table, name, columns, true, false).await
    }

    /// Index `columns` of `table` under `name` unless `table` has an index of
    /// that name. MySQL has no `CREATE INDEX IF NOT EXISTS`, so there the
    /// catalog is asked first.
    pub async fn create_index_if_not_exists(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
    ) -> Result<()> {
        self.add_index(table, name, columns, false, true).await
    }

    /// A unique index on `columns` of `table`, named `name`, unless `table`
    /// has an index of that name.
    pub async fn create_unique_index_if_not_exists(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
    ) -> Result<()> {
        self.add_index(table, name, columns, true, true).await
    }

    async fn add_index(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
        unique: bool,
        if_not_exists: bool,
    ) -> Result<()> {
        if if_not_exists
            && self.database_type == DatabaseType::MySQL
            && self.mysql_index_exists(table, name).await?
        {
            return Ok(());
        }
        let sql = ddl::create_index(
            self.database_type,
            name,
            &ddl::quote_table(self.database_type, table),
            columns,
            unique,
            if_not_exists,
        );
        self.execute(&sql).await
    }

    /// Drop the index `name` of `table`
    pub async fn drop_index(&mut self, table: &str, name: &str) -> Result<()> {
        let (schema, table) = ddl::table_parts(self.database_type, table);
        self.execute(&ddl::drop_index(
            self.database_type,
            schema,
            table,
            name,
            false,
        ))
        .await
    }

    /// Drop the index `name` of `table` if it has one. MySQL and MariaDB
    /// take no `DROP INDEX IF EXISTS` on a table, so there the catalog is
    /// asked first.
    pub async fn drop_index_if_exists(&mut self, table: &str, name: &str) -> Result<()> {
        if matches!(
            self.database_type,
            DatabaseType::MySQL | DatabaseType::MariaDB
        ) && !self.mysql_index_exists(table, name).await?
        {
            return Ok(());
        }
        let (schema, table) = ddl::table_parts(self.database_type, table);
        self.execute(&ddl::drop_index(
            self.database_type,
            schema,
            table,
            name,
            true,
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
            ddl::quote_table(self.database_type, name)
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
        if let Some(pretended) = &mut self.pretended {
            pretended.push(sql.to_string());
            return Ok(());
        }
        let db = crate::database::__current_db()?;
        db.exec_unprepared(sql)
            .await
            .map_err(|error| error.with_context(ErrorContext::new().query(sql)))?;

        Ok(())
    }
}
