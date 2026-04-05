use super::{ColumnType, DatabaseType, DefaultValue, quote_identifier_for_db};

mod sql_building;

/// Definition of a composite unique constraint
#[derive(Debug, Clone)]
pub struct UniqueConstraint {
    /// Optional name for the constraint
    pub name: Option<String>,
    /// Columns that form the unique constraint
    pub columns: Vec<String>,
}

/// Definition of a composite primary key
#[derive(Debug, Clone)]
pub struct CompositePrimaryKey {
    /// Columns that form the composite primary key
    pub columns: Vec<String>,
}

/// Builder for creating tables
pub struct TableBuilder {
    name: String,
    database_type: DatabaseType,
    columns: Vec<ColumnDefinition>,
    indexes: Vec<IndexBuilder>,
    primary_key: Option<String>,
    unique_constraints: Vec<UniqueConstraint>,
    composite_primary_key: Option<CompositePrimaryKey>,
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
        let column = ColumnDefinition {
            name: name.to_string(),
            column_type: ColumnType::BigInteger,
            nullable: false,
            default: None,
            primary_key: true,
            auto_increment: true,
            unique: false,
            check: None,
            extra: None,
        };
        self.columns.push(column);
        self.primary_key = Some(name.to_string());
        self
    }

    /// Add an auto-incrementing integer column
    pub fn increments(&mut self, name: &str) -> &mut Self {
        let column = ColumnDefinition {
            name: name.to_string(),
            column_type: ColumnType::Integer,
            nullable: false,
            default: None,
            primary_key: true,
            auto_increment: true,
            unique: false,
            check: None,
            extra: None,
        };
        self.columns.push(column);
        self.primary_key = Some(name.to_string());
        self
    }

    pub fn string(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::String)
    }

    pub fn text(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Text)
    }

    pub fn integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Integer)
    }

    pub fn big_integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::BigInteger)
    }

    pub fn small_integer(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::SmallInteger)
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

    pub fn float(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Float)
    }

    pub fn double(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Double)
    }

    pub fn boolean(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Boolean)
    }

    pub fn date(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Date)
    }

    pub fn time(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Time)
    }

    pub fn datetime(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::DateTime)
    }

    pub fn timestamp(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Timestamp)
    }

    pub fn timestamptz(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::TimestampTz)
    }

    pub fn timestamps(&mut self) -> &mut Self {
        self.column("created_at", ColumnType::TimestampTz)
            .default_now()
            .not_null();
        self.column("updated_at", ColumnType::TimestampTz)
            .default_now()
            .not_null();
        self
    }

    pub fn timestamps_naive(&mut self) -> &mut Self {
        self.column("created_at", ColumnType::Timestamp)
            .default_now()
            .not_null();
        self.column("updated_at", ColumnType::Timestamp)
            .default_now()
            .not_null();
        self
    }

    pub fn soft_deletes(&mut self) -> &mut Self {
        self.column("deleted_at", ColumnType::TimestampTz)
            .nullable();
        self
    }

    pub fn uuid(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Uuid)
    }

    pub fn json(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Json)
    }

    pub fn jsonb(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Jsonb)
    }

    pub fn binary(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::Binary)
    }

    pub fn integer_array(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::IntegerArray)
    }

    pub fn text_array(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::TextArray)
    }

    pub fn column(&mut self, name: &str, column_type: ColumnType) -> ColumnBuilder<'_> {
        ColumnBuilder {
            table: self,
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

    pub fn foreign_id(&mut self, name: &str) -> ColumnBuilder<'_> {
        self.column(name, ColumnType::BigInteger)
    }

    pub fn index(&mut self, columns: &[&str]) -> &mut Self {
        let index = IndexBuilder {
            name: format!("idx_{}_{}", self.name, columns.join("_")),
            columns: columns.iter().map(|value| value.to_string()).collect(),
            unique: false,
        };
        self.indexes.push(index);
        self
    }

    pub fn unique_index(&mut self, columns: &[&str]) -> &mut Self {
        let index = IndexBuilder {
            name: format!("idx_{}_{}_unique", self.name, columns.join("_")),
            columns: columns.iter().map(|value| value.to_string()).collect(),
            unique: true,
        };
        self.indexes.push(index);
        self
    }

    pub fn unique(&mut self, columns: &[&str]) -> &mut Self {
        self.unique_constraints.push(UniqueConstraint {
            name: None,
            columns: columns.iter().map(|value| value.to_string()).collect(),
        });
        self
    }

    pub fn unique_named(&mut self, name: &str, columns: &[&str]) -> &mut Self {
        self.unique_constraints.push(UniqueConstraint {
            name: Some(name.to_string()),
            columns: columns.iter().map(|value| value.to_string()).collect(),
        });
        self
    }

    pub fn primary_key(&mut self, columns: &[&str]) -> &mut Self {
        self.composite_primary_key = Some(CompositePrimaryKey {
            columns: columns.iter().map(|value| value.to_string()).collect(),
        });
        self
    }

    pub fn index_named(&mut self, name: &str, columns: &[&str]) -> &mut Self {
        let index = IndexBuilder {
            name: name.to_string(),
            columns: columns.iter().map(|value| value.to_string()).collect(),
            unique: false,
        };
        self.indexes.push(index);
        self
    }
}

/// Builder for column definitions (fluent API)
pub struct ColumnBuilder<'a> {
    table: &'a mut TableBuilder,
    definition: ColumnDefinition,
}

impl<'a> ColumnBuilder<'a> {
    /// Mark the column as NOT NULL
    pub fn not_null(mut self) -> Self {
        self.definition.nullable = false;
        self
    }

    /// Mark the column as nullable
    pub fn nullable(mut self) -> Self {
        self.definition.nullable = true;
        self
    }

    /// Set a default value
    pub fn default(mut self, value: impl Into<DefaultValue>) -> Self {
        self.definition.default = Some(value.into().to_sql());
        self
    }

    /// Set default to current timestamp
    pub fn default_now(mut self) -> Self {
        self.definition.default = Some("CURRENT_TIMESTAMP".to_string());
        self
    }

    /// Mark the column as unique
    pub fn unique(mut self) -> Self {
        self.definition.unique = true;
        self
    }

    /// Mark as primary key
    pub fn primary_key(mut self) -> Self {
        self.definition.primary_key = true;
        self.definition.nullable = false;
        self.table.primary_key = Some(self.definition.name.clone());
        self
    }

    /// Add a CHECK constraint to the column
    pub fn check(mut self, expression: &str) -> Self {
        self.definition.check = Some(expression.to_string());
        self
    }

    /// Add extra SQL to the column definition
    pub fn extra(mut self, sql: &str) -> Self {
        self.definition.extra = Some(sql.to_string());
        self
    }
}

impl<'a> Drop for ColumnBuilder<'a> {
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
            self.table.columns.push(definition);
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ColumnDefinition {
    pub(super) name: String,
    pub(super) column_type: ColumnType,
    pub(super) nullable: bool,
    pub(super) default: Option<String>,
    pub(super) primary_key: bool,
    pub(super) auto_increment: bool,
    pub(super) unique: bool,
    pub(super) check: Option<String>,
    pub(super) extra: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct IndexBuilder {
    pub(super) name: String,
    pub(super) columns: Vec<String>,
    pub(super) unique: bool,
}
