/// Supported column types for migrations
#[derive(Debug, Clone)]
pub enum ColumnType {
    /// Small integer (2 bytes)
    SmallInteger,
    /// Integer (4 bytes)
    Integer,
    /// Big integer (8 bytes)
    BigInteger,
    /// Single precision float
    Float,
    /// Double precision float
    Double,
    /// Decimal with precision and scale
    Decimal {
        /// Total number of digits
        precision: u32,
        /// Number of digits after decimal point
        scale: u32,
    },
    /// Variable length string
    String,
    /// Text (unlimited length)
    Text,
    /// Boolean
    Boolean,
    /// Date
    Date,
    /// Time
    Time,
    /// DateTime
    DateTime,
    /// Timestamp (without time zone)
    Timestamp,
    /// Timestamp with time zone (PostgreSQL: TIMESTAMPTZ)
    /// Use this for `chrono::DateTime<Utc>` fields
    TimestampTz,
    /// UUID
    Uuid,
    /// JSON
    Json,
    /// JSONB (PostgreSQL)
    Jsonb,
    /// Binary/Blob
    Binary,
    /// Integer array (PostgreSQL)
    IntegerArray,
    /// Text array (PostgreSQL)
    TextArray,
    /// Custom SQL type
    Custom(String),
}

impl ColumnType {
    /// Convert to PostgreSQL SQL type
    pub fn to_postgres_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger => "SMALLINT".to_string(),
            ColumnType::Integer => "INTEGER".to_string(),
            ColumnType::BigInteger => "BIGINT".to_string(),
            ColumnType::Float => "REAL".to_string(),
            ColumnType::Double => "DOUBLE PRECISION".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::String => "VARCHAR(255)".to_string(),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Boolean => "BOOLEAN".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::DateTime => "TIMESTAMP".to_string(),
            ColumnType::Timestamp => "TIMESTAMP".to_string(),
            ColumnType::TimestampTz => "TIMESTAMPTZ".to_string(),
            ColumnType::Uuid => "UUID".to_string(),
            ColumnType::Json => "JSON".to_string(),
            ColumnType::Jsonb => "JSONB".to_string(),
            ColumnType::Binary => "BYTEA".to_string(),
            ColumnType::IntegerArray => "INTEGER[]".to_string(),
            ColumnType::TextArray => "TEXT[]".to_string(),
            ColumnType::Custom(sql) => sql.clone(),
        }
    }

    /// Convert to MySQL SQL type
    pub fn to_mysql_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger => "SMALLINT".to_string(),
            ColumnType::Integer => "INT".to_string(),
            ColumnType::BigInteger => "BIGINT".to_string(),
            ColumnType::Float => "FLOAT".to_string(),
            ColumnType::Double => "DOUBLE".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::String => "VARCHAR(255)".to_string(),
            ColumnType::Text => "TEXT".to_string(),
            ColumnType::Boolean => "TINYINT(1)".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME".to_string(),
            ColumnType::DateTime => "DATETIME".to_string(),
            ColumnType::Timestamp | ColumnType::TimestampTz => "TIMESTAMP".to_string(),
            ColumnType::Uuid => "CHAR(36)".to_string(),
            ColumnType::Json | ColumnType::Jsonb => "JSON".to_string(),
            ColumnType::Binary => "BLOB".to_string(),
            ColumnType::IntegerArray | ColumnType::TextArray => "JSON".to_string(),
            ColumnType::Custom(sql) => sql.clone(),
        }
    }

    /// Convert to SQLite SQL type
    pub fn to_sqlite_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::Boolean => "INTEGER".to_string(),
            ColumnType::Float | ColumnType::Double | ColumnType::Decimal { .. } => {
                "REAL".to_string()
            }
            ColumnType::String
            | ColumnType::Text
            | ColumnType::Uuid
            | ColumnType::Date
            | ColumnType::Time
            | ColumnType::DateTime
            | ColumnType::Timestamp
            | ColumnType::TimestampTz
            | ColumnType::Json
            | ColumnType::Jsonb
            | ColumnType::IntegerArray
            | ColumnType::TextArray => "TEXT".to_string(),
            ColumnType::Binary => "BLOB".to_string(),
            ColumnType::Custom(sql) => sql.clone(),
        }
    }
}

/// Default value for columns
#[derive(Debug, Clone)]
pub enum DefaultValue {
    /// String value
    String(String),
    /// Integer value
    Integer(i64),
    /// Float value
    Float(f64),
    /// Boolean value
    Boolean(bool),
    /// Raw SQL expression
    Raw(String),
    /// NULL
    Null,
}

impl DefaultValue {
    /// Convert to SQL representation
    pub fn to_sql(&self) -> String {
        match self {
            DefaultValue::String(value) => format!("'{}'", value.replace('\'', "''")),
            DefaultValue::Integer(value) => value.to_string(),
            DefaultValue::Float(value) => value.to_string(),
            DefaultValue::Boolean(value) => {
                if *value {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            }
            DefaultValue::Raw(value) => value.clone(),
            DefaultValue::Null => "NULL".to_string(),
        }
    }
}

impl From<&str> for DefaultValue {
    fn from(value: &str) -> Self {
        DefaultValue::String(value.to_string())
    }
}

impl From<String> for DefaultValue {
    fn from(value: String) -> Self {
        DefaultValue::String(value)
    }
}

impl From<i32> for DefaultValue {
    fn from(value: i32) -> Self {
        DefaultValue::Integer(value as i64)
    }
}

impl From<i64> for DefaultValue {
    fn from(value: i64) -> Self {
        DefaultValue::Integer(value)
    }
}

impl From<f64> for DefaultValue {
    fn from(value: f64) -> Self {
        DefaultValue::Float(value)
    }
}

impl From<bool> for DefaultValue {
    fn from(value: bool) -> Self {
        DefaultValue::Boolean(value)
    }
}
