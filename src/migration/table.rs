use crate::internal::sql_safety::quote_ident;
use crate::model::IndexDefinition;

use super::ddl::{self, ColumnDefinition};
use super::{ColumnType, DatabaseType, DefaultValue};

/// A multi-column `UNIQUE` table constraint.
#[derive(Debug, Clone)]
struct UniqueConstraint {
    name: Option<String>,
    columns: Vec<String>,
}

impl UniqueConstraint {
    fn to_sql(&self, db_type: DatabaseType) -> String {
        let columns = crate::internal::sql_safety::column_list(db_type, &self.columns, "");
        match &self.name {
            Some(name) => format!(
                "CONSTRAINT {} UNIQUE ({})",
                quote_ident(db_type, name),
                columns
            ),
            None => format!("UNIQUE ({})", columns),
        }
    }
}

/// One `TableBuilder` method per column type: `name(column)` adds a column of
/// that type.
macro_rules! column_methods {
    ($($(#[$doc:meta])* $method:ident => $column_type:expr;)*) => {
        $(
            $(#[$doc])*
            pub fn $method(&mut self, name: &str) -> ColumnBuilder<'_> {
                self.column(name, $column_type)
            }
        )*
    };
}

/// Builder for creating tables
pub struct TableBuilder {
    name: String,
    database_type: DatabaseType,
    columns: Vec<ColumnDefinition>,
    indexes: Vec<IndexDefinition>,
    primary_key: Option<String>,
    unique_constraints: Vec<UniqueConstraint>,
    composite_primary_key: Option<Vec<String>>,
}

impl TableBuilder {
    /// Create a new table builder
    pub fn new(name: &str, database_type: DatabaseType) -> Self {
        Self {
            name: name.to_string(),
            database_type,
            columns: Vec::new(),
            indexes: Vec::new(),
            primary_key: None,
            unique_constraints: Vec::new(),
            composite_primary_key: None,
        }
    }

    /// Add an auto-incrementing primary key column named "id"
    pub fn id(&mut self) -> &mut Self {
        self.big_increments("id")
    }

    /// Add an auto-incrementing big integer column
    pub fn big_increments(&mut self, name: &str) -> &mut Self {
        self.increments_column(name, ColumnType::BigInteger)
    }

    /// Add an auto-incrementing integer column
    pub fn increments(&mut self, name: &str) -> &mut Self {
        self.increments_column(name, ColumnType::Integer)
    }

    fn increments_column(&mut self, name: &str, column_type: ColumnType) -> &mut Self {
        let mut column = ColumnDefinition::new(name, column_type);
        column.nullable = false;
        column.primary_key = true;
        column.auto_increment = true;
        self.columns.push(column);
        self.primary_key = Some(name.to_string());
        self
    }

    column_methods! {
        /// A `VARCHAR(255)` column.
        string => ColumnType::String;
        /// A text column: `TEXT`, `LONGTEXT` on MySQL and MariaDB.
        text => ColumnType::Text;
        /// A 32-bit integer column.
        integer => ColumnType::Integer;
        /// A 64-bit integer column.
        big_integer => ColumnType::BigInteger;
        /// A 16-bit integer column.
        small_integer => ColumnType::SmallInteger;
        /// A single-precision float column.
        float => ColumnType::Float;
        /// A double-precision float column.
        double => ColumnType::Double;
        /// A boolean column.
        boolean => ColumnType::Boolean;
        /// A date column.
        date => ColumnType::Date;
        /// A time-of-day column.
        time => ColumnType::Time;
        /// A timestamp column without a time zone.
        datetime => ColumnType::DateTime;
        /// A timestamp column without a time zone.
        timestamp => ColumnType::Timestamp;
        /// A timestamp column with a time zone where the backend has one.
        timestamptz => ColumnType::TimestampTz;
        /// A UUID column.
        uuid => ColumnType::Uuid;
        /// A JSON column.
        json => ColumnType::Json;
        /// A binary JSON column: `JSONB` on PostgreSQL, JSON elsewhere.
        jsonb => ColumnType::Jsonb;
        /// A binary column.
        binary => ColumnType::Binary;
        /// An integer array column: PostgreSQL only, JSON on MySQL.
        integer_array => ColumnType::IntegerArray;
        /// A text array column: PostgreSQL only, JSON on MySQL.
        text_array => ColumnType::TextArray;
        /// A 64-bit integer column holding another table's key.
        foreign_id => ColumnType::BigInteger;
    }

    /// A `VARCHAR(length)` column; [`string`](Self::string) is `VARCHAR(255)`.
    ///
    /// SQLite ignores the length, so a migration it accepts can still fail
    /// elsewhere: `length` must be at least 1, at most 10,485,760 on PostgreSQL,
    /// and at most 16,383 on MySQL and MariaDB, whose tables TideORM declares
    /// `utf8mb4`; a unique or indexed one at most 768 there.
    pub fn string_with(&mut self, name: &str, length: u32) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Varchar(length))
    }

    pub fn decimal(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(
            name,
            ColumnType::Decimal {
                precision: 10,
                scale: 2,
            },
        )
    }

    pub fn decimal_with(&mut self, name: &str, precision: u32, scale: u32) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Decimal { precision, scale })
    }

    /// `created_at` and `updated_at`, with a time zone, defaulting to now.
    pub fn timestamps(&mut self) -> &mut Self {
        self.timestamps_of(ColumnType::TimestampTz)
    }

    /// `created_at` and `updated_at`, without a time zone, defaulting to now.
    pub fn timestamps_naive(&mut self) -> &mut Self {
        self.timestamps_of(ColumnType::Timestamp)
    }

    fn timestamps_of(&mut self, column_type: ColumnType) -> &mut Self {
        for name in ["created_at", "updated_at"] {
            self.column(name, column_type.clone())
                .default_now()
                .not_null();
        }
        self
    }

    pub fn soft_deletes(&mut self) -> &mut Self {
        self.column("deleted_at", ColumnType::TimestampTz)
            .nullable();
        self
    }

    pub fn column(&mut self, name: &str, column_type: ColumnType) -> ColumnBuilder<'_> {
        let database_type = self.database_type;
        ColumnBuilder::new(
            self,
            database_type,
            ColumnDefinition::new(name, column_type),
            |table, column| table.columns.push(column),
        )
    }

    /// Index `columns` under a generated name, `idx_<table>_<columns>`,
    /// shortened with a hash past 63 bytes.
    pub fn index(&mut self, columns: &[&str]) -> &mut Self {
        let name = ddl::index_name(&self.name, columns, false);
        self.push_index(name, columns, false)
    }

    /// A unique index on `columns`, named `idx_<table>_<columns>_unique`
    /// (shortened with a hash past 63 bytes).
    pub fn unique_index(&mut self, columns: &[&str]) -> &mut Self {
        let name = ddl::index_name(&self.name, columns, true);
        self.push_index(name, columns, true)
    }

    fn push_index(&mut self, name: String, columns: &[&str], unique: bool) -> &mut Self {
        self.indexes.push(IndexDefinition::new(
            name,
            columns.iter().map(|column| column.to_string()).collect(),
            unique,
        ));
        self
    }

    pub fn unique(&mut self, columns: &[&str]) -> &mut Self {
        self.push_unique_constraint(None, columns)
    }

    pub fn unique_named(&mut self, name: &str, columns: &[&str]) -> &mut Self {
        self.push_unique_constraint(Some(name.to_string()), columns)
    }

    fn push_unique_constraint(&mut self, name: Option<String>, columns: &[&str]) -> &mut Self {
        self.unique_constraints.push(UniqueConstraint {
            name,
            columns: columns.iter().map(|column| column.to_string()).collect(),
        });
        self
    }

    pub fn primary_key(&mut self, columns: &[&str]) -> &mut Self {
        self.composite_primary_key =
            Some(columns.iter().map(|column| column.to_string()).collect());
        self
    }

    /// The `CREATE TABLE IF NOT EXISTS` statement this table renders, for
    /// the CLI's migration generator, which writes it into a migration file.
    #[doc(hidden)]
    pub fn __create_sql(&self) -> String {
        self.build_create(true)
    }

    pub(crate) fn build_create(&self, if_not_exists: bool) -> String {
        // A table carries exactly one PRIMARY KEY clause. When both a
        // column-level key (`id()`, `.primary_key()`) and an explicit composite
        // key are declared, the explicit composite one wins - emitting both
        // produces SQL every backend rejects.
        let primary_key = match (&self.composite_primary_key, &self.primary_key) {
            (Some(columns), _) => columns.clone(),
            (None, Some(column)) => vec![column.clone()],
            (None, None) => Vec::new(),
        };
        let constraints: Vec<String> = self
            .unique_constraints
            .iter()
            .map(|constraint| constraint.to_sql(self.database_type))
            .collect();

        ddl::create_table(
            self.database_type,
            &quote_ident(self.database_type, &self.name),
            if_not_exists,
            &self.columns,
            &primary_key,
            &constraints,
        )
    }

    /// Each index as `(name, CREATE INDEX statement)`.
    pub(crate) fn build_indexes(&self, if_not_exists: bool) -> Vec<(&str, String)> {
        let table = quote_ident(self.database_type, &self.name);

        self.indexes
            .iter()
            .map(|index| {
                let sql = ddl::create_index(
                    self.database_type,
                    &index.name,
                    &table,
                    &index.columns,
                    index.unique,
                    if_not_exists,
                );
                (index.name.as_str(), sql)
            })
            .collect()
    }
}

