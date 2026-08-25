use crate::config::DatabaseType;
use crate::migration::ColumnType;
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

    /// Add a TIMESTAMPTZ column (timestamp with time zone) - use for `DateTime<Utc>`
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

/// Map a Rust type spelling onto TideORM's logical column type.
///
/// This is the **single** Rust-to-column mapping in the crate. Schema export
/// ([`rust_type_to_sql`]) and `DB_SYNC` (`crate::sync`) both go through it, and
/// the returned [`ColumnType`] renders to SQL through
/// [`ColumnType::to_sql`] - so the two cannot disagree about what a field
/// becomes, and migrations name the same vocabulary by hand.
///
/// The spelling is normalized first: whitespace, references and lifetimes are
/// dropped, module paths are stripped from the type and its generic arguments
/// (`chrono::DateTime<chrono::Utc>` becomes `DateTime<Utc>`), and any number of
/// `Option<..>` wrappers are peeled off - nullability is a column property, not
/// a type.
///
/// Returns `None` for a type the mapping does not know, leaving the fallback to
/// the caller: [`rust_type_to_sql`] falls back to `TEXT` silently, while `sync`
/// warns first.
///
/// ```
/// use tideorm::config::DatabaseType;
/// use tideorm::schema::rust_type_to_column_type;
///
/// let mapped = rust_type_to_column_type("Option<u32>").expect("u32 is mapped");
/// // PostgreSQL has no unsigned types. `INTEGER` is what the driver reads a
/// // `u32` back out of, so widening to `BIGINT` would make the column
/// // unreadable - see `ColumnType::to_postgres_sql`.
/// assert_eq!(mapped.to_sql(DatabaseType::Postgres), "INTEGER");
/// assert_eq!(mapped.to_sql(DatabaseType::MySQL), "INT UNSIGNED");
///
/// assert!(rust_type_to_column_type("MyCustomType").is_none());
/// ```
pub fn rust_type_to_column_type(rust_type: &str) -> Option<ColumnType> {
    let mapped = match lookup_key(rust_type).as_str() {
        "i8" | "i16" => ColumnType::SmallInteger,
        "i32" => ColumnType::Integer,
        "i64" | "isize" | "usize" => ColumnType::BigInteger,
        "u8" => ColumnType::TinyUnsigned,
        "u16" => ColumnType::SmallUnsigned,
        "u32" => ColumnType::Unsigned,
        "u64" => ColumnType::BigUnsigned,
        // 128-bit integers exceed every native column width; 39 digits is the
        // widest an i128/u128 can be. Only PostgreSQL and MySQL can hold that -
        // SQLite renders every decimal as REAL, so a 128-bit value rounds
        // there (see `ColumnType::to_sqlite_sql`).
        "i128" | "u128" => ColumnType::Decimal {
            precision: 39,
            scale: 0,
        },
        "f32" => ColumnType::Float,
        "f64" => ColumnType::Double,
        "bool" => ColumnType::Boolean,
        "String" | "str" | "Text" => ColumnType::Text,
        "Uuid" => ColumnType::Uuid,
        "Decimal" | "BigDecimal" => ColumnType::Numeric,
        "NaiveDate" | "Date" => ColumnType::Date,
        "NaiveTime" | "Time" => ColumnType::Time,
        // A naive timestamp carries no offset, so it must not land in a column
        // the server shifts by session timezone.
        "NaiveDateTime" => ColumnType::DateTime,
        "Json" | "JsonValue" | "Jsonb" | "Value" => ColumnType::Jsonb,
        "Vec<u8>" | "Bytes" => ColumnType::Binary,
        "Vec<i32>" | "IntArray" => ColumnType::IntegerArray,
        "Vec<i64>" | "BigIntArray" => ColumnType::BigIntegerArray,
        "Vec<String>" | "Vec<str>" | "TextArray" => ColumnType::TextArray,
        "Vec<bool>" | "BoolArray" => ColumnType::BooleanArray,
        "Vec<f64>" | "FloatArray" => ColumnType::DoubleArray,
        "Vec<Value>" | "Vec<JsonValue>" | "JsonArray" => ColumnType::JsonArray,
        // Everything else spelled `DateTime<..>` is offset-aware: `DateTime<Utc>`,
        // `DateTime<FixedOffset>`, and the engine's `DateTimeUtc` aliases. This
        // arm is last so the exact `NaiveDateTime` key above wins.
        other if other.contains("DateTime") => ColumnType::TimestampTz,
        _ => return None,
    };

    Some(mapped)
}

/// Utility to map Rust types to SQL types.
///
/// Thin sugar over [`rust_type_to_column_type`] plus [`ColumnType::to_sql`];
/// unknown types fall back to `TEXT`. Because `sync` and the migration builders
/// render through the same pair, the SQL a model exports here is the SQL
/// `DB_SYNC` creates.
pub fn rust_type_to_sql(rust_type: &str, db_type: DatabaseType) -> String {
    rust_type_to_column_type(rust_type)
        .unwrap_or(ColumnType::Text)
        .to_sql(db_type)
}

/// Reduce a Rust type spelling to the key the mapping table is written in.
///
/// Strips references, lifetimes and whitespace, then module paths, then any
/// number of `Option<..>` wrappers.
fn lookup_key(rust_type: &str) -> String {
    let mut cleaned = String::with_capacity(rust_type.len());
    let mut chars = rust_type.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '&' => {}
            whitespace if whitespace.is_whitespace() => {}
            // A lifetime (`'a`, `'_`, `'static`) and its name carry no type
            // information. Whitespace is dropped in this same pass, so the
            // lifetime has to be consumed here or `&'static str` would fuse
            // into `staticstr`.
            '\'' => {
                while chars
                    .peek()
                    .is_some_and(|next| next.is_alphanumeric() || *next == '_')
                {
                    chars.next();
                }
            }
            other => cleaned.push(other),
        }
    }

    let mut key = strip_module_paths(&cleaned);
    // Nullability is a column property; `Option<Option<T>>` is still a `T` column.
    while let Some(peeled) = key
        .strip_prefix("Option<")
        .and_then(|inner| inner.strip_suffix('>'))
        .map(str::to_string)
    {
        key = peeled;
    }

    key
}

/// Drop module paths from a whitespace-free type and its generic arguments.
fn strip_module_paths(rust_type: &str) -> String {
    match rust_type.find('<') {
        Some(open) if rust_type.ends_with('>') => {
            let head = last_path_segment(&rust_type[..open]);
            let arguments = &rust_type[open + 1..rust_type.len() - 1];
            let stripped: Vec<String> = split_generic_arguments(arguments)
                .into_iter()
                .map(strip_module_paths)
                .collect();

            format!("{}<{}>", head, stripped.join(","))
        }
        _ => last_path_segment(rust_type).to_string(),
    }
}

fn last_path_segment(rust_type: &str) -> &str {
    rust_type.rsplit("::").next().unwrap_or(rust_type)
}

/// Split `A,B<C,D>,E` on its top-level commas only.
fn split_generic_arguments(arguments: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, character) in arguments.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&arguments[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&arguments[start..]);

    parts
}
