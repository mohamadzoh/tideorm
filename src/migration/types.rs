use crate::config::DatabaseType;

/// Supported column types for migrations.
///
/// This enum is TideORM's single logical column vocabulary: migrations name a
/// variant directly, and schema sync maps every Rust type TideORM knows how to
/// store onto one through
/// [`rust_type_to_column_type`](crate::schema::rust_type_to_column_type). The
/// three backend renderers below are the only place a logical type becomes SQL
/// text, and migrations and sync render whole column definitions through the
/// same DDL builder, so a table one of them creates matches the table the other
/// would have created.
#[derive(Debug, Clone)]
pub enum ColumnType {
    /// Small integer (2 bytes)
    SmallInteger,
    /// Integer (4 bytes)
    Integer,
    /// Big integer (8 bytes)
    BigInteger,
    /// Unsigned 8-bit integer (`u8`)
    TinyUnsigned,
    /// Unsigned 16-bit integer (`u16`)
    SmallUnsigned,
    /// Unsigned 32-bit integer (`u32`)
    Unsigned,
    /// Unsigned 64-bit integer (`u64`)
    BigUnsigned,
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
    /// Exact numeric with the backend's widest default precision.
    ///
    /// This is what `rust_decimal::Decimal` / `BigDecimal` fields map to when
    /// the model does not pin a precision.
    Numeric,
    /// Variable length string
    String,
    /// Variable length string of at most this many characters: `VARCHAR(n)`,
    /// and `TEXT` on SQLite, which ignores a declared length.
    Varchar(u32),
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
    /// Big integer array (PostgreSQL)
    BigIntegerArray,
    /// Text array (PostgreSQL)
    TextArray,
    /// Boolean array (PostgreSQL)
    BooleanArray,
    /// Double precision array (PostgreSQL)
    DoubleArray,
    /// JSON array (PostgreSQL: `JSONB[]`)
    JsonArray,
    /// Custom SQL type
    Custom(String),
}

impl ColumnType {
    /// Render this logical type as SQL for `db_type`.
    ///
    /// This is the dispatcher every caller should reach for; the three
    /// per-backend methods stay public for callers that already know the
    /// backend. MySQL and MariaDB share one rendering - no type in this enum is
    /// spelled differently between them.
    pub fn to_sql(&self, db_type: DatabaseType) -> String {
        match db_type {
            DatabaseType::Postgres => self.to_postgres_sql(),
            DatabaseType::MySQL | DatabaseType::MariaDB => self.to_mysql_sql(),
            DatabaseType::SQLite => self.to_sqlite_sql(),
        }
    }

    /// Whether a column of this type can carry an auto-increment clause.
    ///
    /// A [`ColumnType::Custom`] type is taken at its word, since its SQL is
    /// whatever the caller declared.
    pub(crate) fn can_auto_increment(&self) -> bool {
        matches!(
            self,
            ColumnType::SmallInteger
                | ColumnType::Integer
                | ColumnType::BigInteger
                | ColumnType::TinyUnsigned
                | ColumnType::SmallUnsigned
                | ColumnType::Unsigned
                | ColumnType::BigUnsigned
                | ColumnType::Custom(_)
        )
    }

