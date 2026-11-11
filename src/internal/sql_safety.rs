use super::sql_lexer::{EscapeMode, Kind, QuotedRun, Token, quoted_end};
use crate::config::DatabaseType;
use crate::orm::sql::Dialect;

#[cfg(feature = "fulltext")]
mod fulltext;

#[cfg(feature = "fulltext")]
pub(crate) use fulltext::{
    double_quoted, escape_fts5_query_literal_terms, fts5_boolean_query, fts5_near_query,
    fts5_phrase_query, fts5_prefix_query, has_searchable_char, sanitize_mysql_fulltext_query,
    sanitize_postgres_boolean_tsquery, sanitize_postgres_proximity_tsquery_literals,
    sanitize_postgres_tsquery_literals, search_words,
};

pub(crate) use crate::orm::sql_builder::is_plain_identifier as is_safe_identifier_segment;

/// The tokens of a raw fragment, read under PostgreSQL's rules whatever the
/// backend: `[` quotes nothing there, so a bracket's contents are checked as
/// SQL, and a dollar quote is a token of its own, which can be refused.
fn fragment_tokens(sql: &str) -> impl Iterator<Item = Token<'_>> {
    crate::orm::sql::tokens(sql, Dialect::Postgres)
}

/// Scan a raw SQL fragment for statement separators and comment introducers that
/// appear outside a quoted string literal or quoted identifier.
///
/// The scan is literal-aware so that a `#` or `--` that is genuinely part of a
/// value (`'issue #42'`) is not mistaken for a comment, while the same token in
/// expression position is still rejected. `#` matters because MySQL and MariaDB
/// treat it as a line comment: a trailing `#` silently comments out every clause
/// the builder appends after the fragment, including soft-delete `IS NULL`
/// scoping, later `AND` predicates, `ORDER BY`, and `LIMIT`.
///
/// Being literal-aware means the scan has to agree with the server about where
/// each literal *ends*, which is where backslashes come in — see
/// [`quoted_run`].
///
/// Returns a short description of the offending construct, or `None` when the
/// fragment is free of them.
fn find_forbidden_raw_sql_token(sql: &str) -> Option<&'static str> {
    let mut tokens = fragment_tokens(sql).peekable();
    while let Some(token) = tokens.next() {
        let literal = token.kind == Kind::Literal;
        let reason = match token.kind {
            // Both quote a `'` the scan would read as a literal's quote, so
            // they would hide a comment from it; the backends disagree on them.
            Kind::Literal if token.text.starts_with('$') => "dollar-quoted strings",
            Kind::Symbol if token.text == "[" && bracket_holds_quote(&sql[token.start..]) => {
                "quotes inside square brackets"
            }
            Kind::Literal | Kind::Identifier => match quoted_run(&sql[token.start..]) {
                QuotedRun::Closed => continue,
                QuotedRun::Unterminated if literal => "unterminated string literals",
                QuotedRun::Unterminated => "unterminated quoted identifiers",
                QuotedRun::AmbiguousEscape if literal => {
                    "backslash-escaped quotes inside string literals"
                }
                QuotedRun::AmbiguousEscape => "backslash-escaped quotes inside quoted identifiers",
            },
            Kind::Comment => "SQL comments",
            Kind::Symbol => match token.text {
                ";" => "statement separators",
                "\0" => "NUL bytes",
                "#" => "SQL comments",
                "*" if tokens.peek().is_some_and(|next| next.text == "/") => "SQL comments",
                _ => continue,
            },
            Kind::Word | Kind::Space | Kind::Parameter => continue,
        };
        return Some(reason);
    }
    // Another backend can end a quote elsewhere: SQLite's `@name(..)`
    // parameter runs to a `)`, quotes included, so `@a('x) -- '` hides a
    // comment there.
    let other_backends = [Dialect::MySql, Dialect::Sqlite];
    other_backends.into_iter().find_map(|dialect| {
        crate::orm::sql::tokens(sql, dialect).find_map(|token| match token.kind {
            Kind::Comment => Some("SQL comments"),
            _ if !token.terminated => Some("unterminated quotes"),
            Kind::Symbol if token.text == ";" => Some("statement separators"),
            _ => None,
        })
    })
}

/// Whether the `[` opening `sql` and the next `]` hold a quote: SQLite reads
/// the brackets as a quoted identifier, in which a quote is an ordinary
/// character.
fn bracket_holds_quote(sql: &str) -> bool {
    sql[1..]
        .chars()
        .take_while(|ch| *ch != ']')
        .any(|ch| matches!(ch, '\'' | '"' | '`'))
}

