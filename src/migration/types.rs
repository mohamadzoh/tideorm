use crate::config::DatabaseType;
use crate::internal::sql_safety::quote_ident;

/// The type of a column: the one vocabulary migrations name, schema sync
/// takes from a field's Rust type (`<T as ValueType>::column_type()`), and a
/// model's `Column::def()` reports. [`column_type_sql`] is the only place one
/// becomes SQL text, so a table a migration creates matches the table sync
/// would have created.
pub use crate::orm::ColumnType;
/// The length of a [`ColumnType::String`] or [`ColumnType::VarBinary`].
pub use crate::orm::sql_builder::StringLen;

/// `column_type` as SQL for `db_type`. MySQL and MariaDB share one rendering.
///
/// PostgreSQL has no unsigned integers, so each unsigned type lands on the
/// narrowest signed type that holds all its values (`u8` on `SMALLINT`, `u16`
/// on `INTEGER`, `u32` on `BIGINT`); a `u64` lands on `BIGINT` too, so one past
/// `i64::MAX` is refused when it is bound. MySQL has native unsigned integers
/// and no arrays, which it stores as `JSON`, and three renderings avoid its
/// defaults that silently lose data: text and binary are `LONGTEXT`/`LONGBLOB`
/// (plain `TEXT` stops at 64 KB), times carry six fractional digits, and every
/// timestamp is `DATETIME(6)` (a `TIMESTAMP` spans only 1970 to 2038; the driver
/// pins each session to `+00:00`, so a `DateTime<Utc>` reads back as UTC). A
/// `Uuid` is `BINARY(16)` there, the bytes its driver sends. SQLite stores
/// affinities: integers and booleans are `INTEGER`, exact numerics `REAL` (so
/// they compare as numbers, rounding past about 15 digits), binary `BLOB`, and
/// everything else `TEXT`. A [`ColumnType::Custom`] type is written as given.
pub fn column_type_sql(column_type: &ColumnType, db_type: DatabaseType) -> String {
    match db_type {
        DatabaseType::Postgres => postgres_sql(column_type),
        DatabaseType::MySQL | DatabaseType::MariaDB => mysql_sql(column_type),
        DatabaseType::SQLite => sqlite_sql(column_type),
    }
}

/// Whether a column of this type can carry an auto-increment clause. A
/// [`ColumnType::Custom`] type is taken at its word.
pub(crate) fn can_auto_increment(column_type: &ColumnType) -> bool {
    matches!(column_type, ColumnType::Custom(_)) || crate::internal::is_integer(column_type)
}

fn sized(name: &str, size: Option<u32>) -> String {
    match size {
        Some(size) => format!("{name}({size})"),
        None => name.to_string(),
    }
}

fn decimal(precision: Option<(u32, u32)>, unsized_sql: &str) -> String {
    match precision {
        Some((precision, scale)) => format!("DECIMAL({precision}, {scale})"),
        None => unsized_sql.to_string(),
    }
}

fn postgres_sql(column_type: &ColumnType) -> String {
    match column_type {
        ColumnType::Char(length) => sized("CHAR", *length),
        ColumnType::String(StringLen::N(length)) => format!("VARCHAR({length})"),
        ColumnType::String(StringLen::None) => "VARCHAR(255)".to_string(),
        ColumnType::String(StringLen::Max) | ColumnType::Text => "TEXT".to_string(),
        ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_) => "BYTEA".to_string(),
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::TinyUnsigned
        | ColumnType::Year => "SMALLINT".to_string(),
        ColumnType::Integer | ColumnType::SmallUnsigned => "INTEGER".to_string(),
        ColumnType::BigInteger | ColumnType::Unsigned | ColumnType::BigUnsigned => {
            "BIGINT".to_string()
        }
        ColumnType::Float => "REAL".to_string(),
        ColumnType::Double => "DOUBLE PRECISION".to_string(),
        ColumnType::Decimal(precision) => decimal(*precision, "DECIMAL"),
        ColumnType::Money(_) => "MONEY".to_string(),
        ColumnType::DateTime | ColumnType::Timestamp => "TIMESTAMP".to_string(),
        ColumnType::TimestampWithTimeZone => "TIMESTAMPTZ".to_string(),
        ColumnType::Time => "TIME".to_string(),
        ColumnType::Date => "DATE".to_string(),
        ColumnType::Interval(fields, precision) => {
            let interval = match fields {
                Some(fields) => format!("INTERVAL {fields}"),
                None => "INTERVAL".to_string(),
            };
            sized(&interval, *precision)
        }
        ColumnType::Bit(length) => sized("BIT", *length),
        ColumnType::VarBit(length) => format!("VARBIT({length})"),
        ColumnType::Boolean => "BOOLEAN".to_string(),
        ColumnType::Json => "JSON".to_string(),
        ColumnType::JsonBinary => "JSONB".to_string(),
        ColumnType::Uuid => "UUID".to_string(),
        ColumnType::Custom(sql) => sql.to_string(),
        // The type itself is the application's to create (`CREATE TYPE .. AS ENUM`).
        ColumnType::Enum { name, .. } => quote_ident(DatabaseType::Postgres, &name.to_string()),
        ColumnType::Array(element) => format!("{}[]", postgres_sql(element)),
        ColumnType::Vector(dimensions) => sized("vector", *dimensions),
        ColumnType::Cidr => "CIDR".to_string(),
        ColumnType::Inet => "INET".to_string(),
        ColumnType::MacAddr => "MACADDR".to_string(),
    }
}

