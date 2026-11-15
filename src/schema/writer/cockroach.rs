//! CockroachDB's schema, as its own `SHOW CREATE ALL ..` statements write it:
//! its emulated `pg_catalog` lacks what the PostgreSQL writer reads
//! (`pg_describe_object` among others).

use super::{Catalog, statements};
use crate::error::Result;
use crate::internal::OrmConnection;
use crate::internal::sql_lexer::{Kind, Token, significant};

/// Schemas and types first, then the tables, sequences and views (their
/// foreign keys last, as the server orders them) and which column owns each
/// sequence, then routines, which may read those tables, and triggers.
pub(super) async fn introspect(conn: &OrmConnection) -> Result<Catalog> {
    let read = |sql: &'static str| statements(conn, sql, Vec::new(), &["create_statement"]);
    let mut prelude = read("SHOW CREATE ALL SCHEMAS").await?;
    prelude.retain_mut(without_public_schema);
    prelude.extend(read("SHOW CREATE ALL TYPES").await?);
    let database = statements(
        conn,
        "SELECT current_database() AS name",
        Vec::new(),
        &["name"],
    )
    .await?;
    let mut verbatim: Vec<String> = read("SHOW CREATE ALL TABLES")
        .await?
        .iter()
        .map(|statement| unqualified(statement, database.first().map_or("", String::as_str)))
        .collect();
    verbatim.extend(statements(conn, OWNED_SEQUENCES, Vec::new(), &["statement"]).await?);
    verbatim.extend(read("SHOW CREATE ALL ROUTINES").await?);
    verbatim.extend(read("SHOW CREATE ALL TRIGGERS").await?);
    Ok(Catalog {
        prelude,
        verbatim,
        ..Catalog::default()
    })
}

/// `SHOW CREATE` leaves out `OWNED BY`, without which a restored sequence
/// outlives its table.
const OWNED_SEQUENCES: &str = "SELECT format('ALTER SEQUENCE %I.%I OWNED BY %I.%I.%I',
        sn.nspname, s.relname, tn.nspname, t.relname, a.attname) AS statement
    FROM pg_catalog.pg_depend d
    JOIN pg_catalog.pg_class s ON s.oid = d.objid AND s.relkind = 'S'
    JOIN pg_catalog.pg_namespace sn ON sn.oid = s.relnamespace
    JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
    JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
    JOIN pg_catalog.pg_attribute a ON a.attrelid = t.oid AND a.attnum = d.refobjsubid
    WHERE d.classid = 'pg_catalog.pg_class'::regclass
      AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype = 'a'
    ORDER BY 1";

/// Every database has `public` already, so its `CREATE SCHEMA` is dropped,
/// keeping a `COMMENT ON SCHEMA public` the server writes after it. Whether
/// anything is left.
fn without_public_schema(statement: &mut String) -> bool {
    if let Some(rest) = statement.strip_prefix("CREATE SCHEMA public")
        && let Some(rest) = rest.strip_prefix(';').or(rest.is_empty().then_some(rest))
    {
        *statement = rest.trim_start().to_owned();
    }
    !statement.is_empty()
}

/// `sql` without the database CockroachDB names in front of each table a
/// view reads (`app.public.users`), so the file restores into a database of
/// any name. Strings, such as a routine's body, are left alone.
fn unqualified(sql: &str, database: &str) -> String {
    let tokens: Vec<Token<'_>> = significant(sql).collect();
    let name = |at: usize| {
        tokens
            .get(at)
            .is_some_and(|token| matches!(token.kind, Kind::Word | Kind::Identifier))
    };
    let dot = |at: usize| tokens.get(at).is_some_and(|token| token.is_symbol(&["."]));
    let mut text = String::with_capacity(sql.len());
    let mut copied = 0;
    for (at, token) in tokens.iter().enumerate() {
        if name(at)
            && token.unquoted().as_deref() == Some(database)
            && !(at > 0 && dot(at - 1))
            && dot(at + 1)
            && name(at + 2)
            && dot(at + 3)
            && name(at + 4)
        {
            text.push_str(&sql[copied..token.start]);
            copied = tokens[at + 2].start;
        }
    }
    text.push_str(&sql[copied..]);
    text
}

#[cfg(test)]
#[path = "../../../tests/unit/schema_cockroach_tests.rs"]
mod tests;
