use crate::config::DatabaseType;
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
    /// Primary key column name
    pub primary_key: String,
    /// Primary key column names, in declaration order.
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

/// Builder for table schemas from model metadata
pub struct TableSchemaBuilder {
    name: String,
    schema_name: Option<String>,
    columns: Vec<ColumnSchema>,
    indexes: Vec<IndexDefinition>,
    primary_key: String,
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
            primary_key: String::new(),
            primary_keys: Vec::new(),
        }
    }

    /// Set the schema name for this table.
    pub fn schema(mut self, schema_name: impl Into<String>) -> Self {
        self.schema_name = Some(schema_name.into());
        self
    }

    /// Add a column
    pub fn column(mut self, schema: ColumnSchema) -> Self {
        if schema.primary_key {
            if self.primary_key.is_empty() {
                self.primary_key = schema.name.clone();
            }
            self.primary_keys.push(schema.name.clone());
        }
        self.columns.push(schema);
        self
    }

    /// Add a BIGINT column
    pub fn bigint(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "BIGINT"))
    }

    /// Add an INTEGER column
    pub fn integer(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "INTEGER"))
    }

    /// Add a SMALLINT column
    pub fn smallint(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "SMALLINT"))
    }

    /// Add a TEXT column
    pub fn text(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TEXT"))
    }

    /// Add a VARCHAR column with specified length
    pub fn varchar(self, name: impl Into<String>, length: u32) -> Self {
        self.column(ColumnSchema::new(name, format!("VARCHAR({})", length)))
    }

    /// Add a BOOLEAN column
    pub fn boolean(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "BOOLEAN"))
    }

    /// Add a TIMESTAMP column (without time zone)
    pub fn timestamp(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TIMESTAMP"))
    }

    /// Add a TIMESTAMPTZ column (timestamp with time zone) - use for DateTime<Utc>
    pub fn timestamptz(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TIMESTAMPTZ"))
    }

    /// Add a DATE column
    pub fn date(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "DATE"))
    }

    /// Add a TIME column
    pub fn time(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "TIME"))
    }

    /// Add a UUID column
    pub fn uuid(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "UUID"))
    }

    /// Add a DECIMAL column
    pub fn decimal(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "DECIMAL"))
    }

    /// Add a DECIMAL column with precision and scale
    pub fn decimal_with_precision(
        self,
        name: impl Into<String>,
        precision: u32,
        scale: u32,
    ) -> Self {
        self.column(ColumnSchema::new(
            name,
            format!("DECIMAL({},{})", precision, scale),
        ))
    }

    /// Add a JSONB column (PostgreSQL)
    pub fn jsonb(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "JSONB"))
    }

    /// Add a JSON column
    pub fn json(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "JSON"))
    }

    /// Add a BYTEA column (PostgreSQL binary)
    pub fn bytea(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "BYTEA"))
    }

    /// Add an REAL (single precision float) column
    pub fn real(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "REAL"))
    }

    /// Add a DOUBLE PRECISION column
    pub fn double(self, name: impl Into<String>) -> Self {
        self.column(ColumnSchema::new(name, "DOUBLE PRECISION"))
    }

    /// Add an index
    pub fn index(mut self, index: IndexDefinition) -> Self {
        self.indexes.push(index);
        self
    }

    /// Add multiple indexes
    pub fn indexes(mut self, indexes: Vec<IndexDefinition>) -> Self {
        self.indexes.extend(indexes);
        self
    }

    /// Build the table schema
    pub fn build(self) -> TableSchema {
        TableSchema {
            name: self.name,
            schema_name: self.schema_name,
            columns: self.columns,
            indexes: self.indexes,
            primary_key: self.primary_key,
            primary_keys: self.primary_keys,
        }
    }
}

impl ColumnSchema {
    /// Create a new column schema
    pub fn new(name: impl Into<String>, sql_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sql_type: sql_type.into(),
            nullable: true,
            default: None,
            primary_key: false,
            auto_increment: false,
        }
    }

    /// Mark as primary key
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    /// Mark as auto increment
    pub fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }

    /// Mark as not nullable
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// Set default value
    pub fn default(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }
}