    /// Convert to PostgreSQL SQL type.
    ///
    /// PostgreSQL has no unsigned integer types, so the unsigned variants land
    /// on a signed one. The width is chosen by what the driver *speaks*, not by
    /// what would hold the widest value:
    ///
    /// * `u32` renders `INTEGER`. `sea-orm` decodes a `u32` column by asking
    ///   `sqlx` for an `Oid` and then falling back to an `i32`, and neither
    ///   accepts an `int8`, so widening to `BIGINT` makes the column
    ///   unreadable. This matches `sea-query`'s own PostgreSQL renderer.
    /// * `u64` renders `BIGINT`, again matching `sea-query`. `sea-orm` cannot
    ///   decode a `u64` on PostgreSQL at all, and the value binder converts one
    ///   to `i64` before it ever reaches the server, so an exact `NUMERIC`
    ///   column would only be writable through the same `i64`.
    /// * `u8` and `u16` do widen (`SMALLINT`, `INTEGER`), because nothing
    ///   downstream constrains them: neither type works on PostgreSQL at all -
    ///   `sea-orm` will not decode them and the value binder will not bind them
    ///   (`u8 unsupported by sqlx-postgres`) - so only hand-written SQL ever
    ///   reads these columns.
    ///
    /// Known limitation: a `u32` above `i32::MAX` or a `u64` above `i64::MAX`
    /// does not round-trip through PostgreSQL - the value is rejected rather
    /// than silently truncated. Store those as [`ColumnType::Custom`]
    /// (`NUMERIC(20,0)`) plus a string or `i64` field if the full range is
    /// required.
    pub fn to_postgres_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger => "SMALLINT".to_string(),
            ColumnType::Integer => "INTEGER".to_string(),
            ColumnType::BigInteger => "BIGINT".to_string(),
            ColumnType::TinyUnsigned => "SMALLINT".to_string(),
            ColumnType::SmallUnsigned => "INTEGER".to_string(),
            ColumnType::Unsigned => "INTEGER".to_string(),
            ColumnType::BigUnsigned => "BIGINT".to_string(),
            ColumnType::Float => "REAL".to_string(),
            ColumnType::Double => "DOUBLE PRECISION".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::Numeric => "DECIMAL".to_string(),
            ColumnType::String => "VARCHAR(255)".to_string(),
            ColumnType::Varchar(length) => format!("VARCHAR({})", length),
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
            ColumnType::BigIntegerArray => "BIGINT[]".to_string(),
            ColumnType::TextArray => "TEXT[]".to_string(),
            ColumnType::BooleanArray => "BOOLEAN[]".to_string(),
            ColumnType::DoubleArray => "DOUBLE PRECISION[]".to_string(),
            ColumnType::JsonArray => "JSONB[]".to_string(),
            ColumnType::Custom(sql) => sql.clone(),
        }
    }

    /// Convert to MySQL SQL type.
    ///
    /// MySQL is the one backend with native unsigned integers, so nothing
    /// widens here. It has no array type either, so the array variants are
    /// stored as `JSON`.
    ///
    /// Three renderings avoid MySQL defaults that silently lose data:
    ///
    /// * Text and binary are `LONGTEXT`/`LONGBLOB`. Plain `TEXT` and `BLOB`
    ///   stop at 64 KB, which is not the unlimited column the other backends
    ///   give a `String` or `Vec<u8>`.
    /// * Times carry six fractional digits. A bare `TIME`/`DATETIME` rounds a
    ///   `NaiveTime` or `Utc::now()` to the whole second on the way in.
    /// * Both timestamp flavors are `DATETIME(6)`. MySQL's `TIMESTAMP` only
    ///   spans 1970 to 2038, so a birth date or a far expiry was rejected.
    ///   `DATETIME` has no session-zone shifting, and sqlx pins every
    ///   session to `+00:00`, so a `DateTime<Utc>` is stored and read as UTC.
    ///
    /// [`ColumnType::Uuid`] renders `BINARY(16)`, matching `sea-query`'s own
    /// MySQL renderer. `sqlx-mysql` encodes a `Uuid` as the 16 raw bytes and
    /// refuses to decode anything else, so a `CHAR(36)` column rejects every
    /// insert with error 1366 (`Incorrect string value`). A model that wants
    /// the hyphenated text form has to declare the field as
    /// `uuid::fmt::Hyphenated` (a `String` column), not as `Uuid`.
    pub fn to_mysql_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger => "SMALLINT".to_string(),
            ColumnType::Integer => "INT".to_string(),
            ColumnType::BigInteger => "BIGINT".to_string(),
            ColumnType::TinyUnsigned => "TINYINT UNSIGNED".to_string(),
            ColumnType::SmallUnsigned => "SMALLINT UNSIGNED".to_string(),
            ColumnType::Unsigned => "INT UNSIGNED".to_string(),
            ColumnType::BigUnsigned => "BIGINT UNSIGNED".to_string(),
            ColumnType::Float => "FLOAT".to_string(),
            ColumnType::Double => "DOUBLE".to_string(),
            ColumnType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnType::Numeric => "DECIMAL(65,30)".to_string(),
            ColumnType::String => "VARCHAR(255)".to_string(),
            ColumnType::Varchar(length) => format!("VARCHAR({})", length),
            ColumnType::Text => "LONGTEXT".to_string(),
            ColumnType::Boolean => "TINYINT(1)".to_string(),
            ColumnType::Date => "DATE".to_string(),
            ColumnType::Time => "TIME(6)".to_string(),
            ColumnType::DateTime | ColumnType::Timestamp | ColumnType::TimestampTz => {
                "DATETIME(6)".to_string()
            }
            ColumnType::Uuid => "BINARY(16)".to_string(),
            ColumnType::Json | ColumnType::Jsonb => "JSON".to_string(),
            ColumnType::Binary => "LONGBLOB".to_string(),
            ColumnType::IntegerArray
            | ColumnType::BigIntegerArray
            | ColumnType::TextArray
            | ColumnType::BooleanArray
            | ColumnType::DoubleArray
            | ColumnType::JsonArray => "JSON".to_string(),
            ColumnType::Custom(sql) => sql.clone(),
        }
    }

    /// Convert to SQLite SQL type.
    ///
    /// SQLite stores type *affinities*, not types, so the vocabulary collapses
    /// hard here. Two collapses are deliberate and load-bearing:
    ///
    /// * Exact numerics render as `REAL`, matching `sea-query`'s own SQLite
    ///   renderer. `TEXT` would be the lossless target - SQLite has no exact
    ///   decimal type and `REAL` rounds money - but it is *unreadable*:
    ///   `sea-orm` decodes both `Decimal` and `BigDecimal` on SQLite through
    ///   `try_get::<Option<f64>>`, and `sqlx` only yields an `f64` from a
    ///   REAL-affinity column. A `TEXT` column fails every read with
    ///   `Rust type Option<f64> (as SQL type REAL) is not compatible with SQL
    ///   type TEXT`, so REAL is forced.
    ///
    ///   Known limitation: decimals therefore round to `f64` precision on
    ///   SQLite, and the 39-digit column `i128`/`u128` map to cannot hold its
    ///   full range. Code that needs exact decimal storage on SQLite has to
    ///   store the value itself - as a `String`, or as an integer number of
    ///   minor units - rather than relying on a decimal column.
    /// * Unsigned integers render as `INTEGER`, which is a *signed* 64-bit
    ///   value. That holds `u8`/`u16`/`u32` exactly; a `u64` above
    ///   `i64::MAX` is out of range and needs an explicit
    ///   [`ColumnType::Custom`] column if it has to be stored.
    pub fn to_sqlite_sql(&self) -> String {
        match self {
            ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::TinyUnsigned
            | ColumnType::SmallUnsigned
            | ColumnType::Unsigned
            | ColumnType::BigUnsigned
            | ColumnType::Boolean => "INTEGER".to_string(),
            ColumnType::Float
            | ColumnType::Double
            | ColumnType::Decimal { .. }
            | ColumnType::Numeric => "REAL".to_string(),
            ColumnType::String
            | ColumnType::Varchar(_)
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
            | ColumnType::BigIntegerArray
            | ColumnType::TextArray
            | ColumnType::BooleanArray
            | ColumnType::DoubleArray
            | ColumnType::JsonArray => "TEXT".to_string(),
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
    /// The value as a `DEFAULT` clause writes it on `db_type`. A string is
    /// escaped the way that backend reads a literal: MySQL and MariaDB also
    /// take a backslash as an escape, so there `C:\temp\` has its backslashes
    /// doubled.
    pub fn to_sql(&self, db_type: DatabaseType) -> String {
        match self {
            DefaultValue::String(value) => format!(
                "'{}'",
                crate::internal::sql_safety::escape_sql_literal_for_db(db_type, value)
            ),
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
