//! Database migration system
//!
//! This module provides a schema migration system for TideORM, similar to
//! Rails migrations, Laravel migrations, or Sequelize migrations.
//!
//! ## Features
//!
//! - Create, alter, and drop tables
//! - Add, modify, and remove columns
//! - Create and drop indexes
//! - Track applied migrations in the database
//! - Rollback support
//!
//! ## Example
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! use tideorm::migration::*;
//!
//! // Define a migration
//! struct CreateUsersTable;
//!
//! #[async_trait]
//! impl Migration for CreateUsersTable {
//!     fn version(&self) -> &str { "20260106_001" }
//!     fn name(&self) -> &str { "create_users_table" }
//!
//!     async fn up(&self, schema: &mut Schema) -> Result<()> {
//!         schema.create_table("users", |t| {
//!             t.id();
//!             t.string("email").unique();
//!             t.string("name");
//!             t.boolean("active").default(true);
//!             t.timestamps();
//!         }).await
//!     }
//!
//!     async fn down(&self, schema: &mut Schema) -> Result<()> {
//!         schema.drop_table("users").await
//!     }
//! }
//!
//! // Run migrations
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     TideConfig::init()
//!         .database("postgres://localhost/myapp")
//!         .connect()
//!         .await?;
//!
//!     Migrator::new()
//!         .add(CreateUsersTable)
//!         .run()
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

use std::fmt;

use crate::config::DatabaseType;
use crate::database::{db, Database};
use crate::error::{Error, Result};
use crate::internal::ConnectionTrait;

// Re-export async_trait for users
pub use async_trait::async_trait;

// ============================================================================
// MIGRATION TRAIT
// ============================================================================

/// Trait for defining database migrations
///
/// Implement this trait to create a migration. Each migration must have:
/// - A unique version string (typically a timestamp)
/// - A descriptive name
/// - An `up` method that applies the migration
/// - A `down` method that reverts the migration
///
/// # Example
///
/// ```rust,ignore
/// struct AddEmailVerifiedToUsers;
///
/// #[async_trait]
/// impl Migration for AddEmailVerifiedToUsers {
///     fn version(&self) -> &str { "20260106_002" }
///     fn name(&self) -> &str { "add_email_verified_to_users" }
///
///     async fn up(&self, schema: &mut Schema) -> Result<()> {
///         schema.alter_table("users", |t| {
///             t.add_column("email_verified", ColumnType::Boolean)
///                 .default(false)
///                 .not_null();
///         }).await
///     }
///
///     async fn down(&self, schema: &mut Schema) -> Result<()> {
///         schema.alter_table("users", |t| {
///             t.drop_column("email_verified");
///         }).await
///     }
/// }
/// ```
#[async_trait]
pub trait Migration: Send + Sync {
    /// Unique version identifier for this migration
    ///
    /// Format: `YYYYMMDD_NNN` (e.g., "20260106_001")
    /// Migrations are run in lexicographical order by version.
    fn version(&self) -> &str;

    /// Human-readable name for this migration
    fn name(&self) -> &str;

    /// Apply the migration
    async fn up(&self, schema: &mut Schema) -> Result<()>;

    /// Revert the migration
    async fn down(&self, schema: &mut Schema) -> Result<()>;
}

// ============================================================================
// SCHEMA OPERATIONS
// ============================================================================

/// Schema manipulation context for migrations
///
/// Provides methods to create, alter, and drop database objects.
pub struct Schema {
    database_type: DatabaseType,
    statements: Vec<String>,
}

impl Schema {
    /// Create a new schema context
    pub fn new(database_type: DatabaseType) -> Self {
        Self {
            database_type,
            statements: Vec::new(),
        }
    }

