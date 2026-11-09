use crate::model::IndexDefinition;

use super::ddl::{self, ColumnDefinition};
use super::foreign_key::{ForeignKey, ForeignKeyBuilder};
use super::table::typed_column_methods;
use super::{ColumnBuilder, ColumnType, DatabaseType, DefaultValue, StringLen};

mod build;
mod mysql_columns;

use mysql_columns::{KeptAttributes, mysql_column_attributes};

/// Builder for a column [`AlterTableBuilder::add_column`] adds.
pub type AlterColumnBuilder<'a> = ColumnBuilder<'a, AlterTableBuilder>;

/// Builder for ALTER TABLE operations
///
/// Each call adds one change, and [`Schema::alter_table`](super::Schema::alter_table)
/// runs them in the order they were made.
pub struct AlterTableBuilder {
    name: String,
    database_type: DatabaseType,
    operations: Vec<AlterOperation>,
    /// What MySQL's `MODIFY COLUMN` would drop from a column it restates, by
    /// column.
    kept_attributes: std::collections::HashMap<String, KeptAttributes>,
    /// Explicit single-column indexes over dropped columns, as (column, index).
    sqlite_column_indexes: Vec<(String, String)>,
    sqlite_index_schema: Option<String>,
}

/// What [`AlterTableBuilder::alter_column`] changes about a column; `None`
/// keeps that part as it is.
#[derive(Debug, Clone, Default)]
struct ColumnChange {
    column_type: Option<ColumnType>,
    nullable: Option<bool>,
    /// The new default, rendered, or `Some(None)` to drop the default.
    default: Option<Option<String>>,
}

impl ColumnChange {
    /// Whether MySQL has to restate the column with `MODIFY COLUMN`, which
    /// `ALTER COLUMN .. SET DEFAULT` spares a change of default alone.
    fn restates(&self) -> bool {
        self.column_type.is_some() || self.nullable.is_some()
    }
}

/// Builder for the change [`AlterTableBuilder::alter_column`] makes to a
/// column.
///
/// The change is added when the builder drops, so
/// `t.alter_column("email").not_null();` needs no terminating call.
pub struct ColumnChangeBuilder<'a> {
    alter: &'a mut AlterTableBuilder,
    name: String,
    change: ColumnChange,
}

impl ColumnChangeBuilder<'_> {
    /// Change the column's type.
    pub fn set_type(mut self, column_type: ColumnType) -> Self {
        self.change.column_type = Some(column_type);
        self
    }

    /// Let the column hold NULL.
    pub fn nullable(mut self) -> Self {
        self.change.nullable = Some(true);
        self
    }

    /// Refuse NULL in the column, which fails while a row holds one.
    pub fn not_null(mut self) -> Self {
        self.change.nullable = Some(false);
        self
    }

    /// Set the column's default.
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.change.default = Some(Some(value.into().to_sql(self.alter.database_type)));
        self
    }

    /// Remove the column's default.
    pub fn drop_default(mut self) -> Self {
        self.change.default = Some(None);
        self
    }
}

impl Drop for ColumnChangeBuilder<'_> {
    fn drop(&mut self) {
        let change = std::mem::take(&mut self.change);
        let name = std::mem::take(&mut self.name);
        self.alter
            .operations
            .push(AlterOperation::ChangeColumn(name, change));
    }
}

impl AlterTableBuilder {
    /// Create a new alter table builder for the table `name`, or
    /// `schema.table`.
    pub fn new(name: &str, database_type: DatabaseType) -> Self {
        Self {
            name: name.to_string(),
            database_type,
            operations: Vec::new(),
            kept_attributes: std::collections::HashMap::new(),
            sqlite_column_indexes: Vec::new(),
            sqlite_index_schema: None,
        }
    }

