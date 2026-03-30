use crate::config::DatabaseType;
use crate::internal::sql_safety::{format_identifier_reference, quote_ident};

use super::{ColumnSchema, TableSchema};

/// Schema generator for creating SQL schema files
pub struct SchemaGenerator {
    database_type: DatabaseType,
    tables: Vec<TableSchema>,
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

        sql.push_str("-- TideORM Generated Schema\n");
        sql.push_str(&format!("-- Database: {:?}\n", self.database_type));
        sql.push_str(&format!(
            "-- Generated at: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));

        for table in &self.tables {
            sql.push_str(&self.generate_create_table(table));
            sql.push('\n');
        }

        for table in &self.tables {
            let indexes = self.generate_indexes(table);
            if !indexes.is_empty() {
                sql.push_str(&indexes);
                sql.push('\n');
            }
        }

        sql
    }

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

    fn generate_column_def(&self, col: &ColumnSchema) -> String {
        let mut def = format!("    {} {}", self.quote_identifier(&col.name), col.sql_type);

        if col.auto_increment {
            match self.database_type {
                DatabaseType::Postgres => {
                    if col.sql_type.to_uppercase().contains("INT") {
                        def = format!("    {} BIGSERIAL", self.quote_identifier(&col.name));
                    }
                }
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    def.push_str(" AUTO_INCREMENT");
                }
                DatabaseType::SQLite => {}
            }
        }

        if !col.nullable && !col.primary_key {
            def.push_str(" NOT NULL");
        }

        if let Some(default) = &col.default {
            def.push_str(&format!(" DEFAULT {}", default));
        }

        def
    }

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
                .map(|column| self.quote_identifier(column))
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