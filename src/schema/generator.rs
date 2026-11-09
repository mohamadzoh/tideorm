use std::fmt::Write as _;

use crate::config::DatabaseType;
use crate::internal::sql_safety::quote_qualified;
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
    pub fn add_table(&mut self, schema: TableSchema) -> &mut Self {
        self.tables.push(schema);
        self
    }

    /// Add the table of the model `M`, typed as schema sync would create it
    /// on this generator's backend.
    pub fn add_model<M: crate::sync::SyncModel>(&mut self) -> &mut Self {
        self.add_table(M::sync_schema().table_schema(self.database_type))
    }

    /// Generate complete SQL schema
    pub fn generate(&self) -> String {
        let mut sql = String::new();

        sql.push_str("-- TideORM Generated Schema\n");
        let _ = writeln!(sql, "-- Database: {:?}", self.database_type);
        let _ = writeln!(
            sql,
            "-- Generated at: {}\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );

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

    pub(super) fn generate_create_table(&self, table: &TableSchema) -> String {
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
                statement + ";\n"
            })
            .collect()
    }

    fn table_reference(&self, table: &TableSchema) -> String {
        if let Some(schema_name) = &table.schema_name {
            return quote_qualified(self.database_type, Some(schema_name), &table.name);
        }

        crate::query::db_sql::format_column(self.database_type, &table.name)
    }
}

/// The DDL column a declared column becomes, its SQL type used verbatim.
fn column_definition(column: &ColumnSchema) -> ColumnDefinition {
    let mut definition = ColumnDefinition::new(
        &column.name,
        ColumnType::Custom(column.sql_type.clone().into()),
    );
    definition.nullable = column.nullable;
    definition.default = column.default.clone();
    definition.primary_key = column.primary_key;
    definition.auto_increment = column.auto_increment;
    definition
}