fn mysql_sql(column_type: &ColumnType) -> String {
    match column_type {
        ColumnType::Char(length) => sized("CHAR", *length),
        ColumnType::String(StringLen::N(length)) => format!("VARCHAR({length})"),
        ColumnType::String(StringLen::None) => "VARCHAR(255)".to_string(),
        ColumnType::String(StringLen::Max) | ColumnType::Text => "LONGTEXT".to_string(),
        ColumnType::Blob | ColumnType::VarBinary(StringLen::None | StringLen::Max) => {
            "LONGBLOB".to_string()
        }
        ColumnType::Binary(length) => format!("BINARY({length})"),
        ColumnType::VarBinary(StringLen::N(length)) => format!("VARBINARY({length})"),
        ColumnType::TinyInteger => "TINYINT".to_string(),
        ColumnType::SmallInteger => "SMALLINT".to_string(),
        ColumnType::Integer => "INT".to_string(),
        ColumnType::BigInteger => "BIGINT".to_string(),
        ColumnType::TinyUnsigned => "TINYINT UNSIGNED".to_string(),
        ColumnType::SmallUnsigned => "SMALLINT UNSIGNED".to_string(),
        ColumnType::Unsigned => "INT UNSIGNED".to_string(),
        ColumnType::BigUnsigned => "BIGINT UNSIGNED".to_string(),
        ColumnType::Float => "FLOAT".to_string(),
        ColumnType::Double => "DOUBLE".to_string(),
        ColumnType::Decimal(precision) => decimal(*precision, "DECIMAL(65,30)"),
        ColumnType::Money(precision) => decimal(*precision, "DECIMAL(19, 4)"),
        ColumnType::DateTime | ColumnType::Timestamp | ColumnType::TimestampWithTimeZone => {
            "DATETIME(6)".to_string()
        }
        ColumnType::Time => "TIME(6)".to_string(),
        ColumnType::Date => "DATE".to_string(),
        ColumnType::Year => "YEAR".to_string(),
        // PostgreSQL types MySQL lacks, kept as their text.
        ColumnType::Interval(..) | ColumnType::Cidr | ColumnType::Inet | ColumnType::MacAddr => {
            "VARCHAR(255)".to_string()
        }
        ColumnType::Bit(length) => sized("BIT", *length),
        ColumnType::VarBit(length) => format!("BIT({length})"),
        ColumnType::Boolean => "TINYINT(1)".to_string(),
        ColumnType::Json
        | ColumnType::JsonBinary
        | ColumnType::Array(_)
        | ColumnType::Vector(_) => "JSON".to_string(),
        ColumnType::Uuid => "BINARY(16)".to_string(),
        ColumnType::Custom(sql) => sql.to_string(),
        ColumnType::Enum { variants, .. } => {
            let variants: Vec<String> = variants
                .iter()
                .map(|variant| {
                    DatabaseType::MySQL
                        .dialect()
                        .string_literal(&variant.to_string())
                })
                .collect();
            format!("ENUM({})", variants.join(", "))
        }
    }
}

fn sqlite_sql(column_type: &ColumnType) -> String {
    match column_type {
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::BigInteger
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned
        | ColumnType::BigUnsigned
        | ColumnType::Boolean
        | ColumnType::Year
        | ColumnType::Bit(_)
        | ColumnType::VarBit(_) => "INTEGER".to_string(),
        ColumnType::Float | ColumnType::Double | ColumnType::Decimal(_) | ColumnType::Money(_) => {
            "REAL".to_string()
        }
        ColumnType::Blob | ColumnType::Binary(_) | ColumnType::VarBinary(_) => "BLOB".to_string(),
        ColumnType::Custom(sql) => sql.to_string(),
        _ => "TEXT".to_string(),
    }
}

/// Default value for columns
#[derive(Debug, Clone, PartialEq)]
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
    /// A UUID, written as the column holds it: its text on PostgreSQL, its
    /// 16 bytes on MySQL and MariaDB (`BINARY(16)`) and on SQLite, where the
    /// driver binds a UUID as a 16-byte blob, which text would never equal.
    Uuid(uuid::Uuid),
    /// NULL
    Null,
}

impl DefaultValue {
    /// The value as a `DEFAULT` clause writes it on `db_type`. A string is
    /// the backend's literal: MySQL and MariaDB take a backslash as an
    /// escape, so there `C:\temp\` has its backslashes doubled, and so does
    /// PostgreSQL's `E'..'` form, which it takes for a string that holds one.
    pub fn to_sql(&self, db_type: DatabaseType) -> String {
        match self {
            DefaultValue::String(value) => db_type.dialect().string_literal(value),
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
            DefaultValue::Uuid(value) => match db_type {
                DatabaseType::Postgres => db_type.dialect().string_literal(&value.to_string()),
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => {
                    format!("X'{}'", value.simple())
                }
            },
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

/// Integers of every width a column holds whole.
macro_rules! integer_defaults {
    ($($integer:ty),*) => {
        $(
            impl From<$integer> for DefaultValue {
                fn from(value: $integer) -> Self {
                    DefaultValue::Integer(i64::from(value))
                }
            }
        )*
    };
}

integer_defaults!(i8, i16, i32, u8, u16, u32);

impl From<f32> for DefaultValue {
    fn from(value: f32) -> Self {
        DefaultValue::Float(f64::from(value))
    }
}

impl From<rust_decimal::Decimal> for DefaultValue {
    /// Written as its exact digits.
    fn from(value: rust_decimal::Decimal) -> Self {
        DefaultValue::Raw(value.to_string())
    }
}

impl From<uuid::Uuid> for DefaultValue {
    fn from(value: uuid::Uuid) -> Self {
        DefaultValue::Uuid(value)
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
