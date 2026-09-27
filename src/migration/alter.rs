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
    /// What MySQL's `MODIFY COLUMN` would drop from a column whose type
    /// changes — its NOT NULL, DEFAULT, AUTO_INCREMENT, COMMENT — by column.
    kept_attributes: std::collections::HashMap<String, String>,
}

impl AlterTableBuilder {
    /// Create a new alter table builder
    pub fn new(name: &str, database_type: DatabaseType) -> Self {
        Self {
            name: name.to_string(),
            database_type,
            operations: Vec::new(),
            kept_attributes: std::collections::HashMap::new(),
        }
    }

    /// Add a new column
    pub fn add_column(&mut self, name: &str, column_type: ColumnType) -> AlterColumnBuilder<'_> {
        let database_type = self.database_type;
        ColumnBuilder::new(
            self,
            database_type,
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

    /// Change a column's type, keeping its nullability, default and
    /// auto-increment: PostgreSQL's `ALTER COLUMN .. TYPE` keeps them, and on
    /// MySQL and MariaDB, whose `MODIFY COLUMN` restates the whole column,
    /// [`Schema::alter_table`](super::Schema::alter_table) reads them from the
    /// table first. SQLite cannot change a column's type.
    pub fn change_column(&mut self, name: &str, column_type: ColumnType) -> &mut Self {
        self.operations.push(AlterOperation::ChangeColumnType(
            name.to_string(),
            column_type,
        ));
        self
    }

    /// Whether a MySQL `MODIFY COLUMN` needs the table's current definition.
    pub(crate) fn changes_mysql_column_types(&self) -> bool {
        matches!(
            self.database_type,
            DatabaseType::MySQL | DatabaseType::MariaDB
        ) && self
            .operations
            .iter()
            .any(|operation| matches!(operation, AlterOperation::ChangeColumnType(..)))
    }

    /// Keep what `create_table`, the table's `SHOW CREATE TABLE`, declares
    /// for each column whose type changes, besides the type.
    pub(crate) fn keep_column_attributes(&mut self, create_table: &str) {
        for operation in &self.operations {
            if let AlterOperation::ChangeColumnType(name, _) = operation
                && let Some(attributes) = mysql_column_attributes(create_table, name)
            {
                self.kept_attributes.insert(name.clone(), attributes);
            }
        }
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
                        let mut sql = format!(
                            "ALTER TABLE {} MODIFY COLUMN {} {}",
                            table,
                            quote_ident(db_type, name),
                            type_sql
                        );
                        if let Some(attributes) = self
                            .kept_attributes
                            .get(name)
                            .filter(|kept| !kept.is_empty())
                        {
                            sql.push(' ');
                            sql.push_str(attributes);
                        }
                        sql
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

/// What `create_table`, a MySQL or MariaDB `SHOW CREATE TABLE`, declares for
/// `column` after its type, character set and collation: `NOT NULL DEFAULT '5'`,
/// `NOT NULL AUTO_INCREMENT`, `DEFAULT NULL COMMENT '..'`. `None` when the table
/// has no such column.
fn mysql_column_attributes(create_table: &str, column: &str) -> Option<String> {
    // The first word of a column's definition that is not part of its type.
    const ATTRIBUTES: [&str; 17] = [
        "NOT",
        "NULL",
        "DEFAULT",
        "AUTO_INCREMENT",
        "ON",
        "COMMENT",
        "GENERATED",
        "AS",
        "INVISIBLE",
        "VISIBLE",
        "CHECK",
        "COLUMN_FORMAT",
        "STORAGE",
        "SRID",
        "PRIMARY",
        "UNIQUE",
        "REFERENCES",
    ];

    let quoted = format!("`{}`", column.replace('`', "``"));
    let definition = create_table.lines().map(str::trim).find_map(|line| {
        let head = line.get(..quoted.len())?;
        let rest = line.get(quoted.len()..)?;
        (head.eq_ignore_ascii_case(&quoted) && rest.starts_with(' ')).then_some(rest)
    })?;
    let mut rest = definition.trim().trim_end_matches(',');

    loop {
        rest = rest.trim_start();
        let first = rest.chars().next()?;
        // A quoted literal or a parenthesized group is one word.
        let end = match first {
            '\'' | '"' | '`' => rest[1..].find(first).map_or(rest.len(), |end| end + 2),
            '(' => {
                let mut depth = 0usize;
                rest.char_indices()
                    .find(|&(_, character)| {
                        match character {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        depth == 0
                    })
                    .map_or(rest.len(), |(index, _)| index + 1)
            }
            _ => rest
                .find(|character: char| character.is_whitespace() || character == '(')
                .unwrap_or(rest.len()),
        };
        let word = &rest[..end];
        if ATTRIBUTES
            .iter()
            .any(|attribute| word.eq_ignore_ascii_case(attribute))
        {
            return Some(rest.to_string());
        }
        if end == rest.len() {
            return Some(String::new());
        }
        rest = &rest[end..];
    }
}

#[derive(Debug, Clone)]
enum AlterOperation {
    AddColumn(ColumnDefinition),
    DropColumn(String),
    RenameColumn(String, String),
    ChangeColumnType(String, ColumnType),
}
