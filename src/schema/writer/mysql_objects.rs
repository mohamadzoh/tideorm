//! MySQL/MariaDB schema objects retain server-authored definitions and the
//! SQL modes under which stored programs were parsed.
use super::*;

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}
fn script(ddl: &str) -> String {
    let mut delimiter = "__tideorm_end__".to_owned();
    while ddl.contains(&delimiter) {
        delimiter.push('_');
    }
    format!(
        "DELIMITER {delimiter}\n{}{delimiter}\nDELIMITER ;",
        ddl.trim_end().trim_end_matches(';')
    )
}

pub(super) async fn introspect(conn: &OrmConnection) -> Result<Catalog> {
    let mut catalog = Catalog::default();
    let database_rows = query(
        conn,
        Backend::MySql,
        "SELECT DATABASE() AS name, CAST(@@lower_case_table_names AS SIGNED) AS fold_names, default_collation_name AS database_collation
         FROM information_schema.schemata
         WHERE schema_name=DATABASE()",
        Vec::new(),
    )
    .await?;
    let database: String = get(
        database_rows
            .first()
            .ok_or_else(|| Error::internal("database name missing"))?,
        "name",
    )?;
    let fold_names = get::<i64>(&database_rows[0], "fold_names")? != 0;
    let database_collation = format!(
        "ALTER DATABASE COLLATE {}",
        quote_ident(
            DatabaseType::MySQL,
            &get::<String>(&database_rows[0], "database_collation")?
        )
    );
    // SHOW CREATE TABLE escapes literals with backslashes, even when the source
    // session uses NO_BACKSLASH_ESCAPES. Restore in its canonical dialect, then
    // restore the caller's settings at the end of the script.
    catalog.prelude.extend([
        "SET @tideorm_saved_sql_mode = @@SESSION.sql_mode".into(),
        "SET @tideorm_saved_client = @@SESSION.character_set_client".into(),
        "SET @tideorm_saved_results = @@SESSION.character_set_results".into(),
        "SET @tideorm_saved_collation = @@SESSION.collation_connection".into(),
        "SET @tideorm_saved_time_zone = @@SESSION.time_zone".into(),
        "SET NAMES utf8mb4".into(),
        "SET SESSION sql_mode = 'ANSI_QUOTES'".into(),
    ]);
    catalog.prelude.push(database_collation.clone());
    let rows = query(
        conn,
        Backend::MySql,
        "SELECT table_name AS name,table_type AS kind
         FROM information_schema.tables
         WHERE table_schema=DATABASE()
         ORDER BY table_name",
        Vec::new(),
    )
    .await?;
    let mut views = Vec::new();
    for row in rows {
        let name: String = get(&row, "name")?;
        let kind: String = get(&row, "kind")?;
        if kind == "VIEW" {
            views.push(name);
            continue;
        }
        // MariaDB sequences have their own DDL, not a normal CREATE TABLE.
        let command = if kind == "SEQUENCE" {
            "SEQUENCE"
        } else {
            "TABLE"
        };
        let rows = query(
            conn,
            Backend::MySql,
            &format!(
                "SHOW CREATE {command} {}",
                quote_ident(DatabaseType::MySQL, &name)
            ),
            Vec::new(),
        )
        .await?;
        let row = rows
            .first()
            .ok_or_else(|| Error::internal("SHOW CREATE returned no row"))?;
        let ddl: String = get(
            row,
            if command == "SEQUENCE" {
                "Create Sequence"
            } else {
                "Create Table"
            },
        )?;
        let ddl = localize(&ddl, &database, "ANSI_QUOTES", fold_names);
        if command == "TABLE" {
            let (table, foreign) = mysql_defer_foreign_keys(&ddl, &name)?;
            catalog.definitions.push(table);
            catalog.constraints.extend(foreign);
        } else {
            catalog.definitions.push(ddl);
        }
    }
    // Temporary stand-in tables let arbitrary view dependency graphs resolve.
    // Each is replaced by its real view, as mysqldump does; no data is inserted.
    for name in &views {
        let columns = query(
            conn,
            Backend::MySql,
            "SELECT column_name AS name, column_type AS data_type, collation_name AS collation
             FROM information_schema.columns
             WHERE table_schema=DATABASE() AND table_name=?
             ORDER BY ordinal_position",
            vec![name.clone().into()],
        )
        .await?;
        let mut definitions = Vec::new();
        for column in columns {
            let mut definition = format!(
                "{} {}",
                quote_ident(DatabaseType::MySQL, &get::<String>(&column, "name")?),
                get::<String>(&column, "data_type")?
            );
            if let Some(collation) = get::<Option<String>>(&column, "collation")? {
                definition.push_str(&format!(
                    " COLLATE {}",
                    quote_ident(DatabaseType::MySQL, &collation)
                ));
            }
            definitions.push(definition);
        }
        catalog.definitions.push(format!(
            "CREATE TABLE {} ({})",
            quote_ident(DatabaseType::MySQL, name),
            definitions.join(", ")
        ));
    }
    // Stored routines can be called by views and triggers. SHOW CREATE preserves
    // DEFINER, SQL SECURITY, characteristics, parameters and compound bodies.
    for row in query(
        conn,
        Backend::MySql,
        "SELECT routine_name AS name,routine_type AS kind
         FROM information_schema.routines
         WHERE routine_schema=DATABASE()
         ORDER BY routine_name",
        Vec::new(),
    )
    .await?
    {
        let name: String = get(&row, "name")?;
        let kind: String = get(&row, "kind")?;
        let key = match kind.as_str() {
            "FUNCTION" => "Create Function",
            "PROCEDURE" => "Create Procedure",
            _ => return Err(Error::internal("unknown routine type")),
        };
        stored_object(
            conn,
            &mut catalog.verbatim,
            &kind,
            &name,
            key,
            &database,
            fold_names,
        )
        .await?;
    }
    for name in views {
        let rows = query(
            conn,
            Backend::MySql,
            &format!(
                "SHOW CREATE VIEW {}",
                quote_ident(DatabaseType::MySQL, &name)
            ),
            Vec::new(),
        )
        .await?;
        let row = rows
            .first()
            .ok_or_else(|| Error::internal("SHOW CREATE VIEW returned no row"))?;
        let ddl: String = get(row, "Create View")?;
        catalog
            .verbatim
            .push("SET SESSION sql_mode = 'ANSI_QUOTES'".into());
        catalog.verbatim.push(format!(
            "SET SESSION collation_connection = {}",
            literal(&get::<String>(row, "collation_connection")?)
        ));
        catalog.verbatim.push(format!(
            "DROP TABLE {}",
            quote_ident(DatabaseType::MySQL, &name)
        ));
        catalog
            .verbatim
            .push(localize(&ddl, &database, "ANSI_QUOTES", fold_names));
    }
    for row in query(
        conn,
        Backend::MySql,
        "SELECT trigger_name AS name
         FROM information_schema.triggers
         WHERE trigger_schema=DATABASE()
         ORDER BY event_object_table,action_timing,event_manipulation,action_order",
        Vec::new(),
    )
    .await?
    {
        stored_object(
            conn,
            &mut catalog.constraints,
            "TRIGGER",
            &get::<String>(&row, "name")?,
            "SQL Original Statement",
            &database,
            fold_names,
        )
        .await?;
    }
    for row in query(
        conn,
        Backend::MySql,
        "SELECT event_name AS name
         FROM information_schema.events
         WHERE event_schema=DATABASE()
         ORDER BY event_name",
        Vec::new(),
    )
    .await?
    {
        stored_object(
            conn,
            &mut catalog.constraints,
            "EVENT",
            &get::<String>(&row, "name")?,
            "Create Event",
            &database,
            fold_names,
        )
        .await?;
    }
    if catalog
        .verbatim
        .iter()
        .chain(&catalog.constraints)
        .any(|sql| sql.starts_with("ALTER DATABASE COLLATE "))
    {
        catalog.constraints.push(database_collation);
    }
    catalog.constraints.extend([
        "SET SESSION sql_mode = @tideorm_saved_sql_mode".into(),
        "SET SESSION character_set_client = @tideorm_saved_client".into(),
        "SET SESSION character_set_results = @tideorm_saved_results".into(),
        "SET SESSION collation_connection = @tideorm_saved_collation".into(),
        "SET SESSION time_zone = @tideorm_saved_time_zone".into(),
    ]);
    Ok(catalog)
}