    /// Add a new column
    ///
    /// A column that [`references`](ColumnBuilder::references) another
    /// table is added with its foreign key, on SQLite too, which can add no
    /// key to an existing column; SQLite then wants the column to default
    /// to NULL.
    pub fn add_column(&mut self, name: &str, column_type: ColumnType) -> AlterColumnBuilder<'_> {
        let database_type = self.database_type;
        ColumnBuilder::new(
            self,
            database_type,
            ColumnDefinition::new(name, column_type),
            |alter, column, key| {
                alter
                    .operations
                    .push(AlterOperation::AddColumn(column, key))
            },
        )
    }

    typed_column_methods!(add_column);

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
    ///
    /// The shorthand for `alter_column(name).set_type(column_type)`.
    pub fn change_column(&mut self, name: &str, column_type: ColumnType) -> &mut Self {
        self.alter_column(name).set_type(column_type);
        self
    }

    /// Change a column's type, nullability or default, keeping what the
    /// builder leaves alone.
    ///
    /// ```rust,ignore
    /// schema.alter_table("users", |t| {
    ///     t.alter_column("email").not_null();
    ///     t.alter_column("score").set_type(ColumnType::BigInteger).default(0);
    ///     t.alter_column("nickname").drop_default();
    /// }).await?;
    /// ```
    ///
    /// PostgreSQL changes each part in place. MySQL and MariaDB restate the
    /// column with `MODIFY COLUMN` for a new type or nullability, keeping
    /// the rest of the definition [`Schema::alter_table`](super::Schema::alter_table)
    /// reads from the table, and change a default alone with
    /// `ALTER COLUMN .. SET DEFAULT`. SQLite cannot alter a column, so there
    /// the alteration fails before it runs anything.
    pub fn alter_column(&mut self, name: &str) -> ColumnChangeBuilder<'_> {
        ColumnChangeBuilder {
            alter: self,
            name: name.to_string(),
            change: ColumnChange::default(),
        }
    }

    /// Index `columns` under the name [`TableBuilder::index`](super::TableBuilder::index)
    /// would give it, `idx_<table>_<columns>`, shortened with a hash past 63
    /// bytes.
    pub fn add_index(&mut self, columns: &[&str]) -> &mut Self {
        let name = ddl::index_name(self.table(), columns, false);
        self.push_index(name, columns, false)
    }

    /// Index `columns` under `name`.
    pub fn add_index_named(&mut self, name: &str, columns: &[&str]) -> &mut Self {
        self.push_index(name.to_string(), columns, false)
    }

    /// A unique index on `columns`, named `idx_<table>_<columns>_unique`
    /// (shortened with a hash past 63 bytes).
    pub fn add_unique_index(&mut self, columns: &[&str]) -> &mut Self {
        let name = ddl::index_name(self.table(), columns, true);
        self.push_index(name, columns, true)
    }

    /// A unique index on `columns`, named `name`.
    pub fn add_unique_index_named(&mut self, name: &str, columns: &[&str]) -> &mut Self {
        self.push_index(name.to_string(), columns, true)
    }

    fn push_index(&mut self, name: String, columns: &[&str], unique: bool) -> &mut Self {
        self.operations
            .push(AlterOperation::AddIndex(IndexDefinition::new(
                name,
                columns.iter().map(|column| column.to_string()).collect(),
                unique,
            )));
        self
    }

    /// Drop the table's index `name`.
    pub fn drop_index(&mut self, name: &str) -> &mut Self {
        self.operations
            .push(AlterOperation::DropIndex(name.to_string()));
        self
    }

    /// Add a foreign key from `columns` of this table to `references` of
    /// `table`. Unnamed, it is `fk_<table>_<columns>`, shortened with a hash
    /// past 63 bytes.
    ///
    /// SQLite cannot add a key to an existing table: there the alteration
    /// fails before it runs anything. Add the key with its column
    /// ([`add_column`](Self::add_column) and
    /// [`references`](ColumnBuilder::references)) or rebuild the table.
    pub fn add_foreign_key(
        &mut self,
        columns: &[&str],
        table: &str,
        references: &[&str],
    ) -> ForeignKeyBuilder<'_, Self> {
        ForeignKeyBuilder::new(
            self,
            ForeignKey::new(columns, table, references),
            |alter, key| alter.operations.push(AlterOperation::AddForeignKey(key)),
        )
    }

    /// Drop the table's foreign key `name`. SQLite cannot drop one, so there
    /// the alteration fails before it runs anything.
    pub fn drop_foreign_key(&mut self, name: &str) -> &mut Self {
        self.operations
            .push(AlterOperation::DropForeignKey(name.to_string()));
        self
    }

    /// Whether a MySQL `MODIFY COLUMN` needs the table's current definition.
    pub(crate) fn restates_mysql_columns(&self) -> bool {
        matches!(
            self.database_type,
            DatabaseType::MySQL | DatabaseType::MariaDB
        ) && self.operations.iter().any(|operation| {
            matches!(operation, AlterOperation::ChangeColumn(_, change) if change.restates())
        })
    }

    /// Keep what `create_table`, the table's `SHOW CREATE TABLE`, declares
    /// for each column the alteration restates. A column renamed earlier in
    /// the same alteration is looked up by the name the table still gives
    /// it; one added or dropped earlier has no stored definition.
    pub(crate) fn keep_column_attributes(&mut self, create_table: &str) {
        // By the name a column has at this point of the alteration, the name
        // the table stores it under, or `None` for one it does not store.
        let mut stored_names: std::collections::HashMap<&str, Option<&str>> =
            std::collections::HashMap::new();
        for operation in &self.operations {
            match operation {
                AlterOperation::RenameColumn(from, to) => {
                    let stored = stored_names.remove(from.as_str()).unwrap_or(Some(from));
                    stored_names.insert(to, stored);
                }
                AlterOperation::AddColumn(column, _) => {
                    stored_names.insert(&column.name, None);
                }
                AlterOperation::DropColumn(name) => {
                    stored_names.insert(name, None);
                }
                AlterOperation::ChangeColumn(name, change) if change.restates() => {
                    if let Some(stored) = stored_names
                        .get(name.as_str())
                        .copied()
                        .unwrap_or(Some(name))
                        && let Some(attributes) = mysql_column_attributes(create_table, stored)
                    {
                        self.kept_attributes.insert(name.clone(), attributes);
                    }
                }
                AlterOperation::ChangeColumn(..)
                | AlterOperation::AddIndex(_)
                | AlterOperation::DropIndex(_)
                | AlterOperation::AddForeignKey(_)
                | AlterOperation::DropForeignKey(_) => {}
            }
        }
    }

    /// Columns whose single-column indexes must be inspected before dropping.
    pub(crate) fn sqlite_dropped_columns(&self) -> Vec<&str> {
        if self.database_type != DatabaseType::SQLite {
            return Vec::new();
        }
        self.operations
            .iter()
            .enumerate()
            .filter_map(|(position, operation)| {
                let AlterOperation::DropColumn(name) = operation else {
                    return None;
                };
                let mut original = name.as_str();
                for earlier in self.operations[..position].iter().rev() {
                    match earlier {
                        AlterOperation::RenameColumn(from, to) if to == original => {
                            original = from;
                        }
                        AlterOperation::AddColumn(column, _) if column.name == original => {
                            return None;
                        }
                        _ => {}
                    }
                }
                Some(original)
            })
            .collect()
    }

    /// Drop these indexes, which [`Schema::alter_table`](super::Schema::alter_table)
    /// found on the table over a column the alteration drops, with the column.
    pub(crate) fn drop_sqlite_column_indexes(
        &mut self,
        schema: String,
        names: Vec<(String, String)>,
    ) {
        self.sqlite_index_schema = Some(schema);
        self.sqlite_column_indexes = names;
    }

    /// The table without its schema, which the names derived from it
    /// (indexes, foreign keys) use.
    fn table(&self) -> &str {
        ddl::table_parts(self.database_type, &self.name).1
    }
}

#[derive(Debug, Clone)]
enum AlterOperation {
    AddColumn(ColumnDefinition, Option<ForeignKey>),
    DropColumn(String),
    RenameColumn(String, String),
    ChangeColumn(String, ColumnChange),
    AddIndex(IndexDefinition),
    DropIndex(String),
    AddForeignKey(ForeignKey),
    DropForeignKey(String),
}