    /// Create a new table
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.create_table("users", |t| {
    ///     t.id();
    ///     t.string("email").unique();
    ///     t.string("name").not_null();
    ///     t.timestamps();
    /// }).await?;
    /// ```
    pub async fn create_table<F>(&mut self, name: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut TableBuilder),
    {
        let mut builder = TableBuilder::new(name, self.database_type);
        f(&mut builder);
        let sql = builder.build_create();
        self.execute(&sql).await?;

        // Create indexes
        for index_sql in builder.build_indexes() {
            self.execute(&index_sql).await?;
        }

        Ok(())
    }

    /// Create a table if it doesn't exist
    pub async fn create_table_if_not_exists<F>(&mut self, name: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut TableBuilder),
    {
        let mut builder = TableBuilder::new(name, self.database_type);
        f(&mut builder);
        let sql = builder.build_create_if_not_exists();
        self.execute(&sql).await?;

        // Create indexes (with IF NOT EXISTS)
        for index_sql in builder.build_indexes_if_not_exists() {
            self.execute(&index_sql).await?;
        }

        Ok(())
    }

    /// Alter an existing table
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.alter_table("users", |t| {
    ///     t.add_column("phone", ColumnType::String);
    ///     t.rename_column("name", "full_name");
    ///     t.drop_column("legacy_field");
    /// }).await?;
    /// ```
    pub async fn alter_table<F>(&mut self, name: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut AlterTableBuilder),
    {
        let mut builder = AlterTableBuilder::new(name, self.database_type);
        f(&mut builder);

        for sql in builder.build() {
            self.execute(&sql).await?;
        }

        Ok(())
    }

    /// Drop a table
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.drop_table("users").await?;
    /// ```
    pub async fn drop_table(&mut self, name: &str) -> Result<()> {
        let sql = format!(
            "DROP TABLE {}",
            self.quote_identifier(name)
        );
        self.execute(&sql).await
    }

    /// Drop a table if it exists
    pub async fn drop_table_if_exists(&mut self, name: &str) -> Result<()> {
        let sql = format!(
            "DROP TABLE IF EXISTS {}",
            self.quote_identifier(name)
        );
        self.execute(&sql).await
    }

    /// Rename a table
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.rename_table("users", "accounts").await?;
    /// ```
    pub async fn rename_table(&mut self, from: &str, to: &str) -> Result<()> {
        let sql = match self.database_type {
            DatabaseType::MySQL => format!(
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.create_index("users", "idx_users_email", &["email"], false).await?;
    /// ```
    pub async fn create_index(
        &mut self,
        table: &str,
        name: &str,
        columns: &[&str],
        unique: bool,
    ) -> Result<()> {
        let index_type = if unique { "UNIQUE INDEX" } else { "INDEX" };
        let cols: Vec<String> = columns.iter().map(|c| self.quote_identifier(c)).collect();

        let sql = format!(
            "CREATE {} {} ON {} ({})",
            index_type,
            self.quote_identifier(name),
            self.quote_identifier(table),
            cols.join(", ")
        );
        self.execute(&sql).await
    }

    /// Drop an index
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.drop_index("idx_users_email").await?;
    /// ```
    pub async fn drop_index(&mut self, table: &str, name: &str) -> Result<()> {
        let sql = match self.database_type {
            DatabaseType::MySQL => format!(
                "DROP INDEX {} ON {}",
                self.quote_identifier(name),
                self.quote_identifier(table)
            ),
            _ => format!(
                "DROP INDEX {}",
                self.quote_identifier(name)
            ),
        };
        self.execute(&sql).await
    }

    /// Execute raw SQL
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// schema.raw("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\"").await?;
    /// ```
    pub async fn raw(&mut self, sql: &str) -> Result<()> {
        self.execute(sql).await
    }

    /// Execute a SQL statement
    async fn execute(&mut self, sql: &str) -> Result<()> {
        log_migration_sql(sql);
        self.statements.push(sql.to_string());

        let db = db();
        db.__internal_connection()
            .execute_unprepared(sql)
            .await
            .map_err(|e| Error::query_with_context(
                e.to_string(),
                crate::error::ErrorContext::new().query(sql.to_string()),
            ))?;

        Ok(())
    }

    /// Quote an identifier for the current database type
    fn quote_identifier(&self, name: &str) -> String {
        match self.database_type {
            DatabaseType::Postgres | DatabaseType::SQLite => format!("\"{}\"", name),
            DatabaseType::MySQL => format!("`{}`", name),
        }
    }

    /// Get the database type
    pub fn database_type(&self) -> DatabaseType {
        self.database_type
    }
}

// ============================================================================
// TABLE BUILDER
// ============================================================================

/// Builder for creating tables
pub struct TableBuilder {
    name: String,
    database_type: DatabaseType,
    columns: Vec<ColumnDefinition>,
    indexes: Vec<IndexBuilder>,
    primary_key: Option<String>,
}

impl TableBuilder {
    /// Create a new table builder
    pub fn new(name: &str, database_type: DatabaseType) -> Self {
        Self {
            name: name.to_string(),
            database_type,
            columns: Vec::new(),
            indexes: Vec::new(),
            primary_key: None,
        }
    }

    /// Add an auto-incrementing primary key column named "id"
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// t.id();  // Creates: id BIGSERIAL PRIMARY KEY
    /// ```
    pub fn id(&mut self) -> &mut Self {
        self.big_increments("id")
    }

    /// Add an auto-incrementing big integer column
    pub fn big_increments(&mut self, name: &str) -> &mut Self {
        let col = ColumnDefinition {
            name: name.to_string(),
            column_type: ColumnType::BigInteger,
            nullable: false,
            default: None,
            primary_key: true,
            auto_increment: true,
            unique: false,
        };
        self.columns.push(col);
        self.primary_key = Some(name.to_string());
        self
    }

    /// Add an auto-incrementing integer column
    pub fn increments(&mut self, name: &str) -> &mut Self {
        let col = ColumnDefinition {
            name: name.to_string(),
            column_type: ColumnType::Integer,
            nullable: false,
            default: None,
            primary_key: true,
            auto_increment: true,
            unique: false,
        };
        self.columns.push(col);
        self.primary_key = Some(name.to_string());
        self
    }

    /// Add a string column (VARCHAR/TEXT)
    pub fn string(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::String)
    }

    /// Add a text column (TEXT)
    pub fn text(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Text)
    }

    /// Add an integer column
    pub fn integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Integer)
    }

    /// Add a big integer column
    pub fn big_integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::BigInteger)
    }

    /// Add a small integer column
    pub fn small_integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::SmallInteger)
    }

    /// Add a decimal column
    pub fn decimal(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Decimal { precision: 10, scale: 2 })
    }

    /// Add a decimal column with precision and scale
    pub fn decimal_with(&mut self, name: &str, precision: u32, scale: u32) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Decimal { precision, scale })
    }

    /// Add a float column
    pub fn float(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Float)
    }

    /// Add a double column
    pub fn double(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Double)
    }

    /// Add a boolean column
    pub fn boolean(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Boolean)
    }

    /// Add a date column
    pub fn date(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Date)
    }

    /// Add a time column
    pub fn time(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Time)
    }

    /// Add a datetime/timestamp column
    pub fn datetime(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::DateTime)
    }

    /// Add a timestamp column
    pub fn timestamp(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Timestamp)
    }

    /// Add created_at and updated_at timestamp columns
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// t.timestamps();  // Adds created_at and updated_at columns
    /// ```
    pub fn timestamps(&mut self) -> &mut Self {
        self.column("created_at", ColumnType::Timestamp)
            .default_now()
            .not_null();
        self.column("updated_at", ColumnType::Timestamp)
            .default_now()
            .not_null();
        self
    }

    /// Add a soft delete column (deleted_at)
    pub fn soft_deletes(&mut self) -> &mut Self {
        self.column("deleted_at", ColumnType::Timestamp).nullable();
        self
    }

    /// Add a UUID column
    pub fn uuid(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Uuid)
    }

    /// Add a JSON column
    pub fn json(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Json)
    }

    /// Add a JSONB column (PostgreSQL)
    pub fn jsonb(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Jsonb)
    }

    /// Add a binary/blob column
    pub fn binary(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Binary)
    }

    /// Add an integer array column (PostgreSQL)
    pub fn integer_array(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::IntegerArray)
    }

    /// Add a text array column (PostgreSQL)
    pub fn text_array(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::TextArray)
    }

    /// Add a generic column with a specific type
    pub fn column(&mut self, name: &str, column_type: ColumnType) -> ColumnBuilder<'_> {
        ColumnBuilder {
            table: self,
            definition: ColumnDefinition {
                name: name.to_string(),
                column_type,
                nullable: true,
                default: None,
                primary_key: false,
                auto_increment: false,
                unique: false,
            },
        }
    }

    /// Add a foreign key column (bigint)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// t.foreign_id("user_id");  // Creates: user_id BIGINT
    /// ```
    pub fn foreign_id(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::BigInteger)
    }

    /// Add an index on one or more columns
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// t.index(&["email"]);
    /// t.index(&["first_name", "last_name"]);
    /// ```
    pub fn index(&mut self, columns: &[&str]) -> &mut Self {
        let idx = IndexBuilder {
            name: format!("idx_{}_{}", self.name, columns.join("_")),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            unique: false,
        };
        self.indexes.push(idx);
        self
    }

    /// Add a unique index on one or more columns
    pub fn unique_index(&mut self, columns: &[&str]) -> &mut Self {
        let idx = IndexBuilder {
            name: format!("idx_{}_{}_unique", self.name, columns.join("_")),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            unique: true,
        };
        self.indexes.push(idx);
        self
    }

    /// Add a named index
    pub fn index_named(&mut self, name: &str, columns: &[&str]) -> &mut Self {
        let idx = IndexBuilder {
            name: name.to_string(),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            unique: false,
        };
        self.indexes.push(idx);
        self
    }

    /// Build the CREATE TABLE SQL statement
    fn build_create(&self) -> String {
        self.build_create_internal(false)
    }

    /// Build the CREATE TABLE IF NOT EXISTS SQL statement
    fn build_create_if_not_exists(&self) -> String {
        self.build_create_internal(true)
    }

    fn build_create_internal(&self, if_not_exists: bool) -> String {
        let exists_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
        let mut sql = format!(
            "CREATE TABLE {}{} (\n",
            exists_clause,
            self.quote_identifier(&self.name)
        );

        let column_defs: Vec<String> = self
            .columns
            .iter()
            .map(|col| self.build_column_def(col))
            .collect();

        sql.push_str(&column_defs.join(",\n"));

        // Add primary key constraint if specified
        if let Some(ref pk) = self.primary_key {
            sql.push_str(",\n");
            sql.push_str(&format!(
                "    PRIMARY KEY ({})",
                self.quote_identifier(pk)
            ));
        }

        sql.push_str("\n)");
        sql
    }

    /// Build column definition SQL
    fn build_column_def(&self, col: &ColumnDefinition) -> String {
        let mut def = format!(
            "    {} {}",
            self.quote_identifier(&col.name),
            self.type_to_sql(&col.column_type)
        );

        // Handle auto-increment
        if col.auto_increment {
            match self.database_type {
                DatabaseType::Postgres => {
                    // Replace type with SERIAL/BIGSERIAL
                    def = match col.column_type {
                        ColumnType::Integer => format!(
                            "    {} SERIAL",
                            self.quote_identifier(&col.name)
                        ),
                        _ => format!(
                            "    {} BIGSERIAL",
                            self.quote_identifier(&col.name)
                        ),
                    };
                }
                DatabaseType::MySQL => {
                    def.push_str(" AUTO_INCREMENT");
                }
                DatabaseType::SQLite => {
                    // SQLite auto-increments INTEGER PRIMARY KEY automatically
                }
            }
        }

        // NOT NULL
        if !col.nullable && !col.primary_key {
            def.push_str(" NOT NULL");
        }

        // DEFAULT
        if let Some(ref default) = col.default {
            def.push_str(&format!(" DEFAULT {}", default));
        }

        // UNIQUE (handled separately from indexes)
        if col.unique && !col.primary_key {
            def.push_str(" UNIQUE");
        }

        def
    }

    /// Build CREATE INDEX statements
    fn build_indexes(&self) -> Vec<String> {
        self.build_indexes_internal(false)
    }

    /// Build CREATE INDEX IF NOT EXISTS statements
    fn build_indexes_if_not_exists(&self) -> Vec<String> {
        self.build_indexes_internal(true)
    }

    fn build_indexes_internal(&self, if_not_exists: bool) -> Vec<String> {
        let exists_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
        self.indexes
            .iter()
            .map(|idx| {
                let index_type = if idx.unique { "UNIQUE INDEX" } else { "INDEX" };
                let cols: Vec<String> = idx
                    .columns
                    .iter()
                    .map(|c| self.quote_identifier(c))
                    .collect();

                format!(
                    "CREATE {} {}{} ON {} ({})",
                    index_type,
                    exists_clause,
                    self.quote_identifier(&idx.name),
                    self.quote_identifier(&self.name),
                    cols.join(", ")
                )
            })
            .collect()
    }

    /// Convert column type to SQL type string
    fn type_to_sql(&self, column_type: &ColumnType) -> String {
        match self.database_type {
            DatabaseType::Postgres => column_type.to_postgres_sql(),
            DatabaseType::MySQL => column_type.to_mysql_sql(),
            DatabaseType::SQLite => column_type.to_sqlite_sql(),
        }
    }

    /// Quote an identifier
    fn quote_identifier(&self, name: &str) -> String {
        match self.database_type {
            DatabaseType::Postgres | DatabaseType::SQLite => format!("\"{}\"", name),
            DatabaseType::MySQL => format!("`{}`", name),
        }
    }
}

