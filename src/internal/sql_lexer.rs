//! Shared lexical boundaries for SQL inspection; this is not a SQL grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Word,
    Identifier,
    Literal,
    Comment,
    Space,
    Symbol,
    Parameter,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    pub text: &'a str,
    pub start: usize,
    pub kind: Kind,
}

pub(crate) fn tokens(sql: &str) -> impl Iterator<Item = Token<'_>> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        let rest = &sql[offset..];
        let first = rest.chars().next()?;
        let mut len = first.len_utf8();
        let kind;
        if first.is_whitespace() {
            kind = Kind::Space;
            len = rest
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(rest.len());
        } else if rest.starts_with("--") {
            kind = Kind::Comment;
            len = rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("/*") {
            kind = Kind::Comment;
            len = block_comment_len(rest, true);
        } else if matches!(first, '\'' | '"' | '`' | '[') {
            kind = if first == '\'' {
                Kind::Literal
            } else {
                Kind::Identifier
            };
            let escaped = first == '\''
                && offset > 0
                && matches!(sql.as_bytes()[offset - 1], b'e' | b'E')
                && (offset == 1
                    || !sql[..offset - 1]
                        .chars()
                        .next_back()
                        .is_some_and(identifier_char));
            len = quoted_len(rest, first, escaped);
        } else if first == '$' {
            if let Some(n) = dollar_len(rest) {
                kind = Kind::Literal;
                len = n;
            } else {
                len = 1 + rest[1..].bytes().take_while(u8::is_ascii_digit).count();
                kind = if len > 1 {
                    Kind::Parameter
                } else {
                    Kind::Symbol
                };
            }
        } else if first == '_' || first.is_alphabetic() {
            kind = Kind::Word;
            len = rest
                .find(|c: char| !identifier_char(c))
                .unwrap_or(rest.len());
        } else {
            kind = Kind::Symbol;
        }
        let token = Token {
            text: &rest[..len],
            start: offset,
            kind,
        };
        offset += len;
        Some(token)
    })
}
fn identifier_char(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}
pub(crate) fn block_comment_len(sql: &str, nested: bool) -> usize {
    let mut depth = 1;
    let mut pos = 2;
    while pos < sql.len() {
        if nested && sql[pos..].starts_with("/*") {
            depth += 1;
            pos += 2;
        } else if sql[pos..].starts_with("*/") {
            depth -= 1;
            pos += 2;
            if depth == 0 {
                return pos;
            }
        } else {
            pos += sql[pos..].chars().next().unwrap().len_utf8();
        }
    }
    pos
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EscapeMode {
    Doubled,
    Backslash,
    RejectAmbiguous,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuotedRun {
    Closed,
    Unterminated,
    AmbiguousEscape,
}

/// Scan after an opening delimiter. Positions may be bytes or character indices;
/// callers supply the corresponding width and end so validation stays allocation-free.
pub(crate) fn quoted_end(
    chars: impl Iterator<Item = (usize, char)>,
    close: char,
    mode: EscapeMode,
    end: usize,
    width: impl Fn(char) -> usize,
) -> (usize, QuotedRun) {
    let mut chars = chars.peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '\\' && mode != EscapeMode::Doubled {
            if mode == EscapeMode::RejectAmbiguous {
                if chars.peek().is_some_and(|(_, next)| *next == close) {
                    return (index, QuotedRun::AmbiguousEscape);
                }
                if chars.peek().is_some_and(|(_, next)| *next == '\\') {
                    chars.next();
                }
            } else {
                chars.next();
            }
        } else if ch == close {
            if chars.peek().is_some_and(|(_, next)| *next == close) {
                chars.next();
            } else {
                return (index + width(ch), QuotedRun::Closed);
            }
        }
    }
    (end, QuotedRun::Unterminated)
}

pub(crate) fn quoted_len(sql: &str, delimiter: char, backslash: bool) -> usize {
    let close = if delimiter == '[' { ']' } else { delimiter };
    let mode = if backslash {
        EscapeMode::Backslash
    } else {
        EscapeMode::Doubled
    };
    quoted_end(
        sql.char_indices().skip(1),
        close,
        mode,
        sql.len(),
        char::len_utf8,
    )
    .0
}
fn dollar_len(sql: &str) -> Option<usize> {
    let end = sql[1..].find('$')? + 1;
    let tag = &sql[1..end];
    if tag.starts_with(|c: char| c.is_ascii_digit())
        || !tag.chars().all(|c| c == '_' || c.is_alphanumeric())
    {
        return None;
    }
    let delimiter = &sql[..=end];
    sql[end + 1..]
        .find(delimiter)
        .map(|n| end + 1 + n + delimiter.len())
}
pub(crate) fn significant(sql: &str) -> impl Iterator<Item = Token<'_>> {
    tokens(sql).filter(|token| !matches!(token.kind, Kind::Space | Kind::Comment))
}
/// Keywords at the statement's outer level (CTE bodies are nested).
pub(crate) fn top_words(sql: &str) -> Vec<&str> {
    let mut depth: usize = 0;
    significant(sql)
        .filter_map(|token| {
            match token.text {
                "(" => depth += 1,
                ")" => depth = depth.saturating_sub(1),
                _ => {}
            }
            (depth == 0 && token.kind == Kind::Word).then_some(token.text)
        })
        .collect()
}
