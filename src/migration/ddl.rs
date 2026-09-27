//! The DDL every schema path renders.
//!
//! Migrations, schema export and schema sync all build their statements here,
//! so a column declared the same way comes out as the same SQL whichever of the
//! three created the table.

use crate::config::DatabaseType;
use crate::internal::sql_safety::quote_ident;

use super::ColumnType;

/// One column of a `CREATE TABLE` or an `ALTER TABLE ... ADD COLUMN`.
#[derive(Debug, Clone)]
pub(crate) struct ColumnDefinition {
    pub(crate) name: String,
    pub(crate) column_type: ColumnType,
    pub(crate) nullable: bool,
    /// Already-rendered SQL for the `DEFAULT` clause.
    pub(crate) default: Option<String>,
    /// Whether the column belongs to the primary key.
    ///
    /// The key itself is always rendered as a table constraint; this only
    /// drops the `NOT NULL` and `UNIQUE` the key already implies.
    pub(crate) primary_key: bool,
    pub(crate) auto_increment: bool,
    pub(crate) unique: bool,
    pub(crate) check: Option<String>,
    pub(crate) extra: Option<String>,
}

impl ColumnDefinition {
    pub(crate) fn new(name: impl Into<String>, column_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            column_type,
            nullable: true,
            default: None,
            primary_key: false,
            auto_increment: false,
            unique: false,
            check: None,
            extra: None,
        }
    }

    pub(crate) fn to_sql(&self, db_type: DatabaseType) -> String {
        let mut sql = format!(
            "{} {}",
            quote_ident(db_type, &self.name),
            self.type_sql(db_type)
        );

        // SQLite needs no clause: an INTEGER primary key is already the rowid.
        if self.auto_increment
            && matches!(db_type, DatabaseType::MySQL | DatabaseType::MariaDB)
            && self.column_type.can_auto_increment()
        {
            sql.push_str(" AUTO_INCREMENT");
        }

        if !self.nullable && !self.primary_key {
            sql.push_str(" NOT NULL");
        }

        if let Some(default) = &self.default {
            sql.push_str(&format!(" DEFAULT {}", self.default_sql(db_type, default)));
        }

        if self.unique && !self.primary_key {
            sql.push_str(" UNIQUE");
        }

        if let Some(check) = &self.check {
            sql.push_str(&format!(" CHECK ({})", check));
        }

        if let Some(extra) = &self.extra {
            sql.push_str(&format!(" {}", extra));
        }

        sql
    }

    /// `default` as `db_type` accepts it for this column. A current-time
    /// default in any common spelling (`now()`, `CURRENT_TIMESTAMP()`, ..) is
    /// written `CURRENT_TIMESTAMP` on SQLite, the only one it reads, and on
    /// MySQL raised to the six fractional digits of a `DATETIME(6)`, because
    /// MySQL rejects a default less precise than its column. On MySQL a
    /// `TEXT`, `BLOB` or `JSON` default (an array column is `JSON` there) is
    /// written as an expression, the only form MySQL allows on those types.
    fn default_sql(&self, db_type: DatabaseType, default: &str) -> String {
        match db_type {
            DatabaseType::Postgres => return default.to_string(),
            DatabaseType::SQLite => {
                return if is_current_timestamp(default) {
                    "CURRENT_TIMESTAMP".to_string()
                } else {
                    default.to_string()
                };
            }
            DatabaseType::MySQL | DatabaseType::MariaDB => {}
        }
        let accepted_as_is = default.eq_ignore_ascii_case("NULL")
            || (default.starts_with('(') && default.ends_with(')'));
        match self.column_type {
            ColumnType::DateTime | ColumnType::Timestamp | ColumnType::TimestampTz
                if is_current_timestamp(default) =>
            {
                "CURRENT_TIMESTAMP(6)".to_string()
            }
            ColumnType::Text
            | ColumnType::Json
            | ColumnType::Jsonb
            | ColumnType::Binary
            | ColumnType::IntegerArray
            | ColumnType::BigIntegerArray
            | ColumnType::TextArray
            | ColumnType::BooleanArray
            | ColumnType::DoubleArray
            | ColumnType::JsonArray
                if !accepted_as_is =>
            {
                format!("({default})")
            }
            _ => default.to_string(),
        }
    }

    /// The column type, swapped on PostgreSQL for the serial pseudo-type of
    /// the same width when the column auto-increments.
    fn type_sql(&self, db_type: DatabaseType) -> String {
        let sql = self.column_type.to_sql(db_type);

        if self.auto_increment
            && db_type == DatabaseType::Postgres
            && let Some(serial) = postgres_serial_type(&sql)
        {
            return serial.to_string();
        }

        sql
    }
}

/// The longest identifier every backend keeps whole: PostgreSQL cuts names at
/// 63 bytes, MySQL rejects them past 64 characters.
const MAX_IDENTIFIER_BYTES: usize = 63;

/// A generated index name that fits [`MAX_IDENTIFIER_BYTES`].
///
/// A longer one keeps its first 54 bytes and ends in `_` and eight hex digits
/// of its FNV-1a hash, so two long names that share a prefix stay distinct
/// (PostgreSQL would cut both to the same 63 bytes and skip the second index)
/// and MySQL accepts it. The model derive shortens `#[index]` names the same
/// way, so both spell an index alike.
pub(crate) fn bounded_index_name(name: String) -> String {
    if name.len() <= MAX_IDENTIFIER_BYTES {
        return name;
    }
    let hash = name.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    let mut cut = MAX_IDENTIFIER_BYTES - 9;
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}_{:08x}", &name[..cut], hash)
}

