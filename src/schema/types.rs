use crate::model::IndexDefinition;

/// Schema for a single table
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Table name
    pub name: String,
    /// Optional schema name for qualified table references
    pub schema_name: Option<String>,
    /// Column definitions
    pub columns: Vec<ColumnSchema>,
    /// Index definitions (regular and unique)
    pub indexes: Vec<IndexDefinition>,
    /// Primary key column names, in key order.
    pub primary_keys: Vec<String>,
}

/// Schema for a single column
#[derive(Debug, Clone)]
pub struct ColumnSchema {
    /// Column name
    pub name: String,
    /// SQL type (e.g., "BIGINT", "TEXT", "TIMESTAMP")
    pub sql_type: String,
    /// Whether the column allows NULL values
    pub nullable: bool,
    /// Default value expression (e.g., "now()", "'active'")
    pub default: Option<String>,
    /// Whether this column is the primary key
    pub primary_key: bool,
    /// Whether this column auto-increments
    pub auto_increment: bool,
}

/// Builder for table schemas
pub struct TableSchemaBuilder {
    name: String,
    schema_name: Option<String>,
    columns: Vec<ColumnSchema>,
    indexes: Vec<IndexDefinition>,
    primary_keys: Vec<String>,
}

impl TableSchemaBuilder {
    /// Create a new table schema builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema_name: None,
            columns: Vec::new(),
            indexes: Vec::new(),
            primary_keys: Vec::new(),
        }
    }

    /// Set the schema name for this table.
    #[must_use]
    pub fn schema(mut self, schema_name: impl Into<String>) -> Self {
        self.schema_name = Some(schema_name.into());
        self
    }

    /// Add a column
    #[must_use]
    pub fn column(mut self, schema: ColumnSchema) -> Self {
        if schema.primary_key {
            self.primary_keys.push(schema.name.clone());
        }
        self.columns.push(schema);
        self
    }

    /// Add an index
    #[must_use]
    pub fn index(mut self, index: IndexDefinition) -> Self {
        self.indexes.push(index);
        self
    }

    /// Add multiple indexes
    #[must_use]
    pub fn indexes(mut self, indexes: impl IntoIterator<Item = IndexDefinition>) -> Self {
        self.indexes.extend(indexes);
        self
    }

    /// Build the table schema
    #[must_use]
    pub fn build(self) -> TableSchema {
        TableSchema {
            name: self.name,
            schema_name: self.schema_name,
            columns: self.columns,
            indexes: self.indexes,
            primary_keys: self.primary_keys,
        }
    }
}

/// The constructor and builder methods of a column declaration, shared by
/// [`ColumnSchema`] (an SQL type) and sync's `ColumnDef` (a [`ColumnType`]):
/// a column is nullable until told otherwise, and a primary key is `NOT NULL`.
///
/// [`ColumnType`]: crate::migration::ColumnType
macro_rules! column_declaration_methods {
    ($declaration:ident, $type_field:ident: $type:ty) => {
        impl $declaration {
            /// Declare a nullable column named `name`.
            #[must_use]
            pub fn new(name: impl Into<String>, $type_field: $type) -> Self {
                Self {
                    name: name.into(),
                    $type_field: $type_field.into(),
                    nullable: true,
                    default: None,
                    primary_key: false,
                    auto_increment: false,
                }
            }

            /// Make the column the primary key, which also makes it `NOT NULL`.
            #[must_use]
            pub fn primary_key(mut self) -> Self {
                self.primary_key = true;
                self.nullable = false;
                self
            }

            /// Make the column auto-increment.
            #[must_use]
            pub fn auto_increment(mut self) -> Self {
                self.auto_increment = true;
                self
            }

            /// Make the column `NOT NULL`.
            #[must_use]
            pub fn not_null(mut self) -> Self {
                self.nullable = false;
                self
            }

            /// Set the column's default, an SQL expression.
            #[must_use]
            pub fn default(mut self, value: impl Into<String>) -> Self {
                self.default = Some(value.into());
                self
            }
        }
    };
}
pub(crate) use column_declaration_methods;

column_declaration_methods!(ColumnSchema, sql_type: impl Into<String>);