// ============================================================================
// COLUMN BUILDER
// ============================================================================

/// Builder for column definitions (fluent API)
pub struct ColumnBuilder<'a> {
    table: &'a mut TableBuilder,
    definition: ColumnDefinition,
}

impl<'a> ColumnBuilder<'a> {
    /// Mark the column as NOT NULL
    pub fn not_null(mut self) -> Self {
        self.definition.nullable = false;
        self
    }

    /// Mark the column as nullable
    pub fn nullable(mut self) -> Self {
        self.definition.nullable = true;
        self
    }

    /// Set a default value
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.definition.default = Some(value.into().to_sql());
        self
    }

    /// Set default to current timestamp
    pub fn default_now(mut self) -> Self {
        self.definition.default = Some("CURRENT_TIMESTAMP".to_string());
        self
    }

    /// Mark the column as unique
    pub fn unique(mut self) -> Self {
        self.definition.unique = true;
        self
    }

    /// Mark as primary key
    pub fn primary_key(mut self) -> Self {
        self.definition.primary_key = true;
        self.definition.nullable = false;
        self.table.primary_key = Some(self.definition.name.clone());
        self
    }
}

impl<'a> Drop for ColumnBuilder<'a> {
    fn drop(&mut self) {
        // Move the definition to the table
        let def = std::mem::replace(
            &mut self.definition,
            ColumnDefinition {
                name: String::new(),
                column_type: ColumnType::String,
                nullable: true,
                default: None,
                primary_key: false,
                auto_increment: false,
                unique: false,
            },
        );
        if !def.name.is_empty() {
            self.table.columns.push(def);
        }
    }
}

