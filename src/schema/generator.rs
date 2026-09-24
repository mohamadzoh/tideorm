use crate::config::DatabaseType;
use crate::internal::sql_safety::{format_identifier_reference, quote_ident};
use crate::migration::ColumnType;
use crate::migration::ddl::{self, ColumnDefinition};

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
        let columns: Vec<ColumnDefinition> = table.columns.iter().map(column_definition).collect();

        let mut sql = ddl::create_table(
            self.database_type,
            &self.table_reference(table),
            true,
            &columns,
            &table.primary_keys,
            &[],
        );
        sql.push_str(";\n");
        sql
    }

    fn generate_indexes(&self, table: &TableSchema) -> String {
        let table_reference = self.table_reference(table);

        table
            .indexes
            .iter()
            .map(|index| {
                let statement = ddl::create_index(
                    self.database_type,
                    &index.name,
                    &table_reference,
                    &index.columns,
                    index.unique,
                    true,
                );
                format!("{};\n", statement)
            })
            .collect()
    }

    fn table_reference(&self, table: &TableSchema) -> String {
        if let Some(schema_name) = &table.schema_name {
            return format!(
                "{}.{}",
                quote_ident(self.database_type, schema_name),
                quote_ident(self.database_type, &table.name)
            );
        }

        format_identifier_reference(self.database_type, &table.name)
            .unwrap_or_else(|| quote_ident(self.database_type, &table.name))
    }
}

/// The DDL column a declared column becomes, its SQL type used verbatim.
fn column_definition(column: &ColumnSchema) -> ColumnDefinition {
    let mut definition =
        ColumnDefinition::new(&column.name, ColumnType::Custom(column.sql_type.clone()));
    definition.nullable = column.nullable;
    definition.default = column.default.clone();
    definition.primary_key = column.primary_key;
    definition.auto_increment = column.auto_increment;
    definition
}
