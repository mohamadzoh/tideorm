//! Schema generation module
//!
//! This module writes SQL schema output from TideORM model definitions.
//!
//! It is mainly for exporting or inspecting schema SQL, not for applying live
//! migrations. If generated SQL looks wrong, check the model metadata and index
//! declarations first.
//!
//! You can wire schema generation through `TideConfig::schema_file(...)` or use
//! `SchemaWriter::write_schema(...)` directly.

use parking_lot::RwLock;
use std::fs;
use std::path::Path;

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::sql_safety::{format_identifier_reference, quote_ident};
use crate::model::IndexDefinition;

// Global schema registry for auto-generation
static SCHEMA_REGISTRY: RwLock<Vec<TableSchema>> = RwLock::new(Vec::new());

/// Schema generator for creating SQL schema files
pub struct SchemaGenerator {
    database_type: DatabaseType,
    tables: Vec<TableSchema>,
}

/// Schema for a single table
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Table name
    pub name: String,
    /// Optional schema name for qualified table references
    pub schema_name: Option<String>,
    /// Column definitions
    pub columns: Vec<ColumnSchema>,
    /// Index definitions (regular and unique)
    pub indexes: Vec<IndexDefinition>,
    /// Primary key column name
    pub primary_key: String,
    /// Primary key column names, in declaration order.
    pub primary_keys: Vec<String>,
}

/// Schema for a single column
#[derive(Debug, Clone)]
pub struct ColumnSchema {
    /// Column name
    pub name: String,
    /// SQL type (e.g., "BIGINT", "TEXT", "TIMESTAMP")
    pub sql_type: String,
    /// Whether the column allows NULL values
    pub nullable: bool,
    /// Default value expression (e.g., "now()", "'active'")
    pub default: Option<String>,
    /// Whether this column is the primary key
    pub primary_key: bool,
    /// Whether this column auto-increments
    pub auto_increment: bool,
}

impl SchemaGenerator {
    /// Create a new schema generator
    pub fn new(database_type: DatabaseType) -> Self {
        Self {
            database_type,
            tables: Vec::new(),
        }
    }

    /// Add a table schema
    pub fn add_table(&mut self, schema: TableSchema) {
        self.tables.push(schema);
    }

