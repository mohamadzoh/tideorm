//! The statements an [`AlterTableBuilder`] runs.

use crate::config::DatabaseType;
use crate::error::{Error, Result};
use crate::internal::sql_safety::{quote_ident, quote_qualified};
use crate::migration::{column_type_sql, ddl};

use super::mysql_columns::{holds_text, restate_attributes};
use super::{AlterOperation, AlterTableBuilder, ColumnChange};

impl AlterTableBuilder {
    /// The name of the unique index a unique `column` added on SQLite comes
    /// with.
    fn sqlite_column_index(&self, column: &str) -> String {
        ddl::index_name(self.table(), &[column], true)
    }

    fn sqlite_index_reference(&self, name: &str) -> String {
        quote_qualified(
            self.database_type,
            self.sqlite_index_schema.as_deref(),
            name,
        )
    }

    pub(crate) fn build(&self) -> Result<Vec<String>> {
        let mut statements = Vec::with_capacity(self.operations.len());
        let mut indexes = self.sqlite_column_indexes.clone();
        for operation in &self.operations {
            // SQLite refuses to drop a column an index covers, such as the
            // unique index a unique column added on SQLite comes with. Only
            // this table's: SQLite names indexes per database, so another
            // table's index can have the name this one's would get.
            if let AlterOperation::DropColumn(name) = operation
                && self.database_type == DatabaseType::SQLite
            {
                for (_, existing) in indexes.iter().filter(|(column, _)| column == name) {
                    statements.push(format!(
                        "DROP INDEX {}",
                        self.sqlite_index_reference(existing)
                    ));
                }
                indexes.retain(|(column, _)| column != name);
            }
            statements.extend(self.build_operation(operation)?);
            if let Some(column) = self.sqlite_unique_addition(operation) {
                let index = self.sqlite_column_index(&column.name);
                statements.push(format!(
                    "CREATE UNIQUE INDEX {} ON {} ({})",
                    self.sqlite_index_reference(&index),
                    quote_ident(self.database_type, self.table()),
                    quote_ident(self.database_type, &column.name),
                ));
                indexes.push((column.name.clone(), index));
            }
            match operation {
                AlterOperation::RenameColumn(from, to) => {
                    for (column, _) in &mut indexes {
                        if column == from {
                            column.clone_from(to);
                        }
                    }
                }
                AlterOperation::AddIndex(index) if index.columns.len() == 1 => {
                    indexes.push((index.columns[0].clone(), index.name.clone()));
                }
                AlterOperation::DropIndex(name) => indexes.retain(|(_, index)| index != name),
                _ => {}
            }
        }
        Ok(statements)
    }