pub(crate) fn validate_raw_sql_fragment(kind: &str, sql: &str) -> std::result::Result<(), String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(format!("unsafe {}: SQL fragment cannot be empty", kind));
    }

    if let Some(reason) = find_forbidden_raw_sql_token(trimmed) {
        return Err(format!(
            "unsafe {}: raw SQL fragments may not contain {}; use parameterized query builder APIs instead",
            kind, reason
        ));
    }

    Ok(())
}

/// How the quoted string literal or quoted identifier opening `sql` ends,
/// read the way no backend disagrees with.
///
/// A doubled quote (`''`, `""`, or a doubled backtick) always continues the run
/// on every backend TideORM targets. Backslashes are the hard part, because
/// they are dialect-dependent: MySQL and MariaDB honour `\'` as an escaped
/// quote under the default `sql_mode`, PostgreSQL does not with
/// `standard_conforming_strings=on`, and SQLite never does. A backslash
/// immediately before the quote character therefore *closes* the run on one
/// backend and *continues* it on another, and a scanner that commits to either
/// reading goes blind to an injection aimed at the other one:
///
/// - reading `\'` as an escape misses `'a\' AND 1=1 --'`, where PostgreSQL ends
///   the literal at the second quote and treats the rest as SQL;
/// - reading `\'` as a closing quote misses `'a\'' -- '`, where MySQL keeps the
///   literal open past the doubled quote and treats the trailing `--` as a
///   comment that swallows the soft-delete scoping, later `AND` predicates,
///   `ORDER BY`, and `LIMIT` the builder appends after the fragment.
///
/// The validator has no backend in scope — fragments are checked when the
/// builder method is called, which can precede any connection — and this is a
/// rejection filter rather than a renderer, so the sequence is reported as
/// [`QuotedRun::AmbiguousEscape`] and the whole fragment is refused. Nothing
/// legitimate is lost: generated SQL is deliberately backslash-free crate-wide
/// (the `LIKE` escape character is `!` for this same family of reasons), so a
/// backslash hugging a quote in a raw fragment is already anomalous.
///
/// `\\` is consumed as a pair instead, because every reading agrees on where it
/// ends — that keeps a value such as `'C:\\'` (as MySQL escapes it) and a plain
/// `'C:\temp'` from being rejected for no reason.
fn quoted_run(sql: &str) -> QuotedRun {
    let quote = sql.chars().next().unwrap_or('\'');
    quoted_end(
        sql.char_indices().skip(1),
        quote,
        EscapeMode::RejectAmbiguous,
        sql.len(),
        char::len_utf8,
    )
    .1
}

/// The length of the number opening `sql`: digits, `.` and `_`, then an
/// exponent when one with digits follows. The lexer has no number token; a
/// number reads as digit symbols, and an exponent as a word (`1e5union` holds
/// the word `union`).
fn numeric_literal_len(sql: &str) -> usize {
    let bytes = sql.as_bytes();
    let mut end = 1 + bytes[1..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'_'))
        .count();
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let digits = bytes[end + 1 + sign..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits > 0 {
            end += 1 + sign + digits;
        }
    }
    end
}

/// One top-level lexical unit of a raw fragment.
enum SqlToken {
    /// A bare word — keyword or identifier — at the given parenthesis depth;
    /// `called` when a `(` follows it, as it does a function's name.
    Word {
        text: String,
        depth: usize,
        called: bool,
    },
    /// Any other character outside a literal, a number or a parenthesis.
    Symbol(char),
}

/// Hand every bare word and symbol of `sql` to `visit`.
///
/// Quoted literals and identifiers, numbers, whitespace and parentheses are
/// consumed here — parentheses only to track depth and to reject an imbalance.
/// Callers run after `validate_raw_sql_fragment`, which already rejects
/// `QuotedRun::AmbiguousEscape`, comments and dollar quotes.
fn scan_sql_tokens(
    sql: &str,
    kind: &str,
    mut visit: impl FnMut(SqlToken) -> std::result::Result<(), String>,
) -> std::result::Result<(), String> {
    let mut paren_depth = 0usize;
    let mut number_end = 0;

    for token in fragment_tokens(sql) {
        let end = token.start + token.text.len();
        if end <= number_end {
            continue;
        }
        // The rest of a word a number runs into.
        let start = token.start.max(number_end);
        let text = &sql[start..end];
        match token.kind {
            Kind::Literal | Kind::Identifier if !token.terminated => {
                let what = if token.kind == Kind::Literal {
                    "string literal"
                } else {
                    "quoted identifier"
                };
                return Err(format!("unsafe {}: unterminated {}", kind, what));
            }
            Kind::Space | Kind::Comment | Kind::Literal | Kind::Identifier => {}
            Kind::Symbol if text == "(" => paren_depth += 1,
            Kind::Symbol if text == ")" => {
                if paren_depth == 0 {
                    return Err(format!("unsafe {}: unbalanced closing parenthesis", kind));
                }
                paren_depth -= 1;
            }
            Kind::Symbol if text.starts_with(|ch: char| ch.is_ascii_digit()) => {
                number_end = start + numeric_literal_len(&sql[start..]);
            }
            Kind::Word => visit(SqlToken::Word {
                text: text.to_string(),
                depth: paren_depth,
                called: sql[end..].trim_start().starts_with('('),
            })?,
            Kind::Symbol | Kind::Parameter => {
                visit(SqlToken::Symbol(text.chars().next().unwrap_or_default()))?
            }
        }
    }

    if paren_depth != 0 {
        return Err(format!("unsafe {}: unbalanced parentheses", kind));
    }

    Ok(())
}