    /// Generate complete SQL schema
    pub fn generate(&self) -> String {
        let mut sql = String::new();

        // Header comment
        sql.push_str("-- TideORM Generated Schema\n");
        sql.push_str(&format!("-- Database: {:?}\n", self.database_type));
        sql.push_str(&format!(
            "-- Generated at: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));

        // Generate CREATE TABLE statements
        for table in &self.tables {
            sql.push_str(&self.generate_create_table(table));
            sql.push('\n');
        }

        // Generate CREATE INDEX statements
        for table in &self.tables {
            let indexes = self.generate_indexes(table);
            if !indexes.is_empty() {
                sql.push_str(&indexes);
                sql.push('\n');
            }
        }

        sql
    }

    /// Generate CREATE TABLE statement
    fn generate_create_table(&self, table: &TableSchema) -> String {
        let mut sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (\n",
            self.quote_table_identifier(table)
        );

        let column_defs: Vec<String> = table
            .columns
            .iter()
            .map(|col| self.generate_column_def(col))
            .collect();

        sql.push_str(&column_defs.join(",\n"));

        // Add primary key constraint if not inline
        let primary_keys = if !table.primary_keys.is_empty() {
            table.primary_keys.clone()
        } else if !table.primary_key.is_empty() {
            vec![table.primary_key.clone()]
        } else {
            Vec::new()
        };

        if !primary_keys.is_empty() {
            sql.push_str(",\n");
            sql.push_str(&format!(
                "    PRIMARY KEY ({})",
                primary_keys
                    .iter()
                    .map(|column| self.quote_identifier(column))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        sql.push_str("\n);\n");
        sql
    }

    /// Generate column definition
    fn generate_column_def(&self, col: &ColumnSchema) -> String {
        let mut def = format!("    {} {}", self.quote_identifier(&col.name), col.sql_type);

        // Auto increment handling
        if col.auto_increment {
            match self.database_type {
                DatabaseType::Postgres => {
                    // PostgreSQL uses SERIAL/BIGSERIAL or GENERATED
                    if col.sql_type.to_uppercase().contains("INT") {
                        def = format!("    {} BIGSERIAL", self.quote_identifier(&col.name));
                    }
                }
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    def.push_str(" AUTO_INCREMENT");
                }
                DatabaseType::SQLite => {
                    // SQLite auto-increments INTEGER PRIMARY KEY automatically
                }
            }
        }

        // Nullable
        if !col.nullable && !col.primary_key {
            def.push_str(" NOT NULL");
        }

        // Default value
        if let Some(default) = &col.default {
            def.push_str(&format!(" DEFAULT {}", default));
        }

        def
    }

    /// Generate CREATE INDEX statements
    fn generate_indexes(&self, table: &TableSchema) -> String {
        let mut sql = String::new();

        for index in &table.indexes {
            let index_type = if index.unique {
                "UNIQUE INDEX"
            } else {
                "INDEX"
            };
            let columns: Vec<String> = index
                .columns
                .iter()
                .map(|c| self.quote_identifier(c))
                .collect();

            sql.push_str(&format!(
                "CREATE {} IF NOT EXISTS {} ON {} ({});\n",
                index_type,
                self.quote_identifier(&index.name),
                self.quote_table_identifier(table),
                columns.join(", ")
            ));
        }

        sql
    }

    /// Quote identifier based on database type
    fn quote_identifier(&self, name: &str) -> String {
        quote_ident(self.database_type, name)
    }

    fn quote_identifier_reference(&self, name: &str) -> String {
        format_identifier_reference(self.database_type, name)
            .unwrap_or_else(|| self.quote_identifier(name))
    }

    fn quote_table_identifier(&self, table: &TableSchema) -> String {
        if let Some(schema_name) = &table.schema_name {
            return format!(
                "{}.{}",
                self.quote_identifier(schema_name),
                self.quote_identifier(&table.name)
            );
        }

        self.quote_identifier_reference(&table.name)
    }
}

/// Builder for table schemas from model metadata
pub struct TableSchemaBuilder {
    name: String,
    schema_name: Option<String>,
    columns: Vec<ColumnSchema>,
    indexes: Vec<IndexDefinition>,
    primary_key: String,
    primary_keys: Vec<String>,
}

impl TableSchemaBuilder {
    /// Create a new table schema builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema_name: None,
            columns: Vec::new(),
            indexes: Vec::new(),
            primary_key: String::new(),
            primary_keys: Vec::new(),
        }
    }

    /// Set the schema name for this table.
    pub fn schema(mut self, schema_name: impl Into<String>) -> Self {
        self.schema_name = Some(schema_name.into());
        self
    }

    /// Add a column
    pub fn column(mut self, schema: ColumnSchema) -> Self {
        if schema.primary_key {
            if self.primary_key.is_empty() {
                self.primary_key = schema.name.clone();
            }
            self.primary_keys.push(schema.name.clone());
        }
        self.columns.push(schema);
        self
    }

    // ========================================================================
    // Convenience methods for common column types
    // ========================================================================

    /// Add a BIGINT column
    pub fn bigint(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "BIGINT"))
    }

    /// Add an INTEGER column
    pub fn integer(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "INTEGER"))
    }

    /// Add a SMALLINT column
    pub fn smallint(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "SMALLINT"))
    }

    /// Add a TEXT column
    pub fn text(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TEXT"))
    }

    /// Add a VARCHAR column with specified length
    pub fn varchar(self, name: impl Into<String>, length: u32) -> Self {
        self.column(ColumnSchema::new(name, format!("VARCHAR({})", length)))
    }

    /// Add a BOOLEAN column
    pub fn boolean(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "BOOLEAN"))
    }

    /// Add a TIMESTAMP column (without time zone)
    pub fn timestamp(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TIMESTAMP"))
    }

    /// Add a TIMESTAMPTZ column (timestamp with time zone) - use for DateTime<Utc>
    pub fn timestamptz(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TIMESTAMPTZ"))
    }

    /// Add a DATE column
    pub fn date(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "DATE"))
    }

    /// Add a TIME column
    pub fn time(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TIME"))
    }

    /// Add a UUID column
    pub fn uuid(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "UUID"))
    }

    /// Add a DECIMAL column
    pub fn decimal(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "DECIMAL"))
    }

    /// Add a DECIMAL column with precision and scale
    pub fn decimal_with_precision(
        self,
        name: impl Into<String>,
        precision: u32,
        scale: u32,
    ) -> Self {
        self.column(ColumnSchema::new(
            name,
            format!("DECIMAL({},{})", precision, scale),
        ))
    }

    /// Add a JSONB column (PostgreSQL)
    pub fn jsonb(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "JSONB"))
    }

    /// Add a JSON column
    pub fn json(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "JSON"))
    }

    /// Add a BYTEA column (PostgreSQL binary)
    pub fn bytea(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "BYTEA"))
    }

    /// Add an REAL (single precision float) column
    pub fn real(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "REAL"))
    }

    /// Add a DOUBLE PRECISION column
    pub fn double(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "DOUBLE PRECISION"))
    }

    /// Add an index
    pub fn index(mut self, index: IndexDefinition) -> Self {
        self.indexes.push(index);
        self
    }

    /// Add multiple indexes
    pub fn indexes(mut self, indexes: Vec<IndexDefinition>) -> Self {
        self.indexes.extend(indexes);
        self
    }

    /// Build the table schema
    pub fn build(self) -> TableSchema {
        TableSchema {
            name: self.name,
            schema_name: self.schema_name,
            columns: self.columns,
            indexes: self.indexes,
            primary_key: self.primary_key,
            primary_keys: self.primary_keys,
        }
    }
}