// ============================================================================
// ALTER TABLE BUILDER
// ============================================================================

/// Builder for ALTER TABLE operations
pub struct AlterTableBuilder {
    name: String,
    database_type: DatabaseType,
    operations: Vec<AlterOperation>,
}

impl AlterTableBuilder {
    /// Create a new alter table builder
    pub fn new(name: &str, database_type: DatabaseType) -> Self {
        Self {
            name: name.to_string(),
            database_type,
            operations: Vec::new(),
        }
    }

    /// Add a new column
    pub fn add_column(&mut self, name: &str, column_type: ColumnType) -> AlterColumnBuilder<'_> {
        AlterColumnBuilder {
            builder: self,
            definition: ColumnDefinition {
                name: name.to_string(),
                column_type,
                nullable: true,
                default: None,
                primary_key: false,
                auto_increment: false,
                unique: false,
            },
        }
    }

    /// Drop a column
    pub fn drop_column(&mut self, name: &str) -> &mut Self {
        self.operations.push(AlterOperation::DropColumn(name.to_string()));
        self
    }

    /// Rename a column
    pub fn rename_column(&mut self, from: &str, to: &str) -> &mut Self {
        self.operations.push(AlterOperation::RenameColumn(
            from.to_string(),
            to.to_string(),
        ));
        self
    }

    /// Change column type
    pub fn change_column(&mut self, name: &str, column_type: ColumnType) -> &mut Self {
        self.operations.push(AlterOperation::ChangeColumnType(
            name.to_string(),
            column_type,
        ));
        self
    }

    /// Add an index
    pub fn add_index(&mut self, name: &str, columns: &[&str], unique: bool) -> &mut Self {
        self.operations.push(AlterOperation::AddIndex(IndexBuilder {
            name: name.to_string(),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            unique,
        }));
        self
    }

    /// Drop an index
    pub fn drop_index(&mut self, name: &str) -> &mut Self {
        self.operations.push(AlterOperation::DropIndex(name.to_string()));
        self
    }

    /// Build the ALTER TABLE SQL statements
    fn build(&self) -> Vec<String> {
        self.operations
            .iter()
            .map(|op| self.build_operation(op))
            .collect()
    }

    fn build_operation(&self, op: &AlterOperation) -> String {
        match op {
            AlterOperation::AddColumn(col) => {
                let col_def = self.build_column_def(col);
                format!(
                    "ALTER TABLE {} ADD COLUMN {}",
                    self.quote_identifier(&self.name),
                    col_def.trim()
                )
            }
            AlterOperation::DropColumn(name) => {
                format!(
                    "ALTER TABLE {} DROP COLUMN {}",
                    self.quote_identifier(&self.name),
                    self.quote_identifier(name)
                )
            }
            AlterOperation::RenameColumn(from, to) => match self.database_type {
                DatabaseType::Postgres | DatabaseType::SQLite => {
                    format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        self.quote_identifier(&self.name),
                        self.quote_identifier(from),
                        self.quote_identifier(to)
                    )
                }
                DatabaseType::MySQL => {
                    // MySQL requires the column type in CHANGE
                    format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        self.quote_identifier(&self.name),
                        self.quote_identifier(from),
                        self.quote_identifier(to)
                    )
                }
            },
            AlterOperation::ChangeColumnType(name, column_type) => {
                let type_sql = self.type_to_sql(column_type);
                match self.database_type {
                    DatabaseType::Postgres => {
                        format!(
                            "ALTER TABLE {} ALTER COLUMN {} TYPE {}",
                            self.quote_identifier(&self.name),
                            self.quote_identifier(name),
                            type_sql
                        )
                    }
                    DatabaseType::MySQL => {
                        format!(
                            "ALTER TABLE {} MODIFY COLUMN {} {}",
                            self.quote_identifier(&self.name),
                            self.quote_identifier(name),
                            type_sql
                        )
                    }
                    DatabaseType::SQLite => {
                        // SQLite doesn't support ALTER COLUMN TYPE directly
                        // Would need to recreate the table
                        format!(
                            "-- SQLite does not support ALTER COLUMN TYPE; table recreation needed for {}",
                            name
                        )
                    }
                }
            }
            AlterOperation::AddIndex(idx) => {
                let index_type = if idx.unique { "UNIQUE INDEX" } else { "INDEX" };
                let cols: Vec<String> = idx
                    .columns
                    .iter()
                    .map(|c| self.quote_identifier(c))
                    .collect();

                format!(
                    "CREATE {} {} ON {} ({})",
                    index_type,
                    self.quote_identifier(&idx.name),
                    self.quote_identifier(&self.name),
                    cols.join(", ")
                )
            }
            AlterOperation::DropIndex(name) => match self.database_type {
                DatabaseType::MySQL => {
                    format!(
                        "DROP INDEX {} ON {}",
                        self.quote_identifier(name),
                        self.quote_identifier(&self.name)
                    )
                }
                _ => {
                    format!("DROP INDEX {}", self.quote_identifier(name))
                }
            },
        }
    }

    fn build_column_def(&self, col: &ColumnDefinition) -> String {
        let mut def = format!(
            "{} {}",
            self.quote_identifier(&col.name),
            self.type_to_sql(&col.column_type)
        );

        if !col.nullable {
            def.push_str(" NOT NULL");
        }

        if let Some(ref default) = col.default {
            def.push_str(&format!(" DEFAULT {}", default));
        }

        if col.unique {
            def.push_str(" UNIQUE");
        }

        def
    }

    fn type_to_sql(&self, column_type: &ColumnType) -> String {
        match self.database_type {
            DatabaseType::Postgres => column_type.to_postgres_sql(),
            DatabaseType::MySQL => column_type.to_mysql_sql(),
            DatabaseType::SQLite => column_type.to_sqlite_sql(),
        }
    }

    fn quote_identifier(&self, name: &str) -> String {
        match self.database_type {
            DatabaseType::Postgres | DatabaseType::SQLite => format!("\"{}\"", name),
            DatabaseType::MySQL => format!("`{}`", name),
        }
    }
}

