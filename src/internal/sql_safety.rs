use crate::config::DatabaseType;
use crate::internal::Backend;

#[cfg(feature = "fulltext")]
mod fulltext;

#[cfg(feature = "fulltext")]
pub(crate) use fulltext::{
    escape_fts5_query_literal_terms, fts5_boolean_query, fts5_near_query, fts5_phrase_query,
    fts5_prefix_query, sanitize_mysql_fulltext_query, sanitize_postgres_boolean_tsquery,
    sanitize_postgres_proximity_tsquery_literals, sanitize_postgres_tsquery_literals,
};

/// Escape `value` for use inside a single-quoted SQL literal on `db_type`.
pub(crate) fn escape_sql_literal_for_db(db_type: DatabaseType, value: &str) -> String {
    let escaped = value.replace('\'', "''");
    match db_type {
        DatabaseType::MySQL | DatabaseType::MariaDB => escaped.replace('\\', "\\\\"),
        DatabaseType::Postgres | DatabaseType::SQLite => escaped,
    }
}

pub(crate) fn is_safe_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
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
/// [`consume_quoted_run`].
///
/// Returns a short description of the offending construct, or `None` when the
/// fragment is free of them.
fn find_forbidden_raw_sql_token(sql: &str) -> Option<&'static str> {
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '\'' => match consume_quoted_run(&chars, &mut index, '\'') {
                QuotedRun::Closed => {}
                QuotedRun::Unterminated => return Some("unterminated string literals"),
                QuotedRun::AmbiguousEscape => {
                    return Some("backslash-escaped quotes inside string literals");
                }
            },
            '"' | '`' => match consume_quoted_run(&chars, &mut index, ch) {
                QuotedRun::Closed => {}
                QuotedRun::Unterminated => return Some("unterminated quoted identifiers"),
                QuotedRun::AmbiguousEscape => {
                    return Some("backslash-escaped quotes inside quoted identifiers");
                }
            },
            ';' => return Some("statement separators"),
            '\0' => return Some("NUL bytes"),
            '#' => return Some("SQL comments"),
            '-' if chars.get(index + 1) == Some(&'-') => return Some("SQL comments"),
            '/' if chars.get(index + 1) == Some(&'*') => return Some("SQL comments"),
            '*' if chars.get(index + 1) == Some(&'/') => return Some("SQL comments"),
            _ => index += 1,
        }
    }

    None
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

/// How a quoted string literal or quoted identifier ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotedRun {
    /// The run was closed by an unescaped quote character.
    Closed,
    /// Input ran out before the closing quote.
    Unterminated,
    /// A backslash sat immediately before the quote character, so the run ends
    /// here on some backends and continues on others.
    AmbiguousEscape,
}

/// Walk a quoted string literal or quoted identifier starting at its opening
/// quote, leaving `index` just past the closing quote when the run is closed.
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
fn consume_quoted_run(chars: &[char], index: &mut usize, quote: char) -> QuotedRun {
    *index += 1;

    while *index < chars.len() {
        let ch = chars[*index];

        if ch == '\\' {
            match chars.get(*index + 1) {
                Some(&next) if next == quote => return QuotedRun::AmbiguousEscape,
                Some(&'\\') => *index += 2,
                _ => *index += 1,
            }
            continue;
        }

        if ch == quote {
            if chars.get(*index + 1) == Some(&quote) {
                *index += 2;
            } else {
                *index += 1;
                return QuotedRun::Closed;
            }
        } else {
            *index += 1;
        }
    }

    QuotedRun::Unterminated
}