impl ColumnSchema {
    /// Create a new column schema
    pub fn new(name: impl Into<String>, sql_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sql_type: sql_type.into(),
            nullable: true,
            default: None,
            primary_key: false,
            auto_increment: false,
        }
    }

    /// Mark as primary key
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    /// Mark as auto increment
    pub fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }

    /// Mark as not nullable
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// Set default value
    pub fn default(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }
}

/// Utility to map Rust types to SQL types
pub fn rust_type_to_sql(rust_type: &str, db_type: DatabaseType) -> String {
    // Normalize by removing whitespace first (handles "Option < i64 >" from stringify!)
    let normalized: String = rust_type.chars().filter(|c| !c.is_whitespace()).collect();

    // Unwrap Option<T> → T, but preserve inner generics like Vec<i32>
    let base_type = if normalized.starts_with("Option<") && normalized.ends_with(">") {
        // Strip "Option<" prefix and last ">"
        normalized[7..normalized.len() - 1].to_string()
    } else {
        normalized
    };

    let base_type = base_type
        .replace("&", "")
        .replace("'static", "")
        .trim()
        .to_string();

    match db_type {
        DatabaseType::Postgres => match base_type.as_str() {
            "i8" | "i16" => "SMALLINT".to_string(),
            "i32" => "INTEGER".to_string(),
            "i64" => "BIGINT".to_string(),
            "u8" | "u16" => "SMALLINT".to_string(),
            "u32" => "INTEGER".to_string(),
            "u64" => "BIGINT".to_string(),
            "f32" => "REAL".to_string(),
            "f64" => "DOUBLE PRECISION".to_string(),
            "bool" => "BOOLEAN".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "UUID".to_string(),
            // DateTime<Utc> uses TIMESTAMPTZ (timestamp with time zone)
            "DateTime<Utc>" | "chrono::DateTime<Utc>" | "chrono::DateTime<chrono::Utc>" => {
                "TIMESTAMPTZ".to_string()
            }
            // NaiveDateTime uses TIMESTAMP (without time zone)
            "DateTime" | "NaiveDateTime" => "TIMESTAMP".to_string(),
            "NaiveDate" => "DATE".to_string(),
            "NaiveTime" => "TIME".to_string(),
            "Decimal" => "DECIMAL".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "JSONB".to_string(),
            "Vec<u8>" => "BYTEA".to_string(),
            // Array types
            "Vec<i32>" | "IntArray" => "INTEGER[]".to_string(),
            "Vec<i64>" | "BigIntArray" => "BIGINT[]".to_string(),
            "Vec<String>" | "TextArray" => "TEXT[]".to_string(),
            "Vec<bool>" | "BoolArray" => "BOOLEAN[]".to_string(),
            "Vec<f64>" | "FloatArray" => "DOUBLE PRECISION[]".to_string(),
            "Vec<serde_json::Value>" | "JsonArray" => "JSONB[]".to_string(),
            _ => "TEXT".to_string(),
        },
        DatabaseType::MySQL | DatabaseType::MariaDB => match base_type.as_str() {
            "i8" | "i16" => "SMALLINT".to_string(),
            "i32" => "INT".to_string(),
            "i64" => "BIGINT".to_string(),
            "u8" | "u16" => "SMALLINT UNSIGNED".to_string(),
            "u32" => "INT UNSIGNED".to_string(),
            "u64" => "BIGINT UNSIGNED".to_string(),
            "f32" => "FLOAT".to_string(),
            "f64" => "DOUBLE".to_string(),
            "bool" => "TINYINT(1)".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "CHAR(36)".to_string(),
            "DateTime<Utc>" | "DateTime" | "NaiveDateTime" => "DATETIME".to_string(),
            "NaiveDate" => "DATE".to_string(),
            "NaiveTime" => "TIME".to_string(),
            "Decimal" => "DECIMAL(65,30)".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "JSON".to_string(),
            "Vec<u8>" => "BLOB".to_string(),
            // Array types stored as JSON in MySQL/MariaDB
            "Vec<i32>" | "IntArray" => "JSON".to_string(),
            "Vec<i64>" | "BigIntArray" => "JSON".to_string(),
            "Vec<String>" | "TextArray" => "JSON".to_string(),
            "Vec<bool>" | "BoolArray" => "JSON".to_string(),
            "Vec<f64>" | "FloatArray" => "JSON".to_string(),
            "Vec<serde_json::Value>" | "JsonArray" => "JSON".to_string(),
            _ => "TEXT".to_string(),
        },
        DatabaseType::SQLite => match base_type.as_str() {
            "i8" | "i16" | "i32" | "i64" => "INTEGER".to_string(),
            "u8" | "u16" | "u32" | "u64" => "INTEGER".to_string(),
            "f32" | "f64" => "REAL".to_string(),
            "bool" => "INTEGER".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "TEXT".to_string(),
            "DateTime<Utc>" | "DateTime" | "NaiveDateTime" | "NaiveDate" | "NaiveTime" => {
                "TEXT".to_string()
            }
            "Decimal" => "TEXT".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "TEXT".to_string(),
            "Vec<u8>" => "BLOB".to_string(),
            _ => "TEXT".to_string(),
        },
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
#[path = "testing/schema_tests.rs"]
mod tests;

// =============================================================================
// SCHEMA WRITER - Auto-generate schema.sql
// =============================================================================

/// Schema writer for auto-generating schema files
pub struct SchemaWriter;

impl SchemaWriter {
    /// Register a table schema for generation
    ///
    /// Called automatically by the Model derive macro
    pub fn register_schema(schema: TableSchema) {
        let mut registry = SCHEMA_REGISTRY.write();
        // Check if table already exists (avoid duplicates)
        if !registry
            .iter()
            .any(|t| t.name == schema.name && t.schema_name == schema.schema_name)
        {
            registry.push(schema);
        }
    }

    /// Generate schema SQL and write it to a file.
    pub async fn write_schema<P: AsRef<Path>>(path: P) -> Result<()> {
        let db_type =
            crate::config::TideConfig::get_database_type().unwrap_or(DatabaseType::Postgres);
        let schemas = SCHEMA_REGISTRY.read().clone();

        if schemas.is_empty() {
            // No schemas registered, generate from database introspection
            return Self::write_schema_from_db(path).await;
        }

        let mut generator = SchemaGenerator::new(db_type);
        for schema in schemas {
            generator.add_table(schema);
        }

        let sql = generator.generate();

        fs::write(path.as_ref(), sql)
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))?;

        Ok(())
    }

    /// Generate schema from current database state (introspection)
    pub async fn write_schema_from_db<P: AsRef<Path>>(path: P) -> Result<()> {
        let db_type =
            crate::config::TideConfig::get_database_type().unwrap_or(DatabaseType::Postgres);

        let tables = match db_type {
            DatabaseType::Postgres => Self::introspect_postgres().await?,
            DatabaseType::MySQL | DatabaseType::MariaDB => Self::introspect_mysql().await?,
            DatabaseType::SQLite => Self::introspect_sqlite().await?,
        };

        let mut generator = SchemaGenerator::new(db_type);
        for table in tables {
            generator.add_table(table);
        }

        let sql = generator.generate();

        fs::write(path.as_ref(), sql)
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))?;

        Ok(())
    }

    /// Introspect PostgreSQL database
    async fn introspect_postgres() -> Result<Vec<TableSchema>> {
        use sea_orm::{ConnectionTrait, DbBackend, TryGetable};

        let conn = crate::require_db()?.__internal_connection()?;

        // Get all tables
        let table_rows = conn
            .query_all_raw(crate::internal::build_statement(
                DbBackend::Postgres,
                "SELECT table_schema, table_name FROM information_schema.tables 
             WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
             AND table_schema NOT LIKE 'pg_toast%'
             AND table_schema NOT LIKE 'pg_temp_%'
             AND table_type = 'BASE TABLE'
             ORDER BY table_schema, table_name",
            ))
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let mut schemas = Vec::new();

        for row in table_rows {
            let table_schema: String = row
                .try_get("", "table_schema")
                .map_err(|e| Error::query(e.to_string()))?;
            let table_name: String = row
                .try_get("", "table_name")
                .map_err(|e| Error::query(e.to_string()))?;

            // Get columns
            let col_rows = conn
                .query_all_raw(crate::internal::build_statement_with_values(
                    DbBackend::Postgres,
                    "SELECT column_name, data_type, is_nullable, column_default
                 FROM information_schema.columns
                 WHERE table_schema = $1 AND table_name = $2
                 ORDER BY ordinal_position",
                    vec![table_schema.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            // Get primary key
            let pk_rows = conn
                .query_all_raw(crate::internal::build_statement_with_values(
                    DbBackend::Postgres,
                    "SELECT c.column_name
                 FROM information_schema.table_constraints tc
                 JOIN information_schema.constraint_column_usage AS ccu 
                     ON ccu.constraint_name = tc.constraint_name
                     AND ccu.constraint_schema = tc.constraint_schema
                     AND ccu.table_schema = tc.table_schema
                     AND ccu.table_name = tc.table_name
                 JOIN information_schema.columns AS c 
                     ON c.table_schema = ccu.table_schema AND c.table_name = ccu.table_name AND c.column_name = ccu.column_name
                 WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_schema = $1 AND tc.table_name = $2",
                    vec![table_schema.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let pk_column = pk_rows
                .first()
                .and_then(|r| String::try_get(r, "", "column_name").ok())
                .unwrap_or_default();

            // Get indexes
            let index_rows = conn
                .query_all_raw(crate::internal::build_statement_with_values(
                    DbBackend::Postgres,
                    "SELECT i.relname as index_name, ix.indisunique, a.attname as column_name
                 FROM pg_class t
                      JOIN pg_namespace ns ON ns.oid = t.relnamespace
                 JOIN pg_index ix ON t.oid = ix.indrelid
                 JOIN pg_class i ON i.oid = ix.indexrelid
                 JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(ix.indkey)
                      WHERE t.relkind = 'r' AND ns.nspname = $1 AND t.relname = $2
                 AND NOT ix.indisprimary
                 ORDER BY i.relname, a.attnum",
                    vec![table_schema.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            // Group index columns
            let mut index_map: std::collections::HashMap<String, (bool, Vec<String>)> =
                std::collections::HashMap::new();
            for row in index_rows {
                let idx_name: String = row.try_get("", "index_name").unwrap_or_default();
                let is_unique: bool = row.try_get("", "indisunique").unwrap_or(false);
                let col_name: String = row.try_get("", "column_name").unwrap_or_default();

                index_map
                    .entry(idx_name)
                    .or_insert((is_unique, Vec::new()))
                    .1
                    .push(col_name);
            }

            let indexes: Vec<IndexDefinition> = index_map
                .into_iter()
                .map(|(name, (unique, cols))| IndexDefinition::new(name, cols, unique))
                .collect();

            // Build table schema
            let mut builder = TableSchemaBuilder::new(&table_name).schema(&table_schema);

            for row in col_rows {
                let col_name: String = row.try_get("", "column_name").unwrap_or_default();
                let data_type: String = row.try_get("", "data_type").unwrap_or_default();
                let is_nullable: String = row.try_get("", "is_nullable").unwrap_or_default();
                let default: Option<String> = row.try_get("", "column_default").ok();

                let sql_type = data_type.to_uppercase();
                let mut col = ColumnSchema::new(&col_name, &sql_type);

                if col_name == pk_column {
                    col = col.primary_key();
                    if sql_type.contains("SERIAL")
                        || default
                            .as_ref()
                            .map(|d| d.contains("nextval"))
                            .unwrap_or(false)
                    {
                        col = col.auto_increment();
                    }
                }

                if is_nullable == "NO" {
                    col = col.not_null();
                }

                if let Some(def) = default {
                    if !def.contains("nextval") {
                        col = col.default(def);
                    }
                }

                builder = builder.column(col);
            }

            builder = builder.indexes(indexes);
            schemas.push(builder.build());
        }

        Ok(schemas)
    }

    /// Introspect MySQL database
    async fn introspect_mysql() -> Result<Vec<TableSchema>> {
        use sea_orm::{ConnectionTrait, DbBackend};

        let conn = crate::require_db()?.__internal_connection()?;

        // Get database name from connection (we'll use information_schema)
        let db_name_row = conn
            .query_one_raw(crate::internal::build_statement(
                DbBackend::MySql,
                "SELECT DATABASE() as db_name",
            ))
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let db_name: String = db_name_row
            .and_then(|r| r.try_get("", "db_name").ok())
            .unwrap_or_default();

        if db_name.is_empty() {
            return Ok(Vec::new());
        }

        // Get all tables
        let table_rows = conn
            .query_all_raw(crate::internal::build_statement_with_values(
                DbBackend::MySql,
                "SELECT table_name FROM information_schema.tables 
             WHERE table_schema = ? AND table_type = 'BASE TABLE'
             ORDER BY table_name",
                vec![db_name.clone().into()],
            ))
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let mut schemas = Vec::new();

        for row in table_rows {
            let table_name: String = row
                .try_get("", "table_name")
                .or_else(|_| row.try_get("", "TABLE_NAME"))
                .map_err(|e| Error::query(e.to_string()))?;

            // Get columns
            let col_rows = conn.query_all_raw(crate::internal::build_statement_with_values(
                DbBackend::MySql,
                "SELECT column_name, column_type, is_nullable, column_default, column_key, extra
                 FROM information_schema.columns
                 WHERE table_schema = ? AND table_name = ?
                 ORDER BY ordinal_position",
                vec![db_name.clone().into(), table_name.clone().into()]
            )).await.map_err(|e| Error::query(e.to_string()))?;

            // Get indexes
            let index_rows = conn
                .query_all_raw(crate::internal::build_statement_with_values(
                    DbBackend::MySql,
                    "SELECT index_name, non_unique, column_name
                 FROM information_schema.statistics
                 WHERE table_schema = ? AND table_name = ?
                 AND index_name != 'PRIMARY'
                 ORDER BY index_name, seq_in_index",
                    vec![db_name.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            // Group index columns
            let mut index_map: std::collections::HashMap<String, (bool, Vec<String>)> =
                std::collections::HashMap::new();
            for row in index_rows {
                let idx_name: String = row
                    .try_get("", "index_name")
                    .or_else(|_| row.try_get("", "INDEX_NAME"))
                    .unwrap_or_default();
                let non_unique: i32 = row
                    .try_get("", "non_unique")
                    .or_else(|_| row.try_get("", "NON_UNIQUE"))
                    .unwrap_or(1);
                let col_name: String = row
                    .try_get("", "column_name")
                    .or_else(|_| row.try_get("", "COLUMN_NAME"))
                    .unwrap_or_default();

                index_map
                    .entry(idx_name)
                    .or_insert((non_unique == 0, Vec::new()))
                    .1
                    .push(col_name);
            }

            let indexes: Vec<IndexDefinition> = index_map
                .into_iter()
                .map(|(name, (unique, cols))| IndexDefinition::new(name, cols, unique))
                .collect();

            // Build table schema
            let mut builder = TableSchemaBuilder::new(&table_name);
            let mut pk_column = String::new();

            for row in col_rows {
                let col_name: String = row
                    .try_get("", "column_name")
                    .or_else(|_| row.try_get("", "COLUMN_NAME"))
                    .unwrap_or_default();
                let col_type: String = row
                    .try_get("", "column_type")
                    .or_else(|_| row.try_get("", "COLUMN_TYPE"))
                    .unwrap_or_default();
                let is_nullable: String = row
                    .try_get("", "is_nullable")
                    .or_else(|_| row.try_get("", "IS_NULLABLE"))
                    .unwrap_or_default();
                let default: Option<String> = row
                    .try_get("", "column_default")
                    .or_else(|_| row.try_get("", "COLUMN_DEFAULT"))
                    .ok();
                let col_key: String = row
                    .try_get("", "column_key")
                    .or_else(|_| row.try_get("", "COLUMN_KEY"))
                    .unwrap_or_default();
                let extra: String = row
                    .try_get("", "extra")
                    .or_else(|_| row.try_get("", "EXTRA"))
                    .unwrap_or_default();

                let sql_type = col_type.to_uppercase();
                let mut col = ColumnSchema::new(&col_name, &sql_type);

                if col_key == "PRI" {
                    col = col.primary_key();
                    pk_column = col_name.clone();
                    if extra.contains("auto_increment") {
                        col = col.auto_increment();
                    }
                }

                if is_nullable == "NO" {
                    col = col.not_null();
                }

                if let Some(def) = default {
                    col = col.default(def);
                }

                builder = builder.column(col);
            }

            let _ = pk_column; // Used implicitly via primary_key() call
            builder = builder.indexes(indexes);
            schemas.push(builder.build());
        }

        Ok(schemas)
    }

    /// Introspect SQLite database
    async fn introspect_sqlite() -> Result<Vec<TableSchema>> {
        use sea_orm::{ConnectionTrait, DbBackend};

        let conn = crate::require_db()?.__internal_connection()?;

        // Get all tables
        let table_rows = conn
            .query_all_raw(crate::internal::build_statement(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master 
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
            ))
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let mut schemas = Vec::new();

        for row in table_rows {
            let table_name: String = row
                .try_get("", "name")
                .map_err(|e| Error::query(e.to_string()))?;
            let quoted_table_name = quote_ident(DatabaseType::SQLite, &table_name);

            // Get table info (columns)
            let col_rows = conn
                .query_all_raw(crate::internal::build_statement(
                    DbBackend::Sqlite,
                    format!("PRAGMA table_info({})", quoted_table_name),
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            // Get indexes
            let index_list = conn
                .query_all_raw(crate::internal::build_statement(
                    DbBackend::Sqlite,
                    format!("PRAGMA index_list({})", quoted_table_name),
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let mut indexes = Vec::new();
            for idx_row in index_list {
                let idx_name: String = idx_row.try_get("", "name").unwrap_or_default();
                let is_unique: i32 = idx_row.try_get("", "unique").unwrap_or(0);
                let origin: String = idx_row.try_get("", "origin").unwrap_or_default();

                // Skip auto-created indexes (primary key)
                if origin == "pk" {
                    continue;
                }

                // Get columns for this index
                let idx_info = conn
                    .query_all_raw(crate::internal::build_statement(
                        DbBackend::Sqlite,
                        format!(
                            "PRAGMA index_info({})",
                            quote_ident(DatabaseType::SQLite, &idx_name)
                        ),
                    ))
                    .await
                    .map_err(|e| Error::query(e.to_string()))?;

                let columns: Vec<String> = idx_info
                    .iter()
                    .filter_map(|r| r.try_get("", "name").ok())
                    .collect();

                if !columns.is_empty() {
                    indexes.push(IndexDefinition::new(idx_name, columns, is_unique == 1));
                }
            }
            let mut builder = TableSchemaBuilder::new(&table_name);

            for row in col_rows {
                let col_name: String = row.try_get("", "name").unwrap_or_default();
                let col_type: String = row.try_get("", "type").unwrap_or_default();
                let notnull: i32 = row.try_get("", "notnull").unwrap_or(0);
                let default: Option<String> = row.try_get("", "dflt_value").ok();
                let pk: i32 = row.try_get("", "pk").unwrap_or(0);

                let sql_type = col_type.to_uppercase();
                let mut col = ColumnSchema::new(&col_name, &sql_type);

                if pk > 0 {
                    col = col.primary_key();
                    // SQLite INTEGER PRIMARY KEY is auto-increment by default
                    if sql_type == "INTEGER" {
                        col = col.auto_increment();
                    }
                }

                if notnull == 1 {
                    col = col.not_null();
                }

                if let Some(def) = default {
                    col = col.default(def);
                }
                builder = builder.column(col);
            }

            builder = builder.indexes(indexes);
            schemas.push(builder.build());
        }

        Ok(schemas)
    }

    /// Get the currently registered schemas
    pub fn get_registered_schemas() -> Vec<TableSchema> {
        SCHEMA_REGISTRY.read().clone()
    }

    /// Clear the schema registry
    pub fn clear_registry() {
        SCHEMA_REGISTRY.write().clear();
    }
}
