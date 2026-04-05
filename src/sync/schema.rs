use crate::database::Database;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident_for_backend;
use crate::internal::{
    Alias, Backend, ConnectionTrait, Expr, Index, MysqlQueryBuilder, OrmColumnDef, OrmColumnType,
    OrmConnection, PostgresQueryBuilder, SqliteQueryBuilder, Table, build_statement,
    build_statement_with_values,
};
use crate::{tide_debug, tide_info, tide_warn};

use super::SyncRegistry;

/// Column definition for TideORM schema synchronization
#[derive(Debug, Clone)]
pub struct ColumnDef {
    /// Column name
    pub name: String,
    /// Column type (Rust type string, converted at sync time)
    pub col_type: String,
    /// Whether the column allows NULL values
    pub nullable: bool,
    /// Whether this is the primary key
    pub primary_key: bool,
    /// Whether this column auto-increments
    pub auto_increment: bool,
    /// Default value expression (if any)
    pub default: Option<String>,
}

impl ColumnDef {
    /// Create a new column definition
    pub fn new(name: impl Into<String>, col_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            col_type: col_type.into(),
            nullable: true,
            primary_key: false,
            auto_increment: false,
            default: None,
        }
    }

    /// Set as primary key
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    /// Set as auto-increment
    pub fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }

    /// Set as not nullable
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// Set default value
    pub fn default(mut self, expr: impl Into<String>) -> Self {
        self.default = Some(expr.into());
        self
    }
}

/// Model schema definition for TideORM synchronization
#[derive(Debug, Clone)]
pub struct ModelSchema {
    /// Table name in the database
    pub table_name: String,
    /// Schema name (default: "public")
    pub schema_name: String,
    /// Column definitions
    pub columns: Vec<ColumnDef>,
    /// Primary key columns, in declaration order.
    pub primary_keys: Vec<String>,
}

impl ModelSchema {
    /// Create a new model schema
    pub fn new(table_name: impl Into<String>) -> Self {
        Self {
            table_name: table_name.into(),
            schema_name: "public".to_string(),
            columns: Vec::new(),
            primary_keys: Vec::new(),
        }
    }

    /// Set the schema name
    pub fn schema(mut self, schema: impl Into<String>) -> Self {
        self.schema_name = schema.into();
        self
    }

    /// Add a column definition
    pub fn column(mut self, col: ColumnDef) -> Self {
        self.columns.push(col);
        self
    }

    /// Add multiple columns
    pub fn columns(mut self, cols: Vec<ColumnDef>) -> Self {
        self.columns.extend(cols);
        self
    }

    /// Set the model primary keys.
    pub fn primary_keys(mut self, columns: Vec<String>) -> Self {
        self.primary_keys = columns;
        self
    }
}

pub(super) async fn sync_model_schemas(db: &Database, force_sync: bool) -> Result<()> {
    let models = SyncRegistry::get_all_schemas();
    let conn = db.__internal_connection()?;
    let backend = Backend::from(conn.get_database_backend());

    for model in models {
        let table_exists =
            check_table_exists(&conn, &model.schema_name, &model.table_name, backend).await?;

        if force_sync && table_exists {
            let quoted_table = quote_ident_for_backend(backend, &model.table_name);
            let drop_sql = match backend {
                Backend::Postgres => format!(
                    "DROP TABLE IF EXISTS {}.{} CASCADE",
                    quote_ident_for_backend(backend, &model.schema_name),
                    quoted_table
                ),
                _ => format!("DROP TABLE IF EXISTS {}", quoted_table),
            };

            let drop_stmt = build_statement(backend, drop_sql);
            conn.execute_raw(drop_stmt)
                .await
                .map_err(|error| Error::query(error.to_string()))?;

            tide_warn!("Dropped TideORM table: {}", model.table_name);
        }

        if !table_exists || force_sync {
            create_table_from_model_schema(&conn, &model, backend).await?;
            tide_info!("Created TideORM table: {}", model.table_name);
        } else {
            tide_debug!("TideORM table exists: {}", model.table_name);
        }
    }

    Ok(())
}

async fn check_table_exists(
    conn: &OrmConnection,
    schema: &str,
    table: &str,
    backend: Backend,
) -> Result<bool> {
    let statement = match backend {
        Backend::Postgres => build_statement_with_values(
            Backend::Postgres,
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_schema = $1 AND table_name = $2)",
            vec![schema.into(), table.into()],
        ),
        Backend::MySql => build_statement_with_values(
            Backend::MySql,
            "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = ?",
            vec![table.into()],
        ),
        Backend::Sqlite => build_statement_with_values(
            Backend::Sqlite,
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?",
            vec![table.into()],
        ),
    };

    let result = conn
        .query_one_raw(statement)
        .await
        .map_err(|error| Error::query(error.to_string()))?;

    match result {
        Some(row) => {
            let exists: bool = match backend {
                Backend::Postgres => row.try_get_by_index(0).unwrap_or(false),
                _ => {
                    let value: i32 = row.try_get_by_index(0).unwrap_or(0);
                    value > 0
                }
            };
            Ok(exists)
        }
        None => Ok(false),
    }
}

