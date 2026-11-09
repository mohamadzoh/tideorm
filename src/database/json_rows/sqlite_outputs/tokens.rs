//! Reading the token stream: parentheses, keywords, names and the
//! expressions that return a JSON document.

use crate::internal::sql_lexer::{Kind, Token};

/// The names of the parenthesized column list at `at`, none when there is no
/// list, and where the statement goes on after it.
pub(super) fn column_list(tokens: &[Token<'_>], at: usize) -> Option<(Vec<String>, usize)> {
    if !is_symbol(tokens, at, &["("]) {
        return Some((Vec::new(), at));
    }
    let close = closing_parenthesis(tokens, at)?;
    let names = tokens[at + 1..close]
        .iter()
        .filter(|token| token.text != ",")
        .map(unquoted)
        .collect();
    Some((names, close + 1))
}

/// Indices outside parentheses, including each opening parenthesis. Quoted
/// tokens stay intact because all inspection uses the shared SQL lexer.
pub(super) fn top_level<'a>(tokens: &'a [Token<'a>]) -> impl Iterator<Item = usize> + 'a {
    let mut at = 0;
    std::iter::from_fn(move || {
        let current = at;
        tokens.get(current)?;
        at = if is_symbol(tokens, current, &["("]) {
            closing_parenthesis(tokens, current).map_or(tokens.len(), |close| close + 1)
        } else {
            current + 1
        };
        Some(current)
    })
}

/// Where a JSON expression starting at `start` ends: the index of the token
/// after it. That is a call of one of SQLite's functions returning a document,
/// alone or in parentheses, or any operand read with `->` (not `->>`), once or
/// more.
pub(super) fn json_expression_end(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    let (mut at, mut document) = json_operand(tokens, start)?;
    while is_symbol(tokens, at, &["-"])
        && is_symbol(tokens, at + 1, &[">"])
        && !is_symbol(tokens, at + 2, &[">"])
    {
        at = json_path_end(tokens, at + 2)?;
        document = true;
    }
    document.then_some(at)
}

/// Where the operand of `->` starting at `start` ends, and whether it is a
/// document by itself: a call of a function returning one, or such a call in
/// parentheses. A column, a literal, a parameter and any other call or
/// parenthesized expression are documents only once `->` reads them.
fn json_operand(tokens: &[Token<'_>], start: usize) -> Option<(usize, bool)> {
    let first = tokens.get(start)?;
    if is_symbol(tokens, start, &["("]) {
        let close = closing_parenthesis(tokens, start)?;
        let inner = &tokens[start + 1..close];
        return Some((
            close + 1,
            json_expression_end(inner, 0) == Some(inner.len()),
        ));
    }
    if first.kind == Kind::Word && is_symbol(tokens, start + 1, &["("]) {
        let close = closing_parenthesis(tokens, start + 1)?;
        let arguments = &tokens[start + 2..close];
        let name = first.text.to_ascii_lowercase();
        // Single-path json_extract returns SQL scalars as well as JSON
        // containers. Its text cannot distinguish "[2]" from [2]. With
        // multiple paths it always returns a JSON array instead.
        let document = matches!(
            name.as_str(),
            "json"
                | "json_array"
                | "json_object"
                | "json_quote"
                | "json_group_array"
                | "json_group_object"
                | "json_insert"
                | "json_replace"
                | "json_set"
                | "json_remove"
                | "json_patch"
                | "json_pretty"
        ) || (name == "json_extract"
            && top_level(arguments)
                .filter(|&at| is_symbol(arguments, at, &[","]))
                .count()
                >= 2);
        // An aggregate's `FILTER (WHERE ..)` and `OVER` its window.
        let mut at = close + 1;
        if is_word(tokens, at, &["filter"]) && is_symbol(tokens, at + 1, &["("]) {
            at = closing_parenthesis(tokens, at + 1)? + 1;
        }
        if is_word(tokens, at, &["over"]) {
            at = if is_symbol(tokens, at + 1, &["("]) {
                closing_parenthesis(tokens, at + 1)? + 1
            } else {
                at + 2
            };
        }
        return Some((at, document));
    }
    let reference = |at: usize| {
        tokens
            .get(at)
            .is_some_and(|token| matches!(token.kind, Kind::Word | Kind::Identifier))
    };
    if matches!(first.kind, Kind::Literal | Kind::Parameter) {
        return Some((start + 1, false));
    }
    if !reference(start) {
        return None;
    }
    let mut at = start + 1;
    while is_symbol(tokens, at, &["."]) && reference(at + 1) {
        at += 2;
    }
    Some((at, false))
}

/// Where the path of `->` starting at `at` ends: a string, a parameter (`?`,
/// `?1`, `:name`) or an array index.
fn json_path_end(tokens: &[Token<'_>], at: usize) -> Option<usize> {
    let path = tokens.get(at)?;
    let digit = |token: &Token<'_>| {
        token.kind == Kind::Symbol && token.text.bytes().all(|byte| byte.is_ascii_digit())
    };
    let digits = |from: usize| {
        tokens.get(from..).map_or(0, |rest| {
            rest.iter().take_while(|token| digit(token)).count()
        })
    };
    match path.kind {
        Kind::Literal | Kind::Parameter => Some(at + 1),
        Kind::Symbol if path.text == "?" => Some(at + 1 + digits(at + 1)),
        _ if digit(path) => Some(at + digits(at)),
        _ => None,
    }
}

/// The index of the `)` closing the `(` at `open`.
pub(super) fn closing_parenthesis(tokens: &[Token<'_>], open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, token) in tokens.iter().enumerate().skip(open) {
        if token.kind != Kind::Symbol {
            continue;
        }
        match token.text {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether what is at `at` can follow a whole select item.
pub(super) fn ends_select_item(tokens: &[Token<'_>], at: usize) -> bool {
    at == tokens.len()
        || is_symbol(tokens, at, &[",", ")", ";"])
        || is_word(
            tokens,
            at,
            &[
                "from",
                "where",
                "group",
                "having",
                "window",
                "order",
                "limit",
                "union",
                "intersect",
                "except",
            ],
        )
}

pub(super) fn is_word(tokens: &[Token<'_>], at: usize, words: &[&str]) -> bool {
    tokens.get(at).is_some_and(|token| token.is_word(words))
}

pub(super) fn is_symbol(tokens: &[Token<'_>], at: usize, symbols: &[&str]) -> bool {
    tokens.get(at).is_some_and(|token| token.is_symbol(symbols))
}

/// The name `token` gives a column; see [`Token::unquoted`].
pub(super) fn unquoted(token: &Token<'_>) -> String {
    token.unquoted().unwrap_or_default()
}
