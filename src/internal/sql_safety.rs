use crate::config::DatabaseType;
use crate::internal::Backend;

#[cfg(feature = "fulltext")]
mod fulltext;

#[cfg(feature = "fulltext")]
pub(crate) use fulltext::{
    escape_fts5_query_literal_terms, sanitize_postgres_proximity_tsquery_literals,
    sanitize_postgres_tsquery_literals,
};

pub(crate) fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

pub(crate) fn escape_sql_literal_for_db(db_type: DatabaseType, value: &str) -> String {
    let escaped = escape_sql_literal(value);
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

fn collect_top_level_sql_tokens(sql: &str, kind: &str) -> std::result::Result<Vec<String>, String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;
    let mut paren_depth = 0usize;
    let mut tokens = Vec::new();

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            _ if ch.is_whitespace() => {
                index += 1;
            }
            // Both arms below run after `validate_raw_sql_fragment`, which
            // already rejects `QuotedRun::AmbiguousEscape`.
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

                if paren_depth == 0 {
                    let token: String = chars[start..index].iter().collect();
                    tokens.push(token.to_ascii_lowercase());
                }
            }
            _ => {
                index += 1;
            }
        }
    }

    if paren_depth != 0 {
        return Err(format!("unsafe {}: unbalanced parentheses", kind));
    }

    Ok(tokens)
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

    if let Some(token) = top_level_tokens
        .iter()
        .find(|token| is_forbidden_top_level_subquery_keyword(token))
    {
        return Err(format!(
            "unsafe subquery: keyword '{}' is not allowed in raw subquery fragments",
            token
        ));
    }

    if !allow_top_level_set_ops {
        if let Some(token) = top_level_tokens
            .iter()
            .find(|token| matches!(token.as_str(), "union" | "intersect" | "except"))
        {
            return Err(format!(
                "unsafe subquery: top-level '{}' queries are not allowed here; use QueryBuilder union()/union_all()/with_recursive_cte() APIs instead",
                token
            ));
        }
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

    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;
    let mut paren_depth = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        match ch {
            _ if ch.is_whitespace() => {
                index += 1;
            }
            // Both arms below run after `validate_raw_sql_fragment`, which
            // already rejects `QuotedRun::AmbiguousEscape`.
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

                let token: String = chars[start..index].iter().collect();
                let lowered = token.to_ascii_lowercase();

                if is_forbidden_having_keyword(&lowered) {
                    return Err(format!(
                        "unsafe {}: keyword '{}' is not allowed in raw HAVING clauses",
                        kind, token
                    ));
                }

                if !is_safe_identifier_segment(&token) {
                    return Err(format!(
                        "unsafe {}: token '{}' is not allowed in raw HAVING clauses",
                        kind, token
                    ));
                }
            }
            // `#` is deliberately absent: it introduces a line comment on
            // MySQL/MariaDB and is rejected by `validate_raw_sql_fragment` above.
            '.' | ',' | '*' | '+' | '-' | '/' | '%' | '=' | '<' | '>' | '!' | '|' | '&' | '@'
            | '?' | ':' => {
                index += 1;
            }
            _ => {
                return Err(format!(
                    "unsafe {}: unexpected character '{}' in raw HAVING clause",
                    kind, ch
                ));
            }
        }
    }

    if paren_depth != 0 {
        return Err(format!("unsafe {}: unbalanced parentheses", kind));
    }

    Ok(())
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
        "unsafe {} '{}': JOIN identifiers may only contain ASCII letters, numbers, and underscores, and must not start with a number",
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

pub(crate) fn quote_char(db_type: DatabaseType) -> char {
    match db_type {
        DatabaseType::Postgres | DatabaseType::SQLite => '"',
        DatabaseType::MySQL | DatabaseType::MariaDB => '`',
    }
}

