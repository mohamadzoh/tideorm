//! Shared lexical boundaries for SQL inspection; this is not a SQL grammar.
//! The lexer is the SQL builder's; TideORM reads SQL meant for any backend with
//! [`Dialect::Any`] and SQLite's own with [`Dialect::Sqlite`].
use crate::orm::sql::Dialect;

pub(crate) use crate::orm::sql::{
    EscapeMode, Kind, QuotedRun, Token, block_comment_len, quoted_end, quoted_len,
};

pub(crate) fn tokens(sql: &str) -> impl Iterator<Item = Token<'_>> {
    crate::orm::sql::tokens(sql, Dialect::Any)
}

/// SQLite has non-nesting block comments, no dollar-quoted strings, and no
/// PostgreSQL E-string escapes. Keep those boundaries when inspecting its SQL.
pub(crate) fn sqlite_tokens(sql: &str) -> impl Iterator<Item = Token<'_>> {
    crate::orm::sql::tokens(sql, Dialect::Sqlite)
}

pub(crate) fn significant(sql: &str) -> impl Iterator<Item = Token<'_>> {
    tokens(sql).filter(|token| !matches!(token.kind, Kind::Space | Kind::Comment))
}

/// [`significant`] for SQLite's SQL, read by [`sqlite_tokens`].
pub(crate) fn sqlite_significant(sql: &str) -> impl Iterator<Item = Token<'_>> {
    sqlite_tokens(sql).filter(|token| !matches!(token.kind, Kind::Space | Kind::Comment))
}

/// Keywords at the statement's outer level (CTE bodies are nested), lexed
/// only as far as the caller reads.
pub(crate) fn top_words(sql: &str) -> impl Iterator<Item = &str> {
    let mut depth: usize = 0;
    significant(sql).filter_map(move |token| {
        match token.text {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            _ => {}
        }
        (depth == 0 && token.kind == Kind::Word).then_some(token.text)
    })
}