/// The lowercased words of `sql` outside any parentheses, each with whether a
/// `(` follows it.
fn collect_top_level_sql_words(
    sql: &str,
    kind: &str,
) -> std::result::Result<Vec<(String, bool)>, String> {
    let mut words = Vec::new();
    scan_sql_tokens(sql, kind, |token| {
        if let SqlToken::Word {
            mut text,
            depth: 0,
            called,
        } = token
        {
            text.make_ascii_lowercase();
            words.push((text, called));
        }
        Ok(())
    })?;

    Ok(words)
}

fn is_forbidden_top_level_subquery_keyword(token: &str) -> bool {
    matches!(
        token,
        "insert"
            | "update"
            | "delete"
            | "drop"
            | "alter"
            | "create"
            | "truncate"
            | "returning"
            | "merge"
            | "replace"
            | "upsert"
            | "grant"
            | "revoke"
            | "call"
            | "execute"
            | "values"
    )
}

fn validate_subquery_sql_with_mode(
    sql: &str,
    allow_top_level_set_ops: bool,
) -> std::result::Result<(), String> {
    validate_raw_sql_fragment("subquery", sql)?;

    let top_level_words = collect_top_level_sql_words(sql, "subquery")?;
    let first_word = top_level_words.first().map(|(word, _)| word.as_str());
    let starts_like_subquery = matches!(first_word, Some("select") | Some("with"));

    if !starts_like_subquery {
        return Err(
            "unsafe subquery: expected a SELECT/WITH query generated by QueryBuilder".to_string(),
        );
    }

    if first_word == Some("with") && !top_level_words.iter().any(|(word, _)| word == "select") {
        return Err(
            "unsafe subquery: WITH queries must terminate in a top-level SELECT statement"
                .to_string(),
        );
    }

    // A forbidden word followed by `(` names a function — `REPLACE(name, ..)`
    // — not the statement.
    if let Some((token, _)) = top_level_words
        .iter()
        .find(|(token, called)| !called && is_forbidden_top_level_subquery_keyword(token))
    {
        return Err(format!(
            "unsafe subquery: keyword '{}' is not allowed in raw subquery fragments",
            token
        ));
    }

    if !allow_top_level_set_ops
        && let Some((token, _)) = top_level_words
            .iter()
            .find(|(token, _)| matches!(token.as_str(), "union" | "intersect" | "except"))
    {
        return Err(format!(
            "unsafe subquery: top-level '{}' queries are not allowed here; use QueryBuilder union()/union_all()/with_recursive_cte() APIs instead",
            token
        ));
    }

    Ok(())
}

fn is_forbidden_having_keyword(token: &str) -> bool {
    matches!(
        token,
        "select"
            | "with"
            | "join"
            | "inner"
            | "left"
            | "right"
            | "cross"
            | "union"
            | "intersect"
            | "except"
            | "insert"
            | "update"
            | "delete"
            | "drop"
            | "alter"
            | "create"
            | "truncate"
            | "returning"
            | "exists"
            | "into"
            | "limit"
            | "offset"
            | "window"
            | "over"
    )
}

pub(crate) fn validate_having_sql_fragment(
    kind: &str,
    sql: &str,
) -> std::result::Result<(), String> {
    validate_raw_sql_fragment(kind, sql)?;

    scan_sql_tokens(sql, kind, |token| match token {
        // `LEFT(name, 1)` and `RIGHT(..)` are string functions, not joins.
        SqlToken::Word {
            ref text,
            called: true,
            ..
        } if matches!(text.to_ascii_lowercase().as_str(), "left" | "right") => Ok(()),
        SqlToken::Word { text, .. } if is_forbidden_having_keyword(&text.to_ascii_lowercase()) => {
            Err(format!(
                "unsafe {}: keyword '{}' is not allowed in raw HAVING clauses",
                kind, text
            ))
        }
        SqlToken::Word { .. } => Ok(()),
        // `#` is deliberately absent: it introduces a line comment on
        // MySQL/MariaDB and is rejected by `validate_raw_sql_fragment` above.
        SqlToken::Symbol(
            '.' | ',' | '*' | '+' | '-' | '/' | '%' | '=' | '<' | '>' | '!' | '|' | '&' | '@' | '?'
            | ':',
        ) => Ok(()),
        SqlToken::Symbol(ch) => Err(format!(
            "unsafe {}: unexpected character '{}' in raw HAVING clause",
            kind, ch
        )),
    })
}