pub(crate) fn quote_ident(db_type: DatabaseType, name: &str) -> String {
    let q = quote_char(db_type);
    let escaped = name.replace(q, &format!("{q}{q}"));
    format!("{}{}{}", q, escaped, q)
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

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.iter().any(|part| part.is_empty()) {
        return None;
    }

    Some(
        parts
            .into_iter()
            .map(|part| quote_ident(db_type, part))
            .collect::<Vec<_>>()
            .join("."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mysql_backslash_escaped_quote_cannot_smuggle_a_trailing_comment() {
        // MySQL and MariaDB read `\'` as an escaped quote under the default
        // `sql_mode`, so the literal closes at the *doubled* quote and the
        // trailing `--` comments out the soft-delete scoping, later `AND`
        // predicates, `ORDER BY`, and `LIMIT` that the builder appends.
        let error = validate_raw_sql_fragment("WHERE raw SQL", r"name = 'a\'' -- '").unwrap_err();

        assert!(error.contains("unsafe WHERE raw SQL"), "{error}");
        assert!(error.contains("backslash-escaped quotes"), "{error}");
    }

    #[test]
    fn a_postgres_backslash_before_a_closing_quote_cannot_smuggle_a_trailing_comment() {
        // The mirror image: with `standard_conforming_strings=on` PostgreSQL
        // (and SQLite always) reads the backslash as data and closes the
        // literal at the very next quote, leaving ` AND 1=1 --` as live SQL.
        let error =
            validate_raw_sql_fragment("WHERE raw SQL", r"name = 'a\' AND 1=1 --'").unwrap_err();

        assert!(error.contains("unsafe WHERE raw SQL"), "{error}");
        assert!(error.contains("backslash-escaped quotes"), "{error}");
    }

    #[test]
    fn a_backslash_before_a_closing_quoted_identifier_is_rejected() {
        let error = validate_raw_sql_fragment("WHERE raw SQL", r#""na\" -- " = 1"#).unwrap_err();

        assert!(error.contains("unsafe WHERE raw SQL"), "{error}");
        assert!(
            error.contains("backslash-escaped quotes inside quoted identifiers"),
            "{error}"
        );

        let backtick_error =
            validate_raw_sql_fragment("WHERE raw SQL", "`na\\` -- ` = 1").unwrap_err();
        assert!(
            backtick_error.contains("backslash-escaped quotes inside quoted identifiers"),
            "{backtick_error}"
        );
    }

    #[test]
    fn backslash_ambiguity_is_rejected_in_having_and_subquery_fragments_too() {
        let having_error =
            validate_having_sql_fragment("HAVING raw SQL", r"COUNT(*) > 1 AND x = 'a\'' -- '")
                .unwrap_err();
        assert!(
            having_error.contains("backslash-escaped quotes"),
            "{having_error}"
        );

        let subquery_error =
            validate_subquery_sql(r"SELECT id FROM users WHERE name = 'a\'' -- '").unwrap_err();
        assert!(
            subquery_error.contains("backslash-escaped quotes"),
            "{subquery_error}"
        );
    }

    #[test]
    fn a_trailing_backslash_does_not_swallow_the_rest_of_the_fragment() {
        // `'oops\` is unterminated on every backend; the scan must not walk off
        // the end silently and report the fragment as clean.
        let error = validate_raw_sql_fragment("WHERE raw SQL", r"note = 'oops\").unwrap_err();

        assert!(error.contains("unsafe WHERE raw SQL"), "{error}");

        let unterminated = validate_raw_sql_fragment("WHERE raw SQL", "note = 'oops").unwrap_err();
        assert!(
            unterminated.contains("unterminated string literals"),
            "{unterminated}"
        );
    }

    #[test]
    fn comment_introducers_inside_a_literal_are_still_accepted() {
        // The whole point of the literal-aware scan: a value that merely looks
        // like a comment must not be rejected, and a bound value never reaches
        // the scanner at all.
        validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = 'buy 2 -- get 1 free'")
            .expect("a comment introducer inside a literal is just data");
        validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = $1")
            .expect("a bound placeholder carries no literal at all");
        validate_raw_sql_fragment("WHERE raw SQL", "\"note\" = ?")
            .expect("a bound placeholder carries no literal at all");
    }

    #[test]
    fn backslashes_that_are_not_adjacent_to_a_quote_stay_accepted() {
        validate_raw_sql_fragment("WHERE raw SQL", r"path = 'C:\temp'")
            .expect("a backslash in the middle of a literal ends it in no dialect");
        validate_raw_sql_fragment("WHERE raw SQL", r"path = 'C:\\'")
            .expect("an escaped backslash ends the literal at the same quote everywhere");
        validate_raw_sql_fragment("WHERE raw SQL", r"note = 'a\nb'")
            .expect("a newline escape does not move the closing quote");
    }

    #[test]
    fn an_escaped_backslash_does_not_hide_a_following_comment() {
        // `'C:\\'` closes on both readings, so the trailing `--` is live SQL.
        let error = validate_raw_sql_fragment("WHERE raw SQL", r"path = 'C:\\' -- ").unwrap_err();

        assert!(error.contains("SQL comments"), "{error}");
    }
}
