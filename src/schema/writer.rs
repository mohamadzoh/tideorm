use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident;
use crate::internal::{
    Backend, ConnectionTrait, TryGetable, build_statement, build_statement_with_values,
};
use crate::model::IndexDefinition;

use super::{ColumnSchema, SCHEMA_REGISTRY, SchemaGenerator, TableSchema, TableSchemaBuilder};

/// Schema writer for auto-generating schema files
pub struct SchemaWriter;

impl SchemaWriter {
    /// Register a table schema for generation.
    ///
    /// Nothing calls this automatically: the derive macro registers models with
    /// [`crate::sync::SyncRegistry::register_schema`] for schema *sync*, which
    /// is a separate registry. Call this yourself for tables you want
    /// [`SchemaWriter::write_schema`] to emit without introspecting a database.
    pub fn register_schema(schema: TableSchema) {
        let mut registry = SCHEMA_REGISTRY.write();
        if !registry
            .iter()
            .any(|table| table.name == schema.name && table.schema_name == schema.schema_name)
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

    async fn introspect_postgres() -> Result<Vec<TableSchema>> {
        let conn = crate::require_db()?.__internal_connection()?;

        let table_rows = conn
            .query_all_raw(build_statement(
                Backend::Postgres,
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

            // `information_schema.columns.data_type` is a *category*, not a
            // declared type: it reports `text[]` as "ARRAY", an enum as
            // "USER-DEFINED", `varchar(50)` as an unbounded "character varying"
            // and `numeric(10,2)` as a bare "numeric". Replaying that produces
            // invalid SQL, so read the exact declared type from pg_catalog
            // instead - `format_type` renders element type, length and
            // precision the way the column was declared.
            let col_rows = conn
                .query_all_raw(build_statement_with_values(
                    Backend::Postgres,
                    "SELECT a.attname AS column_name,
                        pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                        CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END AS is_nullable,
                        pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS column_default
                 FROM pg_catalog.pg_attribute a
                 JOIN pg_catalog.pg_class c ON c.oid = a.attrelid
                 JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
                 LEFT JOIN pg_catalog.pg_attrdef d
                     ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                 WHERE ns.nspname = $1 AND c.relname = $2
                 AND a.attnum > 0 AND NOT a.attisdropped
                 ORDER BY a.attnum",
                    vec![table_schema.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let pk_rows = conn
                .query_all_raw(build_statement_with_values(
                    Backend::Postgres,
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
                .and_then(|row| String::try_get(row, "", "column_name").ok())
                .unwrap_or_default();

            let index_rows = conn
                .query_all_raw(build_statement_with_values(
                    Backend::Postgres,
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

            let mut index_map: HashMap<String, (bool, Vec<String>)> = HashMap::new();
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
                .map(|(name, (unique, columns))| IndexDefinition::new(name, columns, unique))
                .collect();

            let mut builder = TableSchemaBuilder::new(&table_name).schema(&table_schema);

            for row in col_rows {
                let col_name: String = row.try_get("", "column_name").unwrap_or_default();
                // `format_type` already returns the canonical spelling and
                // quotes what needs quoting, so it is used verbatim: upcasing
                // it would turn a quoted mixed-case enum type such as
                // `"MyStatus"` into an identifier that does not exist.
                let sql_type: String = row.try_get("", "data_type").unwrap_or_default();
                let is_nullable: String = row.try_get("", "is_nullable").unwrap_or_default();
                let default: Option<String> = row.try_get("", "column_default").ok();

                let mut col = ColumnSchema::new(&col_name, &sql_type);

                if col_name == pk_column {
                    col = col.primary_key();
                    if sql_type.to_uppercase().contains("SERIAL")
                        || default
                            .as_ref()
                            .map(|value| value.contains("nextval"))
                            .unwrap_or(false)
                    {
                        col = col.auto_increment();
                    }
                }

                if is_nullable == "NO" {
                    col = col.not_null();
                }

                if let Some(default) = default {
                    if !default.contains("nextval") {
                        col = col.default(default);
                    }
                }

                builder = builder.column(col);
            }

            builder = builder.indexes(indexes);
            schemas.push(builder.build());
        }

        Ok(schemas)
    }

    async fn introspect_mysql() -> Result<Vec<TableSchema>> {
        let conn = crate::require_db()?.__internal_connection()?;

        let db_name_row = conn
            .query_one_raw(build_statement(
                Backend::MySql,
                "SELECT DATABASE() as db_name",
            ))
            .await
            .map_err(|e| Error::query(e.to_string()))?;

        let db_name: String = db_name_row
            .and_then(|row| row.try_get("", "db_name").ok())
            .unwrap_or_default();

        if db_name.is_empty() {
            return Ok(Vec::new());
        }

        let table_rows = conn
            .query_all_raw(build_statement_with_values(
                Backend::MySql,
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

            let col_rows = conn
                .query_all_raw(build_statement_with_values(
                    Backend::MySql,
                    "SELECT column_name, column_type, is_nullable, column_default, column_key, extra
                 FROM information_schema.columns
                 WHERE table_schema = ? AND table_name = ?
                 ORDER BY ordinal_position",
                    vec![db_name.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let index_rows = conn
                .query_all_raw(build_statement_with_values(
                    Backend::MySql,
                    "SELECT index_name, non_unique, column_name
                 FROM information_schema.statistics
                 WHERE table_schema = ? AND table_name = ?
                 AND index_name != 'PRIMARY'
                 ORDER BY index_name, seq_in_index",
                    vec![db_name.clone().into(), table_name.clone().into()],
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let mut index_map: HashMap<String, (bool, Vec<String>)> = HashMap::new();
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
                .map(|(name, (unique, columns))| IndexDefinition::new(name, columns, unique))
                .collect();

            let mut builder = TableSchemaBuilder::new(&table_name);

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
                    if extra.contains("auto_increment") {
                        col = col.auto_increment();
                    }
                }

                if is_nullable == "NO" {
                    col = col.not_null();
                }

                if let Some(default) = default {
                    col = col.default(default);
                }

                builder = builder.column(col);
            }

            builder = builder.indexes(indexes);
            schemas.push(builder.build());
        }

        Ok(schemas)
    }

    async fn introspect_sqlite() -> Result<Vec<TableSchema>> {
        let conn = crate::require_db()?.__internal_connection()?;

        let table_rows = conn
            .query_all_raw(build_statement(
                Backend::Sqlite,
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

            let col_rows = conn
                .query_all_raw(build_statement(
                    Backend::Sqlite,
                    format!("PRAGMA table_info({})", quoted_table_name),
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let index_list = conn
                .query_all_raw(build_statement(
                    Backend::Sqlite,
                    format!("PRAGMA index_list({})", quoted_table_name),
                ))
                .await
                .map_err(|e| Error::query(e.to_string()))?;

            let mut indexes = Vec::new();
            for idx_row in index_list {
                let idx_name: String = idx_row.try_get("", "name").unwrap_or_default();
                let is_unique: i32 = idx_row.try_get("", "unique").unwrap_or(0);
                let origin: String = idx_row.try_get("", "origin").unwrap_or_default();

                if origin == "pk" {
                    continue;
                }

                let idx_info = conn
                    .query_all_raw(build_statement(
                        Backend::Sqlite,
                        format!(
                            "PRAGMA index_info({})",
                            quote_ident(DatabaseType::SQLite, &idx_name)
                        ),
                    ))
                    .await
                    .map_err(|e| Error::query(e.to_string()))?;

                let columns: Vec<String> = idx_info
                    .iter()
                    .filter_map(|row| row.try_get("", "name").ok())
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
                    if sql_type == "INTEGER" {
                        col = col.auto_increment();
                    }
                }

                if notnull == 1 {
                    col = col.not_null();
                }

                if let Some(default) = default {
                    col = col.default(default);
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
