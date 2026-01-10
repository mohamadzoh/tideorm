//! Schema generation module
//!
//! This module provides functionality to generate SQL schema files
//! from TideORM model definitions.
//!
//! ## Usage
//!
//! Schema generation is configured via TideConfig:
//!
//! ```ignore
//! TideConfig::init()
//!     .database("postgres://localhost/myapp")
//!     .schema_file("schema.sql")  // Auto-generate schema file on connect
//!     .connect()
//!     .await?;
//! ```
//!
//! Or generate manually:
//!
//! ```ignore
//! use tideorm::schema::SchemaWriter;
//!
//! // Write schema for registered models
//! SchemaWriter::write_schema("schema.sql").await?;
//! ```
//!
//! ## Index Definitions
//!
//! Define indexes using attribute macros:
//!
//! ```ignore
//! #[derive(Model)]
//! #[tide(
//!     table = "users",
//!     index = "email;name:first_name,last_name",
//!     unique_index = "tenant_id,email"
//! )]
//! struct User { ... }
//! ```

use std::path::Path;
use std::sync::RwLock;

use crate::config::DatabaseType;
use crate::model::IndexDefinition;
use crate::error::{Error, Result};

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
    /// Column definitions
    pub columns: Vec<ColumnSchema>,
    /// Index definitions (regular and unique)
    pub indexes: Vec<IndexDefinition>,
    /// Primary key column name
    pub primary_key: String,
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
        sql.push_str(&format!("-- Generated at: {}\n\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
        
        // Generate CREATE TABLE statements
        for table in &self.tables {
            sql.push_str(&self.generate_create_table(table));
            sql.push_str("\n");
        }
        
        // Generate CREATE INDEX statements
        for table in &self.tables {
            let indexes = self.generate_indexes(table);
            if !indexes.is_empty() {
                sql.push_str(&indexes);
                sql.push_str("\n");
            }
        }
        
        sql
    }
    
    /// Generate CREATE TABLE statement
    fn generate_create_table(&self, table: &TableSchema) -> String {
        let mut sql = format!("CREATE TABLE IF NOT EXISTS {} (\n", self.quote_identifier(&table.name));
        
        let column_defs: Vec<String> = table.columns.iter()
            .map(|col| self.generate_column_def(col))
            .collect();
        
        sql.push_str(&column_defs.join(",\n"));
        
        // Add primary key constraint if not inline
        if !table.primary_key.is_empty() {
            sql.push_str(",\n");
            sql.push_str(&format!("    PRIMARY KEY ({})", self.quote_identifier(&table.primary_key)));
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
                DatabaseType::MySQL => {
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
            let index_type = if index.unique { "UNIQUE INDEX" } else { "INDEX" };
            let columns: Vec<String> = index.columns.iter()
                .map(|c| self.quote_identifier(c))
                .collect();
            
            sql.push_str(&format!(
                "CREATE {} IF NOT EXISTS {} ON {} ({});\n",
                index_type,
                self.quote_identifier(&index.name),
                self.quote_identifier(&table.name),
                columns.join(", ")
            ));
        }
        
        sql
    }
    
    /// Quote identifier based on database type
    fn quote_identifier(&self, name: &str) -> String {
        match self.database_type {
            DatabaseType::Postgres => format!("\"{}\"", name),
            DatabaseType::MySQL => format!("`{}`", name),
            DatabaseType::SQLite => format!("\"{}\"", name),
        }
    }
}

/// Builder for table schemas from model metadata
pub struct TableSchemaBuilder {
    name: String,
    columns: Vec<ColumnSchema>,
    indexes: Vec<IndexDefinition>,
    primary_key: String,
}

impl TableSchemaBuilder {
    /// Create a new table schema builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            columns: Vec::new(),
            indexes: Vec::new(),
            primary_key: String::new(),
        }
    }
    
    /// Add a column
    pub fn column(mut self, schema: ColumnSchema) -> Self {
        if schema.primary_key {
            self.primary_key = schema.name.clone();
        }
        self.columns.push(schema);
        self
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
            columns: self.columns,
            indexes: self.indexes,
            primary_key: self.primary_key,
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
    
    let base_type = normalized
        .replace("Option<", "")
        .replace(">", "")
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
            "DateTime<Utc>" | "DateTime" | "NaiveDateTime" => "TIMESTAMP".to_string(),
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
        DatabaseType::MySQL => match base_type.as_str() {
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
            _ => "TEXT".to_string(),
        },
        DatabaseType::SQLite => match base_type.as_str() {
            "i8" | "i16" | "i32" | "i64" => "INTEGER".to_string(),
            "u8" | "u16" | "u32" | "u64" => "INTEGER".to_string(),
            "f32" | "f64" => "REAL".to_string(),
            "bool" => "INTEGER".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "TEXT".to_string(),
            "DateTime<Utc>" | "DateTime" | "NaiveDateTime" | "NaiveDate" | "NaiveTime" => "TEXT".to_string(),
            "Decimal" => "TEXT".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "TEXT".to_string(),
            "Vec<u8>" => "BLOB".to_string(),
            _ => "TEXT".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_index_definition_parse() {
        // Single field
        let indexes = IndexDefinition::parse("users", "email", false);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].columns, vec!["email"]);
        assert!(!indexes[0].unique);
        
        // Multiple fields
        let indexes = IndexDefinition::parse("users", "first_name,last_name", false);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].columns, vec!["first_name", "last_name"]);
        
        // Named index
        let indexes = IndexDefinition::parse("users", "my_idx:email", false);
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].name, "my_idx");
        assert_eq!(indexes[0].columns, vec!["email"]);
        
        // Multiple indexes
        let indexes = IndexDefinition::parse("users", "email;name:first_name,last_name", false);
        assert_eq!(indexes.len(), 2);
    }
    
    #[test]
    fn test_schema_generation() {
        let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
        
        let table = TableSchemaBuilder::new("users")
            .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
            .column(ColumnSchema::new("email", "TEXT").not_null())
            .column(ColumnSchema::new("name", "TEXT"))
            .index(IndexDefinition::new("idx_users_email", vec!["email".to_string()], false))
            .index(IndexDefinition::new("uidx_users_email", vec!["email".to_string()], true))
            .build();
        
        generator.add_table(table);
        
        let sql = generator.generate();
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS"));
        assert!(sql.contains("CREATE INDEX IF NOT EXISTS"));
        assert!(sql.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
    }
    
    #[test]
    fn test_schema_generator_postgres() {
        let mut generator = SchemaGenerator::new(DatabaseType::Postgres);
        
        let table = TableSchemaBuilder::new("products")
            .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
            .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
            .column(ColumnSchema::new("price", "DECIMAL(10,2)").not_null().default("0.00"))
            .column(ColumnSchema::new("description", "TEXT"))
            .column(ColumnSchema::new("created_at", "TIMESTAMPTZ").not_null().default("NOW()"))
            .build();
        
        generator.add_table(table);
        
        let sql = generator.generate();
        
        // Check PostgreSQL-specific syntax
        assert!(sql.contains("\"products\""));  // Postgres uses double quotes
        assert!(sql.contains("BIGSERIAL"));     // Postgres auto-increment
        assert!(sql.contains("NOT NULL"));
        assert!(sql.contains("DEFAULT"));
    }
    
    #[test]
    fn test_schema_generator_mysql() {
        let mut generator = SchemaGenerator::new(DatabaseType::MySQL);
        
        let table = TableSchemaBuilder::new("products")
            .column(ColumnSchema::new("id", "BIGINT").primary_key().auto_increment())
            .column(ColumnSchema::new("name", "VARCHAR(255)").not_null())
            .build();
        
        generator.add_table(table);
        
        let sql = generator.generate();
        
        // Check MySQL-specific syntax
        assert!(sql.contains("`products`"));    // MySQL uses backticks
        assert!(sql.contains("AUTO_INCREMENT")); // MySQL auto-increment syntax
    }
    
    #[test]
    fn test_schema_generator_sqlite() {
        let mut generator = SchemaGenerator::new(DatabaseType::SQLite);
        
        let table = TableSchemaBuilder::new("products")
            .column(ColumnSchema::new("id", "INTEGER").primary_key().auto_increment())
            .column(ColumnSchema::new("name", "TEXT").not_null())
            .build();
        
        generator.add_table(table);
        
        let sql = generator.generate();
        
        // Check SQLite-specific syntax
        assert!(sql.contains("\"products\""));  // SQLite uses double quotes
        assert!(sql.contains("INTEGER"));
    }
    
    #[test]
    fn test_column_schema_builder() {
        let col = ColumnSchema::new("email", "VARCHAR(255)")
            .not_null()
            .default("''");
        
        assert_eq!(col.name, "email");
        assert_eq!(col.sql_type, "VARCHAR(255)");
        assert!(!col.nullable);
        assert_eq!(col.default, Some("''".to_string()));
        assert!(!col.primary_key);
        assert!(!col.auto_increment);
    }
    
    #[test]
    fn test_column_schema_primary_key() {
        let col = ColumnSchema::new("id", "BIGINT")
            .primary_key()
            .auto_increment();
        
        assert!(col.primary_key);
        assert!(col.auto_increment);
        assert!(!col.nullable);  // primary_key sets nullable to false
    }
    
    #[test]
    fn test_table_schema_builder() {
        let table = TableSchemaBuilder::new("users")
            .column(ColumnSchema::new("id", "BIGINT").primary_key())
            .column(ColumnSchema::new("email", "TEXT").not_null())
            .index(IndexDefinition::new("idx_email", vec!["email".to_string()], false))
            .build();
        
        assert_eq!(table.name, "users");
        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.indexes.len(), 1);
        assert_eq!(table.primary_key, "id");
    }
    
    #[test]
    fn test_table_schema_multiple_indexes() {
        let indexes = vec![
            IndexDefinition::new("idx_email", vec!["email".to_string()], false),
            IndexDefinition::new("idx_name", vec!["first_name".to_string(), "last_name".to_string()], false),
            IndexDefinition::new("uidx_email", vec!["email".to_string()], true),
        ];
        
        let table = TableSchemaBuilder::new("users")
            .column(ColumnSchema::new("id", "BIGINT").primary_key())
            .indexes(indexes)
            .build();
        
        assert_eq!(table.indexes.len(), 3);
    }
    
    #[test]
    fn test_rust_type_to_sql_postgres() {
        assert_eq!(rust_type_to_sql("i64", DatabaseType::Postgres), "BIGINT");
        assert_eq!(rust_type_to_sql("i32", DatabaseType::Postgres), "INTEGER");
        assert_eq!(rust_type_to_sql("String", DatabaseType::Postgres), "TEXT");
        assert_eq!(rust_type_to_sql("bool", DatabaseType::Postgres), "BOOLEAN");
        assert_eq!(rust_type_to_sql("f64", DatabaseType::Postgres), "DOUBLE PRECISION");
        assert_eq!(rust_type_to_sql("Option<i64>", DatabaseType::Postgres), "BIGINT");
        assert_eq!(rust_type_to_sql("serde_json::Value", DatabaseType::Postgres), "JSONB");
    }
    
    #[test]
    fn test_rust_type_to_sql_mysql() {
        assert_eq!(rust_type_to_sql("i64", DatabaseType::MySQL), "BIGINT");
        assert_eq!(rust_type_to_sql("bool", DatabaseType::MySQL), "TINYINT(1)");
        assert_eq!(rust_type_to_sql("f64", DatabaseType::MySQL), "DOUBLE");
        assert_eq!(rust_type_to_sql("Uuid", DatabaseType::MySQL), "CHAR(36)");
    }
    
    #[test]
    fn test_rust_type_to_sql_sqlite() {
        assert_eq!(rust_type_to_sql("i64", DatabaseType::SQLite), "INTEGER");
        assert_eq!(rust_type_to_sql("i32", DatabaseType::SQLite), "INTEGER");
        assert_eq!(rust_type_to_sql("bool", DatabaseType::SQLite), "INTEGER");
        assert_eq!(rust_type_to_sql("f64", DatabaseType::SQLite), "REAL");
        assert_eq!(rust_type_to_sql("String", DatabaseType::SQLite), "TEXT");
    }
    
    #[test]
    fn test_schema_generator_header() {
        let generator = SchemaGenerator::new(DatabaseType::Postgres);
        let sql = generator.generate();
        
        assert!(sql.contains("-- TideORM Generated Schema"));
        assert!(sql.contains("-- Database:"));
        assert!(sql.contains("-- Generated at:"));
    }
    
    #[test]
    fn test_schema_writer_registry() {
        // Clear any existing registrations
        SchemaWriter::clear_registry();
        
        // Register a schema
        let table = TableSchemaBuilder::new("test_table")
            .column(ColumnSchema::new("id", "BIGINT").primary_key())
            .build();
        
        SchemaWriter::register_schema(table.clone());
        
        let schemas = SchemaWriter::get_registered_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "test_table");
        
        // Register same table again (should not duplicate)
        SchemaWriter::register_schema(table);
        let schemas = SchemaWriter::get_registered_schemas();
        assert_eq!(schemas.len(), 1);
        
        // Clear registry
        SchemaWriter::clear_registry();
        let schemas = SchemaWriter::get_registered_schemas();
        assert!(schemas.is_empty());
    }
}

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
        if let Ok(mut registry) = SCHEMA_REGISTRY.write() {
            // Check if table already exists (avoid duplicates)
            if !registry.iter().any(|t| t.name == schema.name) {
                registry.push(schema);
            }
        }
    }
    
    /// Generate and write schema to file
    ///
    /// # Example
    /// ```rust,ignore
    /// SchemaWriter::write_schema("schema.sql").await?;
    /// ```
    pub async fn write_schema<P: AsRef<Path>>(path: P) -> Result<()> {
        let db_type = crate::config::TideConfig::get_database_type()
            .unwrap_or(DatabaseType::Postgres);
        let schemas = SCHEMA_REGISTRY.read()
            .map_err(|e| Error::internal(format!("Failed to read schema registry: {}", e)))?
            .clone();
        
        if schemas.is_empty() {
            // No schemas registered, generate from database introspection
            return Self::write_schema_from_db(path).await;
        }
        
        let mut generator = SchemaGenerator::new(db_type);
        for schema in schemas {
            generator.add_table(schema);
        }
        
        let sql = generator.generate();
        
        tokio::fs::write(path.as_ref(), sql)
            .await
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))?;
        
        Ok(())
    }
    
    /// Generate schema from current database state (introspection)
    pub async fn write_schema_from_db<P: AsRef<Path>>(path: P) -> Result<()> {
        let db_type = crate::config::TideConfig::get_database_type()
            .unwrap_or(DatabaseType::Postgres);
        
        let tables = match db_type {
            DatabaseType::Postgres => Self::introspect_postgres().await?,
            DatabaseType::MySQL => Self::introspect_mysql().await?,
            DatabaseType::SQLite => Self::introspect_sqlite().await?,
        };
        
        let mut generator = SchemaGenerator::new(db_type);
        for table in tables {
            generator.add_table(table);
        }
        
        let sql = generator.generate();
        
        tokio::fs::write(path.as_ref(), sql)
            .await
            .map_err(|e| Error::internal(format!("Failed to write schema file: {}", e)))?;
        
        Ok(())
    }
    
    /// Introspect PostgreSQL database
    async fn introspect_postgres() -> Result<Vec<TableSchema>> {
        use sea_orm::{ConnectionTrait, Statement, DbBackend, TryGetable};
        
        let conn = crate::db().__internal_connection();
        
        // Get all tables
        let table_rows = conn.query_all(Statement::from_string(
            DbBackend::Postgres,
            "SELECT table_name FROM information_schema.tables 
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
             ORDER BY table_name"
        )).await.map_err(|e| Error::query(e.to_string()))?;
        
        let mut schemas = Vec::new();
        
        for row in table_rows {
            let table_name: String = row.try_get("", "table_name")
                .map_err(|e| Error::query(e.to_string()))?;
            
            // Get columns
            let col_rows = conn.query_all(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT column_name, data_type, is_nullable, column_default
                 FROM information_schema.columns
                 WHERE table_schema = 'public' AND table_name = $1
                 ORDER BY ordinal_position",
                vec![table_name.clone().into()]
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
            // Get primary key
            let pk_rows = conn.query_all(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT c.column_name
                 FROM information_schema.table_constraints tc
                 JOIN information_schema.constraint_column_usage AS ccu 
                     ON ccu.constraint_name = tc.constraint_name
                 JOIN information_schema.columns AS c 
                     ON c.table_name = tc.table_name AND ccu.column_name = c.column_name
                 WHERE tc.constraint_type = 'PRIMARY KEY' AND tc.table_name = $1",
                vec![table_name.clone().into()]
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
            let pk_column = pk_rows.first()
                .and_then(|r| {
                    String::try_get(r, "", "column_name").ok()
                })
                .unwrap_or_default();
            
            // Get indexes
            let index_rows = conn.query_all(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT i.relname as index_name, ix.indisunique, a.attname as column_name
                 FROM pg_class t
                 JOIN pg_index ix ON t.oid = ix.indrelid
                 JOIN pg_class i ON i.oid = ix.indexrelid
                 JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(ix.indkey)
                 WHERE t.relkind = 'r' AND t.relname = $1
                 AND NOT ix.indisprimary
                 ORDER BY i.relname, a.attnum",
                vec![table_name.clone().into()]
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
            // Group index columns
            let mut index_map: std::collections::HashMap<String, (bool, Vec<String>)> = std::collections::HashMap::new();
            for row in index_rows {
                let idx_name: String = row.try_get("", "index_name").unwrap_or_default();
                let is_unique: bool = row.try_get("", "indisunique").unwrap_or(false);
                let col_name: String = row.try_get("", "column_name").unwrap_or_default();
                
                index_map.entry(idx_name)
                    .or_insert((is_unique, Vec::new()))
                    .1.push(col_name);
            }
            
            let indexes: Vec<IndexDefinition> = index_map
                .into_iter()
                .map(|(name, (unique, cols))| IndexDefinition::new(name, cols, unique))
                .collect();
            
            // Build table schema
            let mut builder = TableSchemaBuilder::new(&table_name);
            
            for row in col_rows {
                let col_name: String = row.try_get("", "column_name").unwrap_or_default();
                let data_type: String = row.try_get("", "data_type").unwrap_or_default();
                let is_nullable: String = row.try_get("", "is_nullable").unwrap_or_default();
                let default: Option<String> = row.try_get("", "column_default").ok();
                
                let sql_type = data_type.to_uppercase();
                let mut col = ColumnSchema::new(&col_name, &sql_type);
                
                if col_name == pk_column {
                    col = col.primary_key();
                    if sql_type.contains("SERIAL") || default.as_ref().map(|d| d.contains("nextval")).unwrap_or(false) {
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
        use sea_orm::{ConnectionTrait, Statement, DbBackend};
        
        let conn = crate::db().__internal_connection();
        
        // Get database name from connection (we'll use information_schema)
        let db_name_row = conn.query_one(Statement::from_string(
            DbBackend::MySql,
            "SELECT DATABASE() as db_name"
        )).await.map_err(|e| Error::query(e.to_string()))?;
        
        let db_name: String = db_name_row
            .and_then(|r| r.try_get("", "db_name").ok())
            .unwrap_or_default();
        
        if db_name.is_empty() {
            return Ok(Vec::new());
        }
        
        // Get all tables
        let table_rows = conn.query_all(Statement::from_sql_and_values(
            DbBackend::MySql,
            "SELECT table_name FROM information_schema.tables 
             WHERE table_schema = ? AND table_type = 'BASE TABLE'
             ORDER BY table_name",
            vec![db_name.clone().into()]
        )).await.map_err(|e| Error::query(e.to_string()))?;
        
        let mut schemas = Vec::new();
        
        for row in table_rows {
            let table_name: String = row.try_get("", "table_name")
                .or_else(|_| row.try_get("", "TABLE_NAME"))
                .map_err(|e| Error::query(e.to_string()))?;
            
            // Get columns
            let col_rows = conn.query_all(Statement::from_sql_and_values(
                DbBackend::MySql,
                "SELECT column_name, column_type, is_nullable, column_default, column_key, extra
                 FROM information_schema.columns
                 WHERE table_schema = ? AND table_name = ?
                 ORDER BY ordinal_position",
                vec![db_name.clone().into(), table_name.clone().into()]
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
            // Get indexes
            let index_rows = conn.query_all(Statement::from_sql_and_values(
                DbBackend::MySql,
                "SELECT index_name, non_unique, column_name
                 FROM information_schema.statistics
                 WHERE table_schema = ? AND table_name = ?
                 AND index_name != 'PRIMARY'
                 ORDER BY index_name, seq_in_index",
                vec![db_name.clone().into(), table_name.clone().into()]
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
            // Group index columns
            let mut index_map: std::collections::HashMap<String, (bool, Vec<String>)> = std::collections::HashMap::new();
            for row in index_rows {
                let idx_name: String = row.try_get("", "index_name")
                    .or_else(|_| row.try_get("", "INDEX_NAME"))
                    .unwrap_or_default();
                let non_unique: i32 = row.try_get("", "non_unique")
                    .or_else(|_| row.try_get("", "NON_UNIQUE"))
                    .unwrap_or(1);
                let col_name: String = row.try_get("", "column_name")
                    .or_else(|_| row.try_get("", "COLUMN_NAME"))
                    .unwrap_or_default();
                
                index_map.entry(idx_name)
                    .or_insert((non_unique == 0, Vec::new()))
                    .1.push(col_name);
            }
            
            let indexes: Vec<IndexDefinition> = index_map
                .into_iter()
                .map(|(name, (unique, cols))| IndexDefinition::new(name, cols, unique))
                .collect();
            
            // Build table schema
            let mut builder = TableSchemaBuilder::new(&table_name);
            let mut pk_column = String::new();
            
            for row in col_rows {
                let col_name: String = row.try_get("", "column_name")
                    .or_else(|_| row.try_get("", "COLUMN_NAME"))
                    .unwrap_or_default();
                let col_type: String = row.try_get("", "column_type")
                    .or_else(|_| row.try_get("", "COLUMN_TYPE"))
                    .unwrap_or_default();
                let is_nullable: String = row.try_get("", "is_nullable")
                    .or_else(|_| row.try_get("", "IS_NULLABLE"))
                    .unwrap_or_default();
                let default: Option<String> = row.try_get("", "column_default")
                    .or_else(|_| row.try_get("", "COLUMN_DEFAULT"))
                    .ok();
                let col_key: String = row.try_get("", "column_key")
                    .or_else(|_| row.try_get("", "COLUMN_KEY"))
                    .unwrap_or_default();
                let extra: String = row.try_get("", "extra")
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
        use sea_orm::{ConnectionTrait, Statement, DbBackend};
        
        let conn = crate::db().__internal_connection();
        
        // Get all tables
        let table_rows = conn.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master 
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name"
        )).await.map_err(|e| Error::query(e.to_string()))?;
        
        let mut schemas = Vec::new();
        
        for row in table_rows {
            let table_name: String = row.try_get("", "name")
                .map_err(|e| Error::query(e.to_string()))?;
            
            // Get table info (columns)
            let col_rows = conn.query_all(Statement::from_string(
                DbBackend::Sqlite,
                format!("PRAGMA table_info(\"{}\")", table_name)
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
            // Get indexes
            let index_list = conn.query_all(Statement::from_string(
                DbBackend::Sqlite,
                format!("PRAGMA index_list(\"{}\")", table_name)
            )).await.map_err(|e| Error::query(e.to_string()))?;
            
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
                let idx_info = conn.query_all(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("PRAGMA index_info(\"{}\")", idx_name)
                )).await.map_err(|e| Error::query(e.to_string()))?;
                
                let columns: Vec<String> = idx_info.iter()
                    .filter_map(|r| r.try_get("", "name").ok())
                    .collect();
                
                if !columns.is_empty() {
                    indexes.push(IndexDefinition::new(idx_name, columns, is_unique == 1));
                }
            }
            
            // Build table schema
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
        SCHEMA_REGISTRY.read()
            .map(|r| r.clone())
            .unwrap_or_default()
    }
    
    /// Clear the schema registry
    pub fn clear_registry() {
        if let Ok(mut registry) = SCHEMA_REGISTRY.write() {
            registry.clear();
        }
    }
}