async fn create_table_from_model_schema(
    conn: &OrmConnection,
    model: &ModelSchema,
    backend: Backend,
) -> Result<()> {
    let mut table = Table::create();
    table.table(Alias::new(&model.table_name));
    let composite_primary_key = model.primary_keys.len() > 1;

    for col in &model.columns {
        let mut column = OrmColumnDef::new(Alias::new(&col.name));

        apply_column_type(&mut column, &col.col_type, col.auto_increment, backend);

        if col.primary_key && !composite_primary_key {
            column.primary_key();
        }

        if col.auto_increment {
            column.auto_increment();
        }

        if (composite_primary_key || !col.primary_key) && !col.auto_increment && !col.nullable {
            column.not_null();
        }

        if let Some(ref default) = col.default {
            let default_owned = default.clone();
            column.default(Expr::cust(default_owned));
        }

        table.col(&mut column);
    }

    if composite_primary_key {
        let mut primary_key = Index::create();
        for column in &model.primary_keys {
            primary_key.col(Alias::new(column));
        }
        table.primary_key(&mut primary_key);
    }

    table.if_not_exists();

    let sql = match backend {
        Backend::Postgres => table.to_string(PostgresQueryBuilder),
        Backend::MySql => table.to_string(MysqlQueryBuilder),
        Backend::Sqlite => table.to_string(SqliteQueryBuilder),
    };

    let create_stmt = build_statement(backend, sql);
    conn.execute_raw(create_stmt)
        .await
        .map_err(|error| Error::query(error.to_string()))?;

    Ok(())
}

fn apply_column_type(
    column: &mut OrmColumnDef,
    rust_type: &str,
    _auto_increment: bool,
    _backend: Backend,
) {
    let normalized = normalize_rust_type(rust_type);

    let inner_type = normalized
        .strip_prefix("Option<")
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(&normalized);
    let inner_type = canonical_schema_type(inner_type);

    match inner_type.as_str() {
        "i8" | "u8" | "i16" | "u16" => {
            column.small_integer();
        }
        "i32" => {
            column.integer();
        }
        "u32" | "i64" => {
            column.big_integer();
        }
        "u64" | "i128" | "u128" => {
            column.decimal();
        }
        "isize" | "usize" => {
            column.big_integer();
        }
        "f32" => {
            column.float();
        }
        "f64" => {
            column.double();
        }
        "bool" => {
            column.boolean();
        }
        "String" | "&str" => {
            column.text();
        }
        "Uuid" => {
            column.uuid();
        }
        "Json" | "JsonValue" | "serde_json::Value" | "Value" | "Jsonb" => {
            column.json_binary();
        }
        "Vec<u8>" | "Bytes" => {
            column.binary();
        }
        "Decimal" | "BigDecimal" => {
            column.decimal();
        }
        value if value.contains("DateTime") => {
            column.timestamp_with_time_zone();
        }
        value if value.contains("NaiveDateTime") => {
            column.timestamp();
        }
        value if value.contains("NaiveDate") => {
            column.date();
        }
        value if value.contains("NaiveTime") => {
            column.time();
        }
        "Vec<i32>" | "IntArray" => {
            column.array(OrmColumnType::Integer);
        }
        "Vec<i64>" | "BigIntArray" => {
            column.array(OrmColumnType::BigInteger);
        }
        "Vec<String>" | "TextArray" => {
            column.array(OrmColumnType::Text);
        }
        "Vec<bool>" | "BoolArray" => {
            column.array(OrmColumnType::Boolean);
        }
        "Vec<f64>" | "FloatArray" => {
            column.array(OrmColumnType::Double);
        }
        unknown_type => {
            tide_warn!(
                "Unknown Rust type '{}' mapped to TEXT column. Consider adding explicit type mapping.",
                unknown_type
            );
            column.text();
        }
    };
}

fn canonical_schema_type(rust_type: &str) -> String {
    let normalized = rust_type.trim();

    for alias in [
        "Json",
        "JsonValue",
        "JsonArray",
        "Jsonb",
        "IntArray",
        "BigIntArray",
        "TextArray",
        "BoolArray",
        "FloatArray",
        "Decimal",
        "Uuid",
        "NaiveDate",
        "NaiveTime",
        "NaiveDateTime",
        "Text",
    ] {
        if normalized == alias || normalized.ends_with(&format!("::{}", alias)) {
            return alias.to_string();
        }
    }

    normalized.to_string()
}

/// Normalizes a Rust type string by removing whitespace
pub fn normalize_rust_type(rust_type: &str) -> String {
    rust_type.chars().filter(|ch| !ch.is_whitespace()).collect()
}
