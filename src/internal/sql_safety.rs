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
