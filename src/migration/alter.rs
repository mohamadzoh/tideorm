use super::table::{ColumnDefinition, IndexBuilder};
use super::{ColumnType, DatabaseType, DefaultValue, quote_identifier_for_db};

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
                check: None,
                extra: None,
            },
        }
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

    /// Add an index
    pub fn add_index(&mut self, name: &str, columns: &[&str], unique: bool) -> &mut Self {
        self.operations.push(AlterOperation::AddIndex(IndexBuilder {
            name: name.to_string(),
            columns: columns.iter().map(|value| value.to_string()).collect(),
            unique,
        }));
        self
    }

    /// Drop an index
    pub fn drop_index(&mut self, name: &str) -> &mut Self {
        self.operations
            .push(AlterOperation::DropIndex(name.to_string()));
        self
    }

    pub(crate) fn build(&self) -> Vec<String> {
        self.operations
            .iter()
            .map(|operation| self.build_operation(operation))
            .collect()
    }

    fn build_operation(&self, operation: &AlterOperation) -> String {
        match operation {
            AlterOperation::AddColumn(column) => {
                let column_definition = self.build_column_def(column);
                format!(
                    "ALTER TABLE {} ADD COLUMN {}",
                    self.quote_identifier(&self.name),
                    column_definition.trim()
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
                DatabaseType::MySQL | DatabaseType::MariaDB => {
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
                    DatabaseType::MySQL | DatabaseType::MariaDB => {
                        format!(
                            "ALTER TABLE {} MODIFY COLUMN {} {}",
                            self.quote_identifier(&self.name),
                            self.quote_identifier(name),
                            type_sql
                        )
                    }
                    DatabaseType::SQLite => {
                        format!(
                            "-- SQLite does not support ALTER COLUMN TYPE; table recreation needed for {}",
                            name
                        )
                    }
                }
            }
            AlterOperation::AddIndex(index) => {
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

                format!(
                    "CREATE {} {} ON {} ({})",
                    index_type,
                    self.quote_identifier(&index.name),
                    self.quote_identifier(&self.name),
                    columns.join(", ")
                )
            }
            AlterOperation::DropIndex(name) => match self.database_type {
                DatabaseType::MySQL | DatabaseType::MariaDB => {
                    format!(
                        "DROP INDEX {} ON {}",
                        self.quote_identifier(name),
                        self.quote_identifier(&self.name)
                    )
                }
                _ => format!("DROP INDEX {}", self.quote_identifier(name)),
            },
        }
    }

    fn build_column_def(&self, column: &ColumnDefinition) -> String {
        let mut definition = format!(
            "{} {}",
            self.quote_identifier(&column.name),
            self.type_to_sql(&column.column_type)
        );

        if !column.nullable {
            definition.push_str(" NOT NULL");
        }

        if let Some(default) = &column.default {
            definition.push_str(&format!(" DEFAULT {}", default));
        }

        if column.unique {
            definition.push_str(" UNIQUE");
        }

        definition
    }

    fn type_to_sql(&self, column_type: &ColumnType) -> String {
        match self.database_type {
            DatabaseType::Postgres => column_type.to_postgres_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => column_type.to_mysql_sql(),
            DatabaseType::SQLite => column_type.to_sqlite_sql(),
        }
    }

    fn quote_identifier(&self, name: &str) -> String {
        quote_identifier_for_db(name, self.database_type)
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
        let definition = std::mem::replace(
            &mut self.definition,
            ColumnDefinition {
                name: String::new(),
                column_type: ColumnType::String,
                nullable: true,
                default: None,
                primary_key: false,
                auto_increment: false,
                unique: false,
                check: None,
                extra: None,
            },
        );
        if !definition.name.is_empty() {
            self.builder
                .operations
                .push(AlterOperation::AddColumn(definition));
        }
    }
}

#[derive(Debug, Clone)]
enum AlterOperation {
    AddColumn(ColumnDefinition),
    DropColumn(String),
    RenameColumn(String, String),
    ChangeColumnType(String, ColumnType),
    AddIndex(IndexBuilder),
    DropIndex(String),
}
