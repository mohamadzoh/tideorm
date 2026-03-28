use crate::config::DatabaseType;
use crate::internal::DbBackend;

pub(crate) fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

pub(crate) fn is_safe_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn contains_forbidden_raw_sql_token(sql: &str) -> bool {
    sql.contains(';')
        || sql.contains("--")
        || sql.contains("/*")
        || sql.contains("*/")
        || sql.chars().any(|ch| ch == '\0')
}

pub(crate) fn validate_raw_sql_fragment(kind: &str, sql: &str) -> std::result::Result<(), String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(format!("unsafe {}: SQL fragment cannot be empty", kind));
    }

    if contains_forbidden_raw_sql_token(trimmed) {
        return Err(format!(
            "unsafe {}: raw SQL fragments may not contain statement separators, SQL comments, or NUL bytes; use parameterized query builder APIs instead",
            kind
        ));
    }

    Ok(())
}

fn consume_single_quoted_sql_string(chars: &[char], index: &mut usize) -> bool {
    *index += 1;

    while *index < chars.len() {
        if chars[*index] == '\'' {
            if *index + 1 < chars.len() && chars[*index + 1] == '\'' {
                *index += 2;
            } else {
                *index += 1;
                return true;
            }
        } else {
            *index += 1;
        }
    }

    false
}

fn consume_quoted_identifier(chars: &[char], index: &mut usize, quote: char) -> bool {
    *index += 1;

    while *index < chars.len() {
        if chars[*index] == quote {
            if *index + 1 < chars.len() && chars[*index + 1] == quote {
                *index += 2;
            } else {
                *index += 1;
                return true;
            }
        } else {
            *index += 1;
        }
    }

    false
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
            '\'' => {
                if !consume_single_quoted_sql_string(&chars, &mut index) {
                    return Err(format!("unsafe {}: unterminated string literal", kind));
                }
            }
            '"' | '`' => {
                if !consume_quoted_identifier(&chars, &mut index, ch) {
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
            '.' | ',' | '*' | '+' | '-' | '/' | '%' | '=' | '<' | '>' | '!' | '|' | '&' | '#'
            | '@' | '?' | ':' => {
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
    validate_raw_sql_fragment("subquery", sql)?;

    let trimmed = sql.trim_start();
    let starts_like_subquery = trimmed
        .get(..6)
        .map(|prefix| prefix.eq_ignore_ascii_case("select"))
        .unwrap_or(false)
        || trimmed
            .get(..4)
            .map(|prefix| prefix.eq_ignore_ascii_case("with"))
            .unwrap_or(false);

    if starts_like_subquery {
        Ok(())
    } else {
        Err("unsafe subquery: expected a SELECT/WITH query generated by QueryBuilder".to_string())
    }
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

pub(crate) fn quote_ident_for_backend(backend: DbBackend, name: &str) -> String {
    let db_type = match backend {
        DbBackend::Postgres => DatabaseType::Postgres,
        DbBackend::MySql => DatabaseType::MySQL,
        DbBackend::Sqlite => DatabaseType::SQLite,
        _ => DatabaseType::Postgres,
    };
    quote_ident(db_type, name)
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

#[cfg(feature = "fulltext")]
struct SearchSegment {
    text: String,
    quoted: bool,
}

#[cfg(feature = "fulltext")]
fn split_search_segments(input: &str) -> Vec<SearchSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' if in_quotes => {
                let text = current.trim();
                if !text.is_empty() {
                    segments.push(SearchSegment {
                        text: text.to_string(),
                        quoted: true,
                    });
                }
                current.clear();
                in_quotes = false;
            }
            '"' => {
                let text = current.trim();
                if !text.is_empty() {
                    segments.push(SearchSegment {
                        text: text.to_string(),
                        quoted: false,
                    });
                    current.clear();
                }
                in_quotes = true;
            }
            _ if ch.is_whitespace() && !in_quotes => {
                let text = current.trim();
                if !text.is_empty() {
                    segments.push(SearchSegment {
                        text: text.to_string(),
                        quoted: false,
                    });
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    let text = current.trim();
    if !text.is_empty() {
        segments.push(SearchSegment {
            text: text.to_string(),
            quoted: in_quotes,
        });
    }

    segments
}

#[cfg(feature = "fulltext")]
fn extract_postgres_lexemes(input: &str) -> Vec<String> {
    let mut lexemes = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if ch.is_alphanumeric() || matches!(ch, '_' | '\'') {
            current.push(ch);
        } else if !current.is_empty() {
            lexemes.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        lexemes.push(current);
    }

    lexemes
}

#[cfg(feature = "fulltext")]
fn format_postgres_lexeme(lexeme: &str, prefix: bool) -> String {
    let escaped = lexeme.replace('\'', "''");
    if prefix {
        format!("'{}':*", escaped)
    } else {
        format!("'{}'", escaped)
    }
}

#[cfg(feature = "fulltext")]
pub(crate) fn sanitize_postgres_tsquery_literals(input: &str, prefix: bool) -> String {
    let parts: Vec<String> = split_search_segments(input)
        .into_iter()
        .filter_map(|segment| {
            let lexemes = extract_postgres_lexemes(&segment.text);
            if lexemes.is_empty() {
                return None;
            }

            let joiner = if segment.quoted { " <-> " } else { " & " };
            let formatted: Vec<String> = lexemes
                .iter()
                .map(|lexeme| format_postgres_lexeme(lexeme, prefix))
                .collect();

            let phrase = formatted.join(joiner);
            Some(if formatted.len() > 1 {
                format!("({})", phrase)
            } else {
                phrase
            })
        })
        .collect();

    if parts.is_empty() {
        String::new()
    } else {
        parts.join(" & ")
    }
}

#[cfg(feature = "fulltext")]
pub(crate) fn sanitize_postgres_proximity_tsquery_literals(input: &str, distance: u32) -> String {
    let parts: Vec<String> = split_search_segments(input)
        .into_iter()
        .filter_map(|segment| {
            let lexemes = extract_postgres_lexemes(&segment.text);
            if lexemes.is_empty() {
                return None;
            }

            let phrase = lexemes
                .iter()
                .map(|lexeme| format_postgres_lexeme(lexeme, false))
                .collect::<Vec<_>>()
                .join(" <-> ");

            Some(if lexemes.len() > 1 {
                format!("({})", phrase)
            } else {
                phrase
            })
        })
        .collect();

    if parts.is_empty() {
        String::new()
    } else {
        parts.join(&format!(" <{}> ", distance))
    }
}

#[cfg(feature = "fulltext")]
pub(crate) fn escape_fts5_query_literal_terms(input: &str) -> String {
    split_search_segments(input)
        .into_iter()
        .map(|segment| format!("\"{}\"", segment.text.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