/// Builder for adding columns in ALTER TABLE
pub struct AlterColumnBuilder<'a> {
    builder: &'a mut AlterTableBuilder,
    definition: ColumnDefinition,
}

impl<'a> AlterColumnBuilder<'a> {
    /// Mark as NOT NULL
    pub fn not_null(mut self) -> Self {
        self.definition.nullable = false;
        self
    }

    /// Mark as nullable
    pub fn nullable(mut self) -> Self {
        self.definition.nullable = true;
        self
    }

    /// Set default value
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.definition.default = Some(value.into().to_sql());
        self
    }

    /// Set default to current timestamp
    pub fn default_now(mut self) -> Self {
        self.definition.default = Some("CURRENT_TIMESTAMP".to_string());
        self
    }

    /// Mark as unique
    pub fn unique(mut self) -> Self {
        self.definition.unique = true;
        self
    }
}

impl<'a> Drop for AlterColumnBuilder<'a> {
    fn drop(&mut self) {
        let def = std::mem::replace(
            &mut self.definition,
            ColumnDefinition {
                name: String::new(),
                column_type: ColumnType::String,
                nullable: true,
                default: None,
                primary_key: false,
                auto_increment: false,
                unique: false,
            },
        );
        if !def.name.is_empty() {
            self.builder.operations.push(AlterOperation::AddColumn(def));
        }
    }
}