/// Utility to map Rust types to SQL types
pub fn rust_type_to_sql(rust_type: &str, db_type: DatabaseType) -> String {
    let normalized: String = rust_type.chars().filter(|c| !c.is_whitespace()).collect();

    let base_type = if normalized.starts_with("Option<") && normalized.ends_with(">") {
        normalized[7..normalized.len() - 1].to_string()
    } else {
        normalized
    };

    let base_type = base_type
        .replace("&", "")
        .replace("'static", "")
        .trim()
        .to_string();

    match db_type {
        DatabaseType::Postgres => match base_type.as_str() {
            "i8" | "i16" => "SMALLINT".to_string(),
            "i32" => "INTEGER".to_string(),
            "i64" => "BIGINT".to_string(),
            "u8" | "u16" => "SMALLINT".to_string(),
            "u32" => "INTEGER".to_string(),
            "u64" => "BIGINT".to_string(),
            "f32" => "REAL".to_string(),
            "f64" => "DOUBLE PRECISION".to_string(),
            "bool" => "BOOLEAN".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "UUID".to_string(),
            "DateTime<Utc>" | "chrono::DateTime<Utc>" | "chrono::DateTime<chrono::Utc>" => {
                "TIMESTAMPTZ".to_string()
            }
            "DateTime" | "NaiveDateTime" => "TIMESTAMP".to_string(),
            "NaiveDate" => "DATE".to_string(),
            "NaiveTime" => "TIME".to_string(),
            "Decimal" => "DECIMAL".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "JSONB".to_string(),
            "Vec<u8>" => "BYTEA".to_string(),
            "Vec<i32>" | "IntArray" => "INTEGER[]".to_string(),
            "Vec<i64>" | "BigIntArray" => "BIGINT[]".to_string(),
            "Vec<String>" | "TextArray" => "TEXT[]".to_string(),
            "Vec<bool>" | "BoolArray" => "BOOLEAN[]".to_string(),
            "Vec<f64>" | "FloatArray" => "DOUBLE PRECISION[]".to_string(),
            "Vec<serde_json::Value>" | "JsonArray" => "JSONB[]".to_string(),
            _ => "TEXT".to_string(),
        },
        DatabaseType::MySQL | DatabaseType::MariaDB => match base_type.as_str() {
            "i8" | "i16" => "SMALLINT".to_string(),
            "i32" => "INT".to_string(),
            "i64" => "BIGINT".to_string(),
            "u8" | "u16" => "SMALLINT UNSIGNED".to_string(),
            "u32" => "INT UNSIGNED".to_string(),
            "u64" => "BIGINT UNSIGNED".to_string(),
            "f32" => "FLOAT".to_string(),
            "f64" => "DOUBLE".to_string(),
            "bool" => "TINYINT(1)".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "CHAR(36)".to_string(),
            "DateTime<Utc>" | "DateTime" | "NaiveDateTime" => "DATETIME".to_string(),
            "NaiveDate" => "DATE".to_string(),
            "NaiveTime" => "TIME".to_string(),
            "Decimal" => "DECIMAL(65,30)".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "JSON".to_string(),
            "Vec<u8>" => "BLOB".to_string(),
            "Vec<i32>" | "IntArray" => "JSON".to_string(),
            "Vec<i64>" | "BigIntArray" => "JSON".to_string(),
            "Vec<String>" | "TextArray" => "JSON".to_string(),
            "Vec<bool>" | "BoolArray" => "JSON".to_string(),
            "Vec<f64>" | "FloatArray" => "JSON".to_string(),
            "Vec<serde_json::Value>" | "JsonArray" => "JSON".to_string(),
            _ => "TEXT".to_string(),
        },
        DatabaseType::SQLite => match base_type.as_str() {
            "i8" | "i16" | "i32" | "i64" => "INTEGER".to_string(),
            "u8" | "u16" | "u32" | "u64" => "INTEGER".to_string(),
            "f32" | "f64" => "REAL".to_string(),
            "bool" => "INTEGER".to_string(),
            "String" | "str" => "TEXT".to_string(),
            "Uuid" => "TEXT".to_string(),
            "DateTime<Utc>" | "DateTime" | "NaiveDateTime" | "NaiveDate" | "NaiveTime" => {
                "TEXT".to_string()
            }
            "Decimal" => "TEXT".to_string(),
            "Json" | "JsonValue" | "Value" | "serde_json::Value" => "TEXT".to_string(),
            "Vec<u8>" => "BLOB".to_string(),
            _ => "TEXT".to_string(),
        },
    }
}