async fn stored_object(
    conn: &OrmConnection,
    output: &mut Vec<String>,
    kind: &str,
    name: &str,
    key: &str,
    database: &str,
    fold_names: bool,
) -> Result<()> {
    let rows = query(
        conn,
        Backend::MySql,
        &format!(
            "SHOW CREATE {kind} {}",
            quote_ident(DatabaseType::MySQL, name)
        ),
        Vec::new(),
    )
    .await?;
    let row = rows
        .first()
        .ok_or_else(|| Error::internal("SHOW CREATE stored object returned no row"))?;
    let ddl: Option<String> = get(row, key)?;
    let ddl=ddl.ok_or_else(||Error::query(format!("Cannot export {kind} {name}: SHOW CREATE did not disclose its definition; additional database privileges are required")))?;
    // Use canonical escaping while setting the object's original parsing mode.
    output.push("SET SESSION sql_mode = 'ANSI_QUOTES'".into());
    output.push(format!(
        "SET SESSION collation_connection = {}",
        literal(&get::<String>(row, "collation_connection")?)
    ));
    if kind == "EVENT" {
        output.push(format!(
            "SET SESSION time_zone = {}",
            literal(&get::<String>(row, "time_zone")?)
        ));
    }
    // Parameters, return values and local variables without an explicit
    // charset inherit the database's default at CREATE time, not the session's
    // connection collation. Restore that context for each stored program.
    output.push(format!(
        "ALTER DATABASE COLLATE {}",
        quote_ident(
            DatabaseType::MySQL,
            &get::<String>(row, "Database Collation")?
        )
    ));
    let mode: String = get(row, "sql_mode")?;
    output.push(format!("SET SESSION sql_mode = {}", literal(&mode)));
    output.push(script(&localize(&ddl, database, &mode, fold_names)));
    Ok(())
}