fn consume_numeric_literal(chars: &[char], index: &mut usize) {
    *index += 1;

    while *index < chars.len()
        && (chars[*index].is_ascii_digit() || chars[*index] == '.' || chars[*index] == '_')
    {
        *index += 1;
    }

    if *index < chars.len() && (chars[*index] == 'e' || chars[*index] == 'E') {
        let exponent_start = *index;
        *index += 1;

        if *index < chars.len() && (chars[*index] == '+' || chars[*index] == '-') {
            *index += 1;
        }

        let exponent_digits_start = *index;
        while *index < chars.len() && chars[*index].is_ascii_digit() {
            *index += 1;
        }

        if exponent_digits_start == *index {
            *index = exponent_start;
        }
    }
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
/// `QuotedRun::AmbiguousEscape`.
fn scan_sql_tokens(
    sql: &str,
    kind: &str,
    mut visit: impl FnMut(SqlToken) -> std::result::Result<(), String>,
) -> std::result::Result<(), String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;
    let mut paren_depth = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            _ if ch.is_whitespace() => {
                index += 1;
            }
            '\'' => {
                if consume_quoted_run(&chars, &mut index, '\'') != QuotedRun::Closed {
                    return Err(format!("unsafe {}: unterminated string literal", kind));
                }
            }
            '"' | '`' => {
                if consume_quoted_run(&chars, &mut index, ch) != QuotedRun::Closed {
                    return Err(format!("unsafe {}: unterminated quoted identifier", kind));
                }
            }
            '(' => {
                paren_depth += 1;
                index += 1;
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(format!("unsafe {}: unbalanced closing parenthesis", kind));
                }
                paren_depth -= 1;
                index += 1;
            }
            _ if ch.is_ascii_digit() => {
                consume_numeric_literal(&chars, &mut index);
            }
            _ if ch == '_' || ch.is_ascii_alphabetic() => {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index] == '_' || chars[index].is_ascii_alphanumeric())
                {
                    index += 1;
                }

                let mut next = index;
                while next < chars.len() && chars[next].is_whitespace() {
                    next += 1;
                }
                visit(SqlToken::Word {
                    text: chars[start..index].iter().collect(),
                    depth: paren_depth,
                    called: chars.get(next) == Some(&'('),
                })?;
            }
            _ => {
                visit(SqlToken::Symbol(ch))?;
                index += 1;
            }
        }
    }

    if paren_depth != 0 {
        return Err(format!("unsafe {}: unbalanced parentheses", kind));
    }

    Ok(())
}

/// The lowercased words of `sql` outside any parentheses.
fn collect_top_level_sql_tokens(sql: &str, kind: &str) -> std::result::Result<Vec<String>, String> {
    Ok(collect_top_level_sql_words(sql, kind)?
        .into_iter()
        .map(|(word, _)| word)
        .collect())
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
            text,
            depth: 0,
            called,
        } = token
        {
            words.push((text.to_ascii_lowercase(), called));
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

    let top_level_tokens = collect_top_level_sql_tokens(sql, "subquery")?;
    let starts_like_subquery = matches!(
        top_level_tokens.first().map(String::as_str),
        Some("select") | Some("with")
    );

    if !starts_like_subquery {
        return Err(
            "unsafe subquery: expected a SELECT/WITH query generated by QueryBuilder".to_string(),
        );
    }

    if top_level_tokens.first().map(String::as_str) == Some("with")
        && !top_level_tokens.iter().any(|token| token == "select")
    {
        return Err(
            "unsafe subquery: WITH queries must terminate in a top-level SELECT statement"
                .to_string(),
        );
    }

    // A forbidden word followed by `(` names a function — `REPLACE(name, ..)`
    // — not the statement.
    if let Some((token, _)) = collect_top_level_sql_words(sql, "subquery")?
        .into_iter()
        .find(|(token, called)| !called && is_forbidden_top_level_subquery_keyword(token))
    {
        return Err(format!(
            "unsafe subquery: keyword '{}' is not allowed in raw subquery fragments",
            token
        ));
    }

    if !allow_top_level_set_ops
        && let Some(token) = top_level_tokens
            .iter()
            .find(|token| matches!(token.as_str(), "union" | "intersect" | "except"))
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
        && parts.len() <= 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && is_safe_identifier_segment(part))
    {
        return Ok(());
    }

    Err(format!(
        "invalid {} '{}': expected column or table.column using only ASCII letters, numbers, and underscores",
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
    let q = db_type.quote_char();
    out.push(q);
    for ch in name.chars() {
        if ch == q {
            out.push(q);
        }
        out.push(ch);
    }
    out.push(q);
}

pub(crate) fn quote_ident_for_backend(backend: Backend, name: &str) -> String {
    quote_ident(backend.as_database_type(), name)
}

pub(crate) fn format_identifier_reference(db_type: DatabaseType, value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('"')
        || trimmed.ends_with('"')
        || trimmed.starts_with('`')
        || trimmed.ends_with('`')
        || trimmed.contains('(')
        || trimmed.contains(')')
        || trimmed.contains('*')
        || trimmed.contains(' ')
    {
        return None;
    }

    if trimmed.split('.').any(str::is_empty) {
        return None;
    }

    let mut reference = String::with_capacity(trimmed.len() + 4);
    for (index, part) in trimmed.split('.').enumerate() {
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