// ============================================================================
// COLUMN TYPES
// ============================================================================

/// Supported column types for migrations
#[derive(Debug, Clone)]
pub enum ColumnType {
    /// Small integer (2 bytes)
    SmallInteger,
    /// Integer (4 bytes)
    Integer,
    /// Big integer (8 bytes)
    BigInteger,
    /// Single precision float
    Float,
    /// Double precision float
    Double,
    /// Decimal with precision and scale
    Decimal {
        /// Total number of digits
        precision: u32,
        /// Number of digits after decimal point
        scale: u32,
    },
    /// Variable length string
    String,
    /// Text (unlimited length)
    Text,
    /// Boolean
    Boolean,
    /// Date
    Date,
    /// Time
    Time,
    /// DateTime
    DateTime,
    /// Timestamp
    Timestamp,
    /// UUID
    Uuid,
    /// JSON
    Json,
    /// JSONB (PostgreSQL)
    Jsonb,
    /// Binary/Blob
    Binary,
    /// Integer array (PostgreSQL)
    IntegerArray,
    /// Text array (PostgreSQL)
    TextArray,
    /// Custom SQL type
    Custom(String),
}

impl ColumnType {
    /// Convert to PostgreSQL SQL type
    pub fn to_postgres_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger => "SMALLINT".to_string(),
            ColumnType::Integer => "INTEGER".to_string(),
            ColumnType::BigInteger => "BIGINT".to_string(),
            ColumnType::Float => "REAL".to_string(),
            ColumnType::Double => "DOUBLE PRECISION".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::String => "VARCHAR(255)".to_string(),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Boolean => "BOOLEAN".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::DateTime => "TIMESTAMP".to_string(),
            ColumnType::Timestamp => "TIMESTAMP".to_string(),
            ColumnType::Uuid => "UUID".to_string(),
            ColumnType::Json => "JSON".to_string(),
            ColumnType::Jsonb => "JSONB".to_string(),
            ColumnType::Binary => "BYTEA".to_string(),
            ColumnType::IntegerArray => "INTEGER[]".to_string(),
            ColumnType::TextArray => "TEXT[]".to_string(),
            ColumnType::Custom(s) => s.clone(),
        }
    }

    /// Convert to MySQL SQL type
    pub fn to_mysql_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger => "SMALLINT".to_string(),
            ColumnType::Integer => "INT".to_string(),
            ColumnType::BigInteger => "BIGINT".to_string(),
            ColumnType::Float => "FLOAT".to_string(),
            ColumnType::Double => "DOUBLE".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::String => "VARCHAR(255)".to_string(),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Boolean => "TINYINT(1)".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::DateTime => "DATETIME".to_string(),
            ColumnType::Timestamp => "TIMESTAMP".to_string(),
            ColumnType::Uuid => "CHAR(36)".to_string(),
            ColumnType::Json | ColumnType::Jsonb => "JSON".to_string(),
            ColumnType::Binary => "BLOB".to_string(),
            ColumnType::IntegerArray | ColumnType::TextArray => "JSON".to_string(), // MySQL uses JSON for arrays
            ColumnType::Custom(s) => s.clone(),
        }
    }

    /// Convert to SQLite SQL type
    pub fn to_sqlite_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::Boolean => "INTEGER".to_string(),
            ColumnType::Float | ColumnType::Double | ColumnType::Decimal { .. } => {
                "REAL".to_string()
            }
            ColumnType::String
            | ColumnType::Text
            | ColumnType::Uuid
            | ColumnType::Date
            | ColumnType::Time
            | ColumnType::DateTime
            | ColumnType::Timestamp
            | ColumnType::Json
            | ColumnType::Jsonb
            | ColumnType::IntegerArray
            | ColumnType::TextArray => "TEXT".to_string(),
            ColumnType::Binary => "BLOB".to_string(),
            ColumnType::Custom(s) => s.clone(),
        }
    }
}

// ============================================================================
// DEFAULT VALUES
// ============================================================================

/// Default value for columns
#[derive(Debug, Clone)]
pub enum DefaultValue {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// Boolean value
    Boolean(bool),
    /// Raw SQL expression
    Raw(String),
    /// NULL
    Null,
}

