use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident;

use super::ddl::{self, ColumnDefinition};
use super::{ColumnBuilder, ColumnType, DatabaseType};

/// Builder for adding columns in ALTER TABLE
pub type AlterColumnBuilder<'a> = ColumnBuilder<'a, AlterTableBuilder>;

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
        ColumnBuilder::new(
            self,
            ColumnDefinition::new(name, column_type),
            |alter, column| alter.operations.push(AlterOperation::AddColumn(column)),
        )
    }

    /// Drop a column
    pub fn drop_column(&mut self, name: &str) -> &mut Self {
        self.operations
            .push(AlterOperation::DropColumn(name.to_string()));
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

    pub(crate) fn build(&self) -> Result<Vec<String>> {
        self.operations
            .iter()
            .map(|operation| self.build_operation(operation))
            .collect()
    }

    fn build_operation(&self, operation: &AlterOperation) -> Result<String> {
        let db_type = self.database_type;
        let table = quote_ident(db_type, &self.name);

        let sql = match operation {
            AlterOperation::AddColumn(column) => ddl::add_column(db_type, &table, column),
            AlterOperation::DropColumn(name) => {
                format!(
                    "ALTER TABLE {} DROP COLUMN {}",
                    table,
                    quote_ident(db_type, name)
                )
            }
            AlterOperation::RenameColumn(from, to) => {
                format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {}",
                    table,
                    quote_ident(db_type, from),
                    quote_ident(db_type, to)
                )
            }
            AlterOperation::ChangeColumnType(name, column_type) => {
                let type_sql = column_type.to_sql(db_type);
                match db_type {
                    DatabaseType::Postgres => {
                        format!(
                            "ALTER TABLE {} ALTER COLUMN {} TYPE {}",
                            table,
                            quote_ident(db_type, name),
                            type_sql
                        )
                    }
                    DatabaseType::MySQL | DatabaseType::MariaDB => {
                        format!(
                            "ALTER TABLE {} MODIFY COLUMN {} {}",
                            table,
                            quote_ident(db_type, name),
                            type_sql
                        )
                    }
                    // Emitting a SQL comment here would let the migration be
                    // recorded as applied while the column keeps its old type.
                    DatabaseType::SQLite => {
                        return Err(Error::backend_not_supported(
                            format!(
                                "SQLite cannot change the type of column '{}' on table '{}'. \
                                 Rebuild the table instead: create a new table with the target \
                                 definition, copy the rows across, drop the old table and rename \
                                 the new one.",
                                name, self.name
                            ),
                            "SQLite",
                        ));
                    }
                }
            }
        };

        Ok(sql)
    }
}

#[derive(Debug, Clone)]
enum AlterOperation {
    AddColumn(ColumnDefinition),
    DropColumn(String),
    RenameColumn(String, String),
    ChangeColumnType(String, ColumnType),
}