/// Whether `default` is the current timestamp in one of the spellings the
/// backends share between them.
fn is_current_timestamp(default: &str) -> bool {
    matches!(
        default.trim().to_ascii_uppercase().as_str(),
        "CURRENT_TIMESTAMP"
            | "CURRENT_TIMESTAMP()"
            | "NOW()"
            | "LOCALTIMESTAMP"
            | "LOCALTIMESTAMP()"
    )
}

/// Whether SQLite can add a column with `default` to a table that has rows:
/// `ALTER TABLE .. ADD COLUMN` takes no current-time default and no
/// parenthesized expression.
pub(crate) fn sqlite_can_add_with_default(default: &str) -> bool {
    let default = default.trim();
    !(default.starts_with('(')
        || is_current_timestamp(default)
        || default.eq_ignore_ascii_case("CURRENT_DATE")
        || default.eq_ignore_ascii_case("CURRENT_TIME"))
}

/// Map a PostgreSQL integer type to the serial pseudo-type of the same width.
///
/// The width is preserved rather than widened to `BIGSERIAL`: an `int4` key
/// behind an `i32` or `u32` field has to stay `int4`, or the driver cannot
/// decode it. Types with no serial form are left alone.
fn postgres_serial_type(sql_type: &str) -> Option<&'static str> {
    match sql_type.trim().to_uppercase().as_str() {
        "SMALLINT" | "INT2" => Some("SMALLSERIAL"),
        "INTEGER" | "INT" | "INT4" => Some("SERIAL"),
        "BIGINT" | "INT8" => Some("BIGSERIAL"),
        _ => None,
    }
}

/// Quote `columns` and join them for a key or index column list.
pub(crate) fn column_list(db_type: DatabaseType, columns: &[impl AsRef<str>]) -> String {
    columns
        .iter()
        .map(|column| quote_ident(db_type, column.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render `CREATE TABLE`, one column or table constraint per line.
///
/// `table` arrives quoted, because the callers disagree on what a dotted name
/// means: migrations quote it as one identifier, while schema export and sync
/// qualify it with a schema.
///
/// A MySQL or MariaDB table is declared `utf8mb4`: it otherwise inherits the
/// database's character set, which is latin1 by default before MariaDB 11.6
/// and MySQL 8.0, and a latin1 column rejects most of what a Rust `String` can
/// hold.
pub(crate) fn create_table(
    db_type: DatabaseType,
    table: &str,
    if_not_exists: bool,
    columns: &[ColumnDefinition],
    primary_key: &[String],
    constraints: &[String],
) -> String {
    let mut lines: Vec<String> = columns
        .iter()
        .map(|column| column.to_sql(db_type))
        .collect();

    if !primary_key.is_empty() {
        lines.push(format!(
            "PRIMARY KEY ({})",
            column_list(db_type, primary_key)
        ));
    }

    lines.extend_from_slice(constraints);

    let exists_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
    let table_options = match db_type {
        DatabaseType::MySQL | DatabaseType::MariaDB => " DEFAULT CHARSET=utf8mb4",
        DatabaseType::Postgres | DatabaseType::SQLite => "",
    };
    format!(
        "CREATE TABLE {}{} (\n    {}\n){}",
        exists_clause,
        table,
        lines.join(",\n    "),
        table_options
    )
}

/// Render `CREATE [UNIQUE] INDEX` on the already-quoted `table`.
///
/// `IF NOT EXISTS` is dropped on MySQL, which rejects it outright - aborting a
/// `create_table_if_not_exists` migration after its table was created. MariaDB
/// accepts it, so the two are told apart here even though `internal::Backend`
/// collapses them into one variant.
pub(crate) fn create_index(
    db_type: DatabaseType,
    name: &str,
    table: &str,
    columns: &[impl AsRef<str>],
    unique: bool,
    if_not_exists: bool,
) -> String {
    let index_type = if unique { "UNIQUE INDEX" } else { "INDEX" };
    let exists_clause = if if_not_exists && db_type != DatabaseType::MySQL {
        "IF NOT EXISTS "
    } else {
        ""
    };

    format!(
        "CREATE {} {}{} ON {} ({})",
        index_type,
        exists_clause,
        quote_ident(db_type, name),
        table,
        column_list(db_type, columns)
    )
}

/// Render `ALTER TABLE ... ADD COLUMN` on the already-quoted `table`.
pub(crate) fn add_column(db_type: DatabaseType, table: &str, column: &ColumnDefinition) -> String {
    format!(
        "ALTER TABLE {} ADD COLUMN {}",
        table,
        column.to_sql(db_type)
    )
}

/// Render the statement that renames table `from` to `to`.
///
/// MySQL and MariaDB spell it `RENAME TABLE`; PostgreSQL and SQLite take
/// `ALTER TABLE ... RENAME TO`.
pub(crate) fn rename_table(db_type: DatabaseType, from: &str, to: &str) -> String {
    let (from, to) = (quote_ident(db_type, from), quote_ident(db_type, to));
    match db_type {
        DatabaseType::MySQL | DatabaseType::MariaDB => format!("RENAME TABLE {from} TO {to}"),
        DatabaseType::Postgres | DatabaseType::SQLite => {
            format!("ALTER TABLE {from} RENAME TO {to}")
        }
    }
}