impl DefaultValue {
    /// Convert to SQL representation
    pub fn to_sql(&self) -> String {
        match self {
            DefaultValue::String(s) => format!("'{}'", s.replace('\'', "''")),
            DefaultValue::Integer(i) => i.to_string(),
            DefaultValue::Float(f) => f.to_string(),
            DefaultValue::Boolean(b) => {
                if *b {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            }
            DefaultValue::Raw(s) => s.clone(),
            DefaultValue::Null => "NULL".to_string(),
        }
    }
}

impl From<&str> for DefaultValue {
    fn from(s: &str) -> Self {
        DefaultValue::String(s.to_string())
    }
}

impl From<String> for DefaultValue {
    fn from(s: String) -> Self {
        DefaultValue::String(s)
    }
}

impl From<i32> for DefaultValue {
    fn from(i: i32) -> Self {
        DefaultValue::Integer(i as i64)
    }
}

impl From<i64> for DefaultValue {
    fn from(i: i64) -> Self {
        DefaultValue::Integer(i)
    }
}

impl From<f64> for DefaultValue {
    fn from(f: f64) -> Self {
        DefaultValue::Float(f)
    }
}

impl From<bool> for DefaultValue {
    fn from(b: bool) -> Self {
        DefaultValue::Boolean(b)
    }
}

// ============================================================================
// INTERNAL TYPES
// ============================================================================

/// Internal column definition
#[derive(Debug, Clone)]
struct ColumnDefinition {
    name: String,
    column_type: ColumnType,
    nullable: bool,
    default: Option<String>,
    primary_key: bool,
    auto_increment: bool,
    unique: bool,
}

/// Internal index definition
#[derive(Debug, Clone)]
struct IndexBuilder {
    name: String,
    columns: Vec<String>,
    unique: bool,
}

/// ALTER TABLE operations
#[derive(Debug, Clone)]
enum AlterOperation {
    AddColumn(ColumnDefinition),
    DropColumn(String),
    RenameColumn(String, String),
    ChangeColumnType(String, ColumnType),
    AddIndex(IndexBuilder),
    DropIndex(String),
}

// ============================================================================
// MIGRATOR
// ============================================================================

/// Migration runner
///
/// Manages and executes database migrations.
///
/// # Example
///
/// ```rust,ignore
/// Migrator::new()
///     .add(CreateUsersTable)
///     .add(CreatePostsTable)
///     .add(AddEmailVerifiedToUsers)
///     .run()
///     .await?;
/// ```
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

        let db = db();
        let db_type = detect_database_type(db);

        // Sort migrations by version
        let mut migrations: Vec<_> = self.migrations.iter().collect();
        migrations.sort_by_key(|m| m.version());

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

            // Record migration
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

        // Get the last applied migration version
        let last_version = applied.last().unwrap();

        let db = db();
        let db_type = detect_database_type(db);

        // Find the migration
        for migration in &self.migrations {
            if migration.version() == last_version {
                log_migration_rollback(last_version, migration.name());

                let mut schema = Schema::new(db_type);
                migration.down(&mut schema).await?;

                // Remove migration record
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
        migrations.sort_by_key(|m| m.version());

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

    // =========================================================================
    // MIGRATIONS TABLE MANAGEMENT
    // =========================================================================

    /// Ensure the migrations table exists
    async fn ensure_migrations_table(&self) -> Result<()> {
        let db = db();
        let db_type = detect_database_type(db);

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
            DatabaseType::MySQL => {
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

        db.__internal_connection()
            .execute_unprepared(sql)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        Ok(())
    }

    /// Get list of applied migration versions
    async fn get_applied_migrations(&self) -> Result<Vec<String>> {
        let db = db();

        use crate::internal::Statement;

        let backend = db.__internal_connection().get_database_backend();
        let sql = r#"SELECT "version" FROM "_migrations" ORDER BY "version" ASC"#;
        let stmt = Statement::from_string(backend, sql.to_string());

        let results = db
            .__internal_connection()
            .query_all(stmt)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let mut versions = Vec::new();
        for row in results {
            let version: String = row
                .try_get("", "version")
                .map_err(|e| Error::query(e.to_string()))?;
            versions.push(version);
        }

        Ok(versions)
    }

    /// Record a migration as applied
    async fn record_migration(&self, version: &str, name: &str) -> Result<()> {
        let db = db();

        let sql = format!(
            r#"INSERT INTO "_migrations" ("version", "name") VALUES ('{}', '{}')"#,
            version.replace('\'', "''"),
            name.replace('\'', "''")
        );

        db.__internal_connection()
            .execute_unprepared(&sql)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        Ok(())
    }

    /// Remove a migration record
    async fn remove_migration_record(&self, version: &str) -> Result<()> {
        let db = db();

        let sql = format!(
            r#"DELETE FROM "_migrations" WHERE "version" = '{}'"#,
            version.replace('\'', "''")
        );

        db.__internal_connection()
            .execute_unprepared(&sql)
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        Ok(())
    }
}

impl Default for Migrator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// RESULT TYPES
// ============================================================================

/// Result of migration operations
#[derive(Debug, Clone)]
pub struct MigrationResult {
    /// Successfully applied migrations
    pub applied: Vec<MigrationInfo>,
    /// Skipped (already applied) migrations
    pub skipped: Vec<MigrationInfo>,
    /// Rolled back migrations
    pub rolled_back: Vec<MigrationInfo>,
}

impl MigrationResult {
    fn new() -> Self {
        Self {
            applied: Vec::new(),
            skipped: Vec::new(),
            rolled_back: Vec::new(),
        }
    }

    /// Check if any migrations were applied
    pub fn has_applied(&self) -> bool {
        !self.applied.is_empty()
    }

    /// Check if any migrations were rolled back
    pub fn has_rolled_back(&self) -> bool {
        !self.rolled_back.is_empty()
    }
}

impl fmt::Display for MigrationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.applied.is_empty() {
            writeln!(f, "Applied migrations:")?;
            for m in &self.applied {
                writeln!(f, "  ✓ {} - {}", m.version, m.name)?;
            }
        }

        if !self.skipped.is_empty() {
            writeln!(f, "Skipped migrations (already applied):")?;
            for m in &self.skipped {
                writeln!(f, "  - {} - {}", m.version, m.name)?;
            }
        }

        if !self.rolled_back.is_empty() {
            writeln!(f, "Rolled back migrations:")?;
            for m in &self.rolled_back {
                writeln!(f, "  ↩ {} - {}", m.version, m.name)?;
            }
        }

        Ok(())
    }
}

/// Information about a single migration
#[derive(Debug, Clone)]
pub struct MigrationInfo {
    /// Migration version
    pub version: String,
    /// Migration name
    pub name: String,
}

/// Status of a single migration
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    /// Migration version
    pub version: String,
    /// Migration name
    pub name: String,
    /// Whether the migration has been applied
    pub applied: bool,
}