/// Builder for column definitions (fluent API)
///
/// The column is added when the builder drops, so a chain such as
/// `t.string("email").unique().not_null();` needs no terminating call.
pub struct ColumnBuilder<'a, T = TableBuilder> {
    target: &'a mut T,
    database_type: DatabaseType,
    definition: Option<ColumnDefinition>,
    add: fn(&mut T, ColumnDefinition),
}

impl<'a, T> ColumnBuilder<'a, T> {
    pub(super) fn new(
        target: &'a mut T,
        database_type: DatabaseType,
        definition: ColumnDefinition,
        add: fn(&mut T, ColumnDefinition),
    ) -> Self {
        Self {
            target,
            database_type,
            definition: Some(definition),
            add,
        }
    }

    fn definition(&mut self) -> &mut ColumnDefinition {
        self.definition
            .as_mut()
            .expect("the definition is only taken when the builder drops")
    }

    /// Mark the column as NOT NULL
    pub fn not_null(mut self) -> Self {
        self.definition().nullable = false;
        self
    }

    /// Mark the column as nullable
    pub fn nullable(mut self) -> Self {
        self.definition().nullable = true;
        self
    }

    /// Set a default value
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        let default = value.into().to_sql(self.database_type);
        self.definition().default = Some(default);
        self
    }

    /// Set default to current timestamp
    pub fn default_now(mut self) -> Self {
        self.definition().default = Some("CURRENT_TIMESTAMP".to_string());
        self
    }

    /// Mark the column as unique
    pub fn unique(mut self) -> Self {
        self.definition().unique = true;
        self
    }
}

impl ColumnBuilder<'_, TableBuilder> {
    /// Mark as primary key
    pub fn primary_key(mut self) -> Self {
        let definition = self.definition();
        definition.primary_key = true;
        definition.nullable = false;
        let name = definition.name.clone();
        self.target.primary_key = Some(name);
        self
    }

    /// Add a CHECK constraint to the column
    pub fn check(mut self, expression: &str) -> Self {
        self.definition().check = Some(expression.to_string());
        self
    }

    /// Add extra SQL to the column definition
    pub fn extra(mut self, sql: &str) -> Self {
        self.definition().extra = Some(sql.to_string());
        self
    }
}

impl<T> Drop for ColumnBuilder<'_, T> {
    fn drop(&mut self) {
        if let Some(definition) = self.definition.take() {
            (self.add)(self.target, definition);
        }
    }
}
