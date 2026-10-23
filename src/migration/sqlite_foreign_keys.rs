//! SQLite migrations that rebuild a table with foreign key enforcement off.
//!
//! SQLite changes enforcement only outside a transaction, and a migration runs
//! in one, where `PRAGMA foreign_keys = OFF` does nothing: a rebuild's
//! `DROP TABLE` would delete the rows pointing at the table. A migration
//! asking for it runs on a connection of its own opened with enforcement off.

#[cfg(feature = "sqlite")]
use super::migrator::sqlx_error;
#[cfg(feature = "sqlite")]
use crate::database::Database;
#[cfg(feature = "sqlite")]
use crate::error::{Error, Result};

/// Run `work` in a transaction on a connection of `db`'s database with
/// foreign key enforcement off, and commit it once no foreign key points
/// nowhere, as SQLite's own procedure for rebuilding a table checks.
#[cfg(feature = "sqlite")]
pub(super) async fn run_with_foreign_keys_off(
    db: &Database,
    work: impl std::future::Future<Output = Result<()>> + Send + 'static,
) -> Result<()> {
    let migrating = sqlite_foreign_keys_off(db).await?;
    let result = migrating
        .transaction(move |_| {
            Box::pin(async move {
                // The check after the migration cannot tell the rows it
                // leaves from rows that already pointed nowhere.
                if let Some(violations) = foreign_key_violations().await? {
                    return Err(Error::query(format!(
                        "the database already has {violations}; fix them before running a \
                         migration with foreign keys off"
                    )));
                }
                work.await?;
                match foreign_key_violations().await? {
                    Some(violations) => Err(Error::query(format!(
                        "the migration ran with foreign keys off and leaves {violations}; it \
                         was rolled back"
                    ))),
                    None => Ok(()),
                }
            })
        })
        .await;
    let _ = migrating.__internal_connection()?.close_by_ref().await;
    result
}

/// A handle on `db`'s SQLite database through a connection of its own with
/// foreign key enforcement off, under `db`'s identity, so a statement through
/// `db` joins a transaction opened on it.
#[cfg(feature = "sqlite")]
async fn sqlite_foreign_keys_off(db: &Database) -> Result<Database> {
    if matches!(
        db.__get_connection()?,
        crate::database::ConnectionRef::Transaction(..)
    ) {
        return Err(Error::query(
            "a migration with foreign keys off cannot run inside a transaction: SQLite changes \
             foreign key enforcement only outside one",
        ));
    }
    let pool = db.current_inner()?;
    let sqlite = pool.connection().get_sqlite_connection_pool();
    let options = (*sqlite.connect_options()).clone().foreign_keys(false);
    let connection = crate::internal::sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| crate::internal::translate_connection_error(sqlx_error(error)))?;
    let migrating = Database::sharing_identity_of(&pool, connection.into());

    // An in-memory database with a private cache is one per connection, so
    // the new one would be empty, where the ledger already is in `db`'s.
    let objects = |db: Database| async move {
        db.__raw_json_with_params("SELECT count(*) AS n FROM sqlite_master", Vec::new())
            .await
            .map(|rows| rows.first().and_then(|row| row["n"].as_i64()))
    };
    if objects(db.clone()).await? != objects(migrating.clone()).await? {
        let _ = migrating.__internal_connection()?.close_by_ref().await;
        return Err(Error::query(
            "a migration with foreign keys off needs a connection of its own to the database, \
             which an in-memory SQLite database with a private cache cannot give",
        ));
    }
    Ok(migrating)
}

/// The rows whose foreign key matches no row, described, which SQLite's own
/// procedure for rebuilding a table checks for before it commits.
#[cfg(feature = "sqlite")]
async fn foreign_key_violations() -> Result<Option<String>> {
    let violations = crate::database::__current_db()?
        .__raw_json_with_params("PRAGMA foreign_key_check", Vec::new())
        .await?;
    Ok(violations.first().map(|first| {
        format!(
            "{} row(s) whose foreign key matches no row, the first in table {} (rowid {}) \
             pointing at table {}",
            violations.len(),
            first["table"].as_str().unwrap_or_default(),
            first["rowid"],
            first["parent"].as_str().unwrap_or_default()
        )
    }))
}

/// Whether any statement requests disabling SQLite's foreign keys. Recognize
/// both pragma assignment syntaxes, quoted names, schema qualifiers and comments.
/// Inspect every statement before execution so a script cannot partially run
/// before its ineffective request to disable enforcement is rejected.
pub fn disables_foreign_keys(sql: &str) -> bool {
    use crate::internal::sql_lexer::sqlite_significant;

    let tokens: Vec<_> = sqlite_significant(sql).collect();
    tokens.split(|token| token.text == ";").any(|statement| {
        let Some((keyword, mut rest)) = statement.split_first() else {
            return false;
        };
        if !keyword.is_word(&["pragma"]) {
            return false;
        }
        if rest.get(1).is_some_and(|token| token.is_symbol(&["."])) {
            rest = &rest[2..];
        }
        let Some((name, args)) = rest.split_first() else {
            return false;
        };
        if !name
            .unquoted()
            .is_some_and(|name| name.eq_ignore_ascii_case("foreign_keys"))
            || !args
                .first()
                .is_some_and(|token| token.is_symbol(&["=", "("]))
        {
            return false;
        }
        // SQLite removes a numeric unary plus before interpreting the value;
        // a plus inside a quoted string is preserved.
        let args = if args.get(1).is_some_and(|token| token.is_symbol(&["+"])) {
            &args[2..]
        } else {
            &args[1..]
        };
        let value: Vec<_> = args
            .iter()
            .take_while(|token| !token.is_symbol(&[")"]))
            .collect();
        let value = match value.as_slice() {
            [token] => token.unquoted().unwrap_or_default(),
            tokens => tokens.iter().map(|token| token.text).collect(),
        };
        !sqlite_pragma_enabled(&value)
    })
}

/// SQLite's boolean pragma conversion uses a signed 32-bit integer prefix,
/// then its low byte, or one of the affirmative names. Thus 256 and integer
/// overflow disable enforcement, while 257, 1.0, and 0x1 enable it.
fn sqlite_pragma_enabled(value: &str) -> bool {
    if value.starts_with(|c: char| c.is_ascii_digit()) {
        let (digits, radix) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .filter(|hex| hex.starts_with(|c: char| c.is_ascii_hexdigit()))
            .map_or((value, 10), |hex| (hex, 16));
        let end = digits
            .find(|c: char| !c.is_digit(radix))
            .unwrap_or(digits.len());
        return i32::from_str_radix(&digits[..end], radix).is_ok_and(|number| number as u8 != 0);
    }
    ["on", "true", "yes"]
        .iter()
        .any(|on| value.eq_ignore_ascii_case(on))
}