impl fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.applied { "✓" } else { "○" };
        write!(f, "[{}] {} - {}", status, self.version, self.name)
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Detect database type from connection
fn detect_database_type(db: &Database) -> DatabaseType {
    use crate::internal::DbBackend;

    match db.__internal_connection().get_database_backend() {
        DbBackend::Postgres => DatabaseType::Postgres,
        DbBackend::MySql => DatabaseType::MySQL,
        DbBackend::Sqlite => DatabaseType::SQLite,
    }
}

/// Log migration SQL (respects TIDE_LOG_QUERIES)
fn log_migration_sql(sql: &str) {
    if std::env::var("TIDE_LOG_QUERIES").is_ok() {
        eprintln!("[Migration SQL] {}", sql);
    }
}

/// Log migration start
fn log_migration_start(version: &str, name: &str) {
    eprintln!("Running migration: {} - {}", version, name);
}

/// Log migration complete
fn log_migration_complete(version: &str, name: &str) {
    eprintln!("Completed migration: {} - {}", version, name);
}

/// Log migration rollback
fn log_migration_rollback(version: &str, name: &str) {
    eprintln!("Rolling back migration: {} - {}", version, name);
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_type_postgres() {
        assert_eq!(ColumnType::Integer.to_postgres_sql(), "INTEGER");
        assert_eq!(ColumnType::BigInteger.to_postgres_sql(), "BIGINT");
        assert_eq!(ColumnType::String.to_postgres_sql(), "VARCHAR(255)");
        assert_eq!(ColumnType::Text.to_postgres_sql(), "TEXT");
        assert_eq!(ColumnType::Boolean.to_postgres_sql(), "BOOLEAN");
        assert_eq!(ColumnType::Jsonb.to_postgres_sql(), "JSONB");
        assert_eq!(ColumnType::IntegerArray.to_postgres_sql(), "INTEGER[]");
    }

    #[test]
    fn test_column_type_mysql() {
        assert_eq!(ColumnType::Integer.to_mysql_sql(), "INT");
        assert_eq!(ColumnType::BigInteger.to_mysql_sql(), "BIGINT");
        assert_eq!(ColumnType::Boolean.to_mysql_sql(), "TINYINT(1)");
        assert_eq!(ColumnType::Jsonb.to_mysql_sql(), "JSON");
    }

    #[test]
    fn test_column_type_sqlite() {
        assert_eq!(ColumnType::Integer.to_sqlite_sql(), "INTEGER");
        assert_eq!(ColumnType::BigInteger.to_sqlite_sql(), "INTEGER");
        assert_eq!(ColumnType::String.to_sqlite_sql(), "TEXT");
        assert_eq!(ColumnType::Boolean.to_sqlite_sql(), "INTEGER");
    }

    #[test]
    fn test_default_value() {
        assert_eq!(DefaultValue::String("test".to_string()).to_sql(), "'test'");
        assert_eq!(DefaultValue::Integer(42).to_sql(), "42");
        assert_eq!(DefaultValue::Boolean(true).to_sql(), "TRUE");
        assert_eq!(DefaultValue::Boolean(false).to_sql(), "FALSE");
        assert_eq!(DefaultValue::Null.to_sql(), "NULL");
    }

    #[test]
    fn test_table_builder_create() {
        let mut builder = TableBuilder::new("users", DatabaseType::Postgres);
        builder.id();
        builder.string("email").unique().not_null();
        builder.string("name").not_null();
        builder.boolean("active").default(true);
        builder.timestamps();

        let sql = builder.build_create();
        assert!(sql.contains("CREATE TABLE"));
        assert!(sql.contains("\"users\""));
        assert!(sql.contains("\"id\" BIGSERIAL"));
        assert!(sql.contains("\"email\""));
        assert!(sql.contains("\"name\""));
        assert!(sql.contains("\"active\""));
        assert!(sql.contains("\"created_at\""));
        assert!(sql.contains("\"updated_at\""));
    }

    #[test]
    fn test_alter_table_builder() {
        let mut builder = AlterTableBuilder::new("users", DatabaseType::Postgres);
        builder.add_column("phone", ColumnType::String).nullable();
        builder.drop_column("legacy");
        builder.rename_column("name", "full_name");

        let statements = builder.build();
        assert_eq!(statements.len(), 3);
        assert!(statements[0].contains("ADD COLUMN"));
        assert!(statements[1].contains("DROP COLUMN"));
        assert!(statements[2].contains("RENAME COLUMN"));
    }
}