    /// The column `operation` adds when it is a unique one added on SQLite,
    /// which refuses `ADD COLUMN .. UNIQUE`: the column is added without the
    /// constraint and a unique index enforces it.
    fn sqlite_unique_addition<'a>(
        &self,
        operation: &'a AlterOperation,
    ) -> Option<&'a ddl::ColumnDefinition> {
        match operation {
            AlterOperation::AddColumn(column, _)
                if self.database_type == DatabaseType::SQLite
                    && column.unique
                    && !column.primary_key =>
            {
                Some(column)
            }
            _ => None,
        }
    }

    /// The error for what SQLite's `ALTER TABLE` cannot do, with what to do
    /// instead besides rebuilding the table.
    fn sqlite_cannot(&self, what: String, alternative: &str) -> Error {
        Error::backend_not_supported(
            format!(
                "SQLite cannot {what} on table '{}'. Instead, {alternative}rebuild the table in \
                 a migration whose `foreign_keys_off()` is true: create a new table with the \
                 target definition, copy the rows across, drop the old table and rename the \
                 new one.",
                self.name
            ),
            "SQLite",
        )
    }

    fn build_operation(&self, operation: &AlterOperation) -> Result<Vec<String>> {
        let db_type = self.database_type;
        let table = ddl::quote_table(db_type, &self.name);

        let sql = match operation {
            AlterOperation::AddColumn(column, key) => {
                let mut plain;
                let column = match self.sqlite_unique_addition(operation) {
                    Some(column) => {
                        plain = column.clone();
                        plain.unique = false;
                        &plain
                    }
                    None => column,
                };
                let mut sql = ddl::add_column(db_type, &table, column);
                match key {
                    // SQLite adds no table constraint, but takes the key with
                    // the column.
                    Some(key) if db_type == DatabaseType::SQLite => {
                        sql.push(' ');
                        sql.push_str(&key.column_constraint_sql(db_type, self.table()));
                    }
                    Some(key) => {
                        return Ok(vec![
                            sql,
                            format!(
                                "ALTER TABLE {} ADD {}",
                                table,
                                key.constraint_sql(db_type, self.table())
                            ),
                        ]);
                    }
                    None => {}
                }
                sql
            }
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
            AlterOperation::ChangeColumn(name, change) => {
                return self.change_column_sql(&table, name, change);
            }
            AlterOperation::AddIndex(index) => ddl::create_index(
                db_type,
                &index.name,
                &table,
                &index.columns,
                index.unique,
                false,
            ),
            AlterOperation::DropIndex(name) => {
                let (schema, table) = ddl::table_parts(db_type, &self.name);
                ddl::drop_index(db_type, schema, table, name, false)
            }
            AlterOperation::AddForeignKey(key) => {
                if db_type == DatabaseType::SQLite {
                    return Err(self.sqlite_cannot(
                        format!("add foreign key '{}'", key.name(self.table())),
                        "add the key with a new column (`add_column(..).references(..)`) or ",
                    ));
                }
                format!(
                    "ALTER TABLE {} ADD {}",
                    table,
                    key.constraint_sql(db_type, self.table())
                )
            }
            AlterOperation::DropForeignKey(name) => match db_type {
                DatabaseType::Postgres => format!(
                    "ALTER TABLE {} DROP CONSTRAINT {}",
                    table,
                    quote_ident(db_type, name)
                ),
                DatabaseType::MySQL | DatabaseType::MariaDB => format!(
                    "ALTER TABLE {} DROP FOREIGN KEY {}",
                    table,
                    quote_ident(db_type, name)
                ),
                DatabaseType::SQLite => {
                    return Err(self.sqlite_cannot(format!("drop foreign key '{name}'"), ""));
                }
            },
        };

        Ok(vec![sql])
    }

    /// The statements that make `change` to the column `name` of the quoted
    /// `table`: none for an empty change.
    fn change_column_sql(
        &self,
        table: &str,
        name: &str,
        change: &ColumnChange,
    ) -> Result<Vec<String>> {
        let db_type = self.database_type;
        let column = quote_ident(db_type, name);
        let sql = match db_type {
            DatabaseType::Postgres => {
                let mut actions = Vec::new();
                if let Some(None) = &change.default {
                    actions.push(format!("ALTER COLUMN {column} DROP DEFAULT"));
                }
                if let Some(column_type) = &change.column_type {
                    actions.push(format!(
                        "ALTER COLUMN {column} TYPE {}",
                        column_type_sql(column_type, db_type)
                    ));
                }
                if let Some(Some(default)) = &change.default {
                    actions.push(format!("ALTER COLUMN {column} SET DEFAULT {default}"));
                }
                match change.nullable {
                    Some(true) => actions.push(format!("ALTER COLUMN {column} DROP NOT NULL")),
                    Some(false) => actions.push(format!("ALTER COLUMN {column} SET NOT NULL")),
                    None => {}
                }
                if actions.is_empty() {
                    return Ok(Vec::new());
                }
                format!("ALTER TABLE {} {}", table, actions.join(", "))
            }
            DatabaseType::MySQL | DatabaseType::MariaDB if !change.restates() => {
                match &change.default {
                    Some(Some(default)) => {
                        format!("ALTER TABLE {table} ALTER COLUMN {column} SET DEFAULT {default}")
                    }
                    Some(None) => format!("ALTER TABLE {table} ALTER COLUMN {column} DROP DEFAULT"),
                    None => return Ok(Vec::new()),
                }
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {
                let kept = self.kept_attributes.get(name);
                let type_sql = match (&change.column_type, kept) {
                    (Some(column_type), _) => column_type_sql(column_type, db_type),
                    (None, Some(kept)) => kept.column_type.clone(),
                    (None, None) => {
                        return Err(Error::query(format!(
                            "cannot change column '{}' of table '{}': MySQL restates a column \
                             whole, and the table has no such column to restate",
                            name, self.name
                        )));
                    }
                };
                let (character, rest) = kept.map_or(("", ""), |kept| {
                    (kept.character.as_str(), kept.rest.as_str())
                });
                // A character set or collation stays with a column that still
                // holds text, which would otherwise take the table's; any
                // other type refuses one.
                let character = if holds_text(&type_sql) { character } else { "" };
                let rest = restate_attributes(rest, change.nullable, change.default.as_ref());
                let mut sql = format!("ALTER TABLE {table} MODIFY COLUMN {column} {type_sql}");
                for attribute in [character, rest.as_str()] {
                    if !attribute.is_empty() {
                        sql.push(' ');
                        sql.push_str(attribute);
                    }
                }
                sql
            }
            // Emitting a SQL comment here would let the migration be recorded
            // as applied while the column keeps its old definition.
            DatabaseType::SQLite => {
                return Err(self.sqlite_cannot(
                    format!("change the type, nullability or default of column '{name}'"),
                    "",
                ));
            }
        };
        Ok(vec![sql])
    }
}