/// Rebind catalog-qualified references to the selected destination database.
/// A two-part column reference (`alias.column`) is deliberately left alone;
/// three-part columns, relation positions and qualified routine calls name a
/// database. Strings/comments, including dynamic SQL strings, are untouched.
fn localize(sql: &str, database: &str, mode: &str, fold_names: bool) -> String {
    use crate::internal::sql_lexer::{block_comment_len, quoted_len};
    let backslash = !mode.split(',').any(|s| s == "NO_BACKSLASH_ESCAPES");
    let ansi = mode.split(',').any(|s| s == "ANSI_QUOTES");
    let mut tokens: Vec<(usize, usize, String, bool)> = Vec::new();
    let mut pos = 0;
    while pos < sql.len() {
        let rest = &sql[pos..];
        let ch = rest.chars().next().unwrap();
        if ch.is_whitespace() {
            pos += ch.len_utf8();
            continue;
        }
        if rest.starts_with("/*") {
            pos += block_comment_len(rest, false);
            continue;
        }
        if ch == '#'
            || (rest.starts_with("--")
                && rest
                    .as_bytes()
                    .get(2)
                    .is_some_and(|b| b.is_ascii_whitespace() || b.is_ascii_control()))
        {
            pos += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        let (len, text, ident) = if matches!(ch, '\'' | '"' | '`') {
            let len = quoted_len(rest, ch, backslash && ch != '`');
            let ident = ch == '`' || ch == '"' && ansi;
            let text = if ident {
                rest[1..len - 1].replace(&format!("{ch}{ch}"), &ch.to_string())
            } else {
                String::new()
            };
            (len, text, ident)
        } else if ch == '_' || ch.is_alphabetic() {
            let len = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(rest.len());
            (len, rest[..len].to_owned(), true)
        } else {
            (ch.len_utf8(), ch.to_string(), false)
        };
        tokens.push((pos, pos + len, text, ident));
        pos += len;
    }
    let mut remove = Vec::new();
    // Track relation lists at each parenthesis depth. A comma in FROM/UPDATE
    // introduces another table; a comma in SELECT, a function call or SET does
    // not. Nested SELECTs get their own context and restore the outer one.
    let mut contexts = vec![(false, false)]; // (relation list, expects relation)
    let mut trigger_header = false;
    let mut index_header = false;
    for i in 0..tokens.len() {
        let token = &tokens[i];
        let word = token.3 && !matches!(sql.as_bytes()[token.0], b'`' | b'"');
        let upper = if word {
            token.2.to_ascii_uppercase()
        } else {
            String::new()
        };
        if token.2 == "(" {
            let relation_group = contexts.last().unwrap().1;
            contexts.last_mut().unwrap().1 = false;
            contexts.push((relation_group, relation_group));
            continue;
        }
        if token.2 == ")" {
            if contexts.len() > 1 {
                contexts.pop();
            }
            continue;
        }
        let context = contexts.last_mut().unwrap();
        if token.2 == ";" {
            *context = (false, false);
            trigger_header = false;
            index_header = false;
        }
        if token.2 == "," && context.0 {
            context.1 = true;
            continue;
        }
        if upper == "INDEX" {
            index_header = true;
        }
        if upper == "TRIGGER" {
            trigger_header = true;
        }
        if trigger_header && upper == "FOR" {
            trigger_header = false;
        }
        let previous = i.checked_sub(1).map(|j| tokens[j].2.to_ascii_uppercase());
        match upper.as_str() {
            "DELETE" | "FROM" | "JOIN" | "USING" | "TABLE" | "TABLES" => {
                *context = (true, true);
                continue;
            }
            "UPDATE" if previous.as_deref() != Some("KEY") => {
                *context = (true, true);
                continue;
            }
            "INSERT" | "REPLACE" if tokens.get(i + 1).is_none_or(|next| next.2 != "(") => {
                context.1 = true;
                continue;
            }
            "INTO" | "REFERENCES" | "CALL" | "TO" | "TRUNCATE" | "DESCRIBE" | "DESC" | "VIEW"
            | "FUNCTION" | "PROCEDURE" | "TRIGGER" | "EVENT" | "SEQUENCE" => {
                context.1 = true;
                continue;
            }
            "ON" if trigger_header || index_header => {
                index_header = false;
                context.1 = true;
                continue;
            }
            "SELECT" | "WHERE" | "HAVING" | "GROUP" | "ORDER" | "LIMIT" | "SET" | "VALUES"
            | "RETURNING" | "UNION" => {
                *context = (false, false);
                continue;
            }
            "IF" | "NOT" | "EXISTS" | "ONLY" | "LOW_PRIORITY" | "HIGH_PRIORITY" | "DELAYED"
            | "IGNORE" | "LATERAL"
                if context.1 =>
            {
                continue;
            }
            _ => {}
        }
        let relation = context.1;
        if token.3 {
            context.1 = false;
        }
        if !token.3
            || !(token.2 == database || fold_names && token.2.eq_ignore_ascii_case(database))
            || tokens.get(i + 1).is_none_or(|t| t.2 != ".")
            || tokens.get(i + 2).is_none_or(|t| !t.3)
        {
            continue;
        }
        let tail = tokens.get(i + 3).map(|t| t.2.as_str());
        if relation || matches!(tail, Some("." | "(")) {
            remove.push(token.0..tokens[i + 1].1);
        }
    }
    let mut output = sql.to_owned();
    for range in remove.into_iter().rev() {
        output.replace_range(range, "");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::localize;

    #[test]
    fn insert_and_replace_localize_optional_into_and_modifiers() {
        for function in ["INSERT(src.name, 1, 1, 'x')", "REPLACE(src.name, 'x', 'y')"] {
            assert_eq!(
                localize(
                    &format!(
                        "SELECT {function}
         FROM src.items AS src"
                    ),
                    "src",
                    "",
                    false
                ),
                format!(
                    "SELECT {function}
         FROM items AS src"
                )
            );
        }

        for verb in ["INSERT", "REPLACE"] {
            for modifier in ["", "LOW_PRIORITY ", "DELAYED ", "HIGH_PRIORITY IGNORE "] {
                for into in ["", "INTO "] {
                    let sql = format!("{verb} {modifier}{into}/* table */ `src`.`items` VALUES(7)");
                    let expected = format!("{verb} {modifier}{into}/* table */ `items` VALUES(7)");
                    assert_eq!(localize(&sql, "src", "", false), expected);
                }
            }
        }
        assert_eq!(
            localize(
                "INSERT src.items SELECT 'src.items' FROM external.items",
                "src",
                "",
                false
            ),
            "INSERT items SELECT 'src.items' FROM external.items"
        );
    }

    #[test]
    fn comments_and_qualified_delete_targets_are_portable() {
        for whitespace in ["\t", "\r", "\x0b", "\x0c", " "] {
            let sql =
                format!("BEGIN --{whitespace}don't parse this quote\nDELETE src.a FROM src.a; END");
            let expected =
                format!("BEGIN --{whitespace}don't parse this quote\nDELETE a FROM a; END");
            assert_eq!(localize(&sql, "src", "", false), expected);
        }
        for (sql, expected) in [
            (
                "DELETE LOW_PRIORITY IGNORE src.a, src.b FROM src.a JOIN src.b",
                "DELETE LOW_PRIORITY IGNORE a, b FROM a JOIN b",
            ),
            (
                "DELETE FROM src.a, src.b USING src.a JOIN src.b",
                "DELETE FROM a, b USING a JOIN b",
            ),
            ("DELETE `src`.`a` FROM `src`.`a`", "DELETE `a` FROM `a`"),
        ] {
            assert_eq!(localize(sql, "src", "", false), expected);
        }
    }

    #[test]
    fn local_database_relations_are_rebound_without_rewriting_aliases_or_literals() {
        assert_eq!(
            localize("DELETE FROM SRC.a", "src", "", true),
            "DELETE FROM a"
        );
        assert_eq!(
            localize("DELETE FROM SRC.a", "src", "", false),
            "DELETE FROM SRC.a"
        );

        for (sql, expected) in [
            ("TRUNCATE src.a", "TRUNCATE a"),
            (
                "RENAME TABLE src.a TO src.b, src.c TO src.d",
                "RENAME TABLE a TO b, c TO d",
            ),
            ("DROP INDEX idx ON src.a", "DROP INDEX idx ON a"),
            (
                "LOCK TABLES src.a WRITE, src.b READ",
                "LOCK TABLES a WRITE, b READ",
            ),
            (
                "DELETE x,y FROM src.a x, src.b y",
                "DELETE x,y FROM a x, b y",
            ),
            (
                "UPDATE src.a x, src.b y SET x.id=y.id",
                "UPDATE a x, b y SET x.id=y.id",
            ),
            (
                "SELECT src.id, src.name
         FROM src.a AS src",
                "SELECT src.id, src.name
         FROM a AS src",
            ),
            (
                "SELECT COALESCE(src.id, src.other)
         FROM src.a src",
                "SELECT COALESCE(src.id, src.other)
         FROM a src",
            ),
            (
                "SELECT *
         FROM (src.a x, src.b y) JOIN src.c z ON x.id=z.id",
                "SELECT *
         FROM (a x, b y) JOIN c z ON x.id=z.id",
            ),
            (
                "SELECT *
         FROM src.a x, (SELECT *
         FROM src.b y, src.c z) d, src.e",
                "SELECT *
         FROM a x, (SELECT *
         FROM b y, c z) d, e",
            ),
            (
                "CREATE TRIGGER src.guard BEFORE INSERT ON src.a FOR EACH ROW SET NEW.id=1",
                "CREATE TRIGGER guard BEFORE INSERT ON a FOR EACH ROW SET NEW.id=1",
            ),
            (
                "SELECT src.a.id, src.f(1), 'src.a'
         FROM src.a /* src.b */",
                "SELECT a.id, f(1), 'src.a'
         FROM a /* src.b */",
            ),
            (
                "ALTER TABLE src.a ADD FOREIGN KEY (id) REFERENCES src.b(id)",
                "ALTER TABLE a ADD FOREIGN KEY (id) REFERENCES b(id)",
            ),
        ] {
            assert_eq!(localize(sql, "src", "", false), expected, "{sql}");
        }
    }
}