pub(crate) fn validate_subquery_sql(sql: &str) -> std::result::Result<(), String> {
    validate_subquery_sql_with_mode(sql, false)
}

pub(crate) fn validate_compound_subquery_sql(sql: &str) -> std::result::Result<(), String> {
    validate_subquery_sql_with_mode(sql, true)
}

pub(crate) fn validate_identifier(kind: &str, value: &str) -> std::result::Result<(), String> {
    if !value.is_empty() && is_safe_identifier_segment(value) {
        return Ok(());
    }

    Err(format!(
        "unsafe {} '{}': identifiers may only contain ASCII letters, numbers, and underscores, and must not start with a number",
        kind, value
    ))
}

pub(crate) fn validate_identifier_reference(
    kind: &str,
    value: &str,
) -> std::result::Result<(), String> {
    let parts: Vec<&str> = value.split('.').collect();
    if !parts.is_empty()
        && parts.len() <= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && is_safe_identifier_segment(part))
    {
        return Ok(());
    }

    Err(format!(
        "invalid {} '{}': expected column, table.column or schema.table.column using only ASCII letters, numbers, and underscores",
        kind, value
    ))
}

pub(crate) fn validate_join_column(value: &str) -> std::result::Result<(), String> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() == 2 && parts.iter().all(|part| is_safe_identifier_segment(part)) {
        return Ok(());
    }

    Err(format!(
        "unsafe JOIN column reference '{}': expected table.column using only ASCII letters, numbers, and underscores",
        value
    ))
}

pub(crate) fn quote_ident(db_type: DatabaseType, name: &str) -> String {
    let mut quoted = String::with_capacity(name.len() + 2);
    push_quoted_ident(&mut quoted, db_type, name);
    quoted
}

/// Append `name` to `out` as a quoted identifier, doubling any quote inside it.
///
/// Every statement quotes each column it names, so this builds in place
/// rather than allocating per identifier.
pub(crate) fn push_quoted_ident(out: &mut String, db_type: DatabaseType, name: &str) {
    db_type.dialect().push_identifier(out, name);
}

/// `object` quoted and, when `schema` is given, qualified with it:
/// `"schema"."object"`.
pub(crate) fn quote_qualified(db_type: DatabaseType, schema: Option<&str>, object: &str) -> String {
    let mut quoted = String::new();
    push_quoted_qualified(&mut quoted, db_type, schema, object);
    quoted
}

/// Append [`quote_qualified`]'s rendering to `out`.
pub(crate) fn push_quoted_qualified(
    out: &mut String,
    db_type: DatabaseType,
    schema: Option<&str>,
    object: &str,
) {
    if let Some(schema) = schema {
        push_quoted_ident(out, db_type, schema);
        out.push('.');
    }
    push_quoted_ident(out, db_type, object);
}

/// `columns` quoted and joined for a key, index or insert column list, each
/// after `prefix`, such as a trigger's `new.`.
pub(crate) fn column_list(
    db_type: DatabaseType,
    columns: &[impl AsRef<str>],
    prefix: &str,
) -> String {
    columns
        .iter()
        .map(|column| format!("{prefix}{}", quote_ident(db_type, column.as_ref())))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The parts of a plain identifier reference (`column`, `table.column`,
/// `schema.table.column`): not quoted, with no parenthesis, `*` or space, and
/// no empty part. `None` for anything else, such as an expression.
pub(crate) fn identifier_reference_parts(value: &str) -> Option<Vec<&str>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('"')
        || trimmed.ends_with('"')
        || trimmed.starts_with('`')
        || trimmed.ends_with('`')
        || trimmed.contains(['(', ')', '*', ' '])
    {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    (!parts.iter().any(|part| part.is_empty())).then_some(parts)
}

pub(crate) fn format_identifier_reference(db_type: DatabaseType, value: &str) -> Option<String> {
    let parts = identifier_reference_parts(value)?;
    let mut reference = String::with_capacity(value.len() + 4);
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            reference.push('.');
        }
        push_quoted_ident(&mut reference, db_type, part);
    }
    Some(reference)
}

#[cfg(test)]
#[path = "../../tests/unit/sql_safety_tests.rs"]
mod tests;
