//! Property tests for the full-text sanitizers: whatever a user types, each
//! backend's search syntax comes out well formed, and empty only when the
//! input holds no word.

use super::*;
use crate::test_support::Rng;

/// Words, and every character some backend's search syntax gives a meaning.
const SEARCH_PIECES: &[&str] = &[
    "a", "word", "é", "1", "_", " ", " ", "\t", "\"", "'", "''", "+", "-", "~", "<", ">", "(", ")",
    "*", "@", ":", "^", "!", "&", "|", "\\", ".", ",", "AND", "OR", "NOT", "NEAR", "🎉", "\0",
];

fn inputs(seed: u64) -> impl Iterator<Item = String> {
    let mut rng = Rng::new(seed);
    (0..5_000).map(move |_| rng.text(SEARCH_PIECES, 10))
}

fn postgres_queries(input: &str) -> [String; 4] {
    [
        sanitize_postgres_tsquery_literals(input, false),
        sanitize_postgres_tsquery_literals(input, true),
        sanitize_postgres_proximity_tsquery_literals(input, 3),
        sanitize_postgres_boolean_tsquery(input),
    ]
}

fn fts5_queries(input: &str) -> [String; 5] {
    [
        escape_fts5_query_literal_terms(input),
        fts5_boolean_query(input),
        fts5_phrase_query(input),
        fts5_prefix_query(input),
        fts5_near_query(input, 5),
    ]
}

fn mysql_operands(input: &str) -> [String; 2] {
    [
        sanitize_mysql_fulltext_query(input, false),
        sanitize_mysql_fulltext_query(input, true),
    ]
}

#[test]
fn a_search_is_empty_exactly_when_the_input_holds_no_word() {
    for input in inputs(0x5eed_0101) {
        let word = has_searchable_char(&input);
        let [literal, boolean, phrase, prefix, near] = fts5_queries(&input);
        // FTS5's NOT needs a term to subtract from.
        let fts5 = [literal, phrase, prefix, near];
        for query in postgres_queries(&input)
            .into_iter()
            .chain(fts5)
            .chain(mysql_operands(&input))
        {
            assert_eq!(query.is_empty(), !word, "{input:?} gave {query:?}");
        }
        assert!(word || boolean.is_empty(), "{input:?} gave {boolean:?}");
    }
}

/// Whether `rest` opens with a tsquery operand, which it then skips:
/// a quoted lexeme (a quote doubled inside) with an optional `:*`, a
/// negated operand, or a parenthesized expression.
fn tsquery_operand(rest: &mut &str) -> bool {
    *rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix('!') {
        *rest = after;
        return tsquery_operand(rest);
    }
    if let Some(after) = rest.strip_prefix('(') {
        *rest = after;
        if !tsquery_expression(rest) {
            return false;
        }
        let Some(after) = rest.trim_start().strip_prefix(')') else {
            return false;
        };
        *rest = after;
        return true;
    }
    let Some(mut after) = rest.strip_prefix('\'') else {
        return false;
    };
    let mut lexeme = String::new();
    loop {
        let Some(end) = after.find('\'') else {
            return false;
        };
        lexeme.push_str(&after[..end]);
        after = &after[end + 1..];
        match after.strip_prefix('\'') {
            Some(doubled) => {
                lexeme.push('\'');
                after = doubled;
            }
            None => break,
        }
    }
    *rest = after.strip_prefix(":*").unwrap_or(after);
    has_searchable_char(&lexeme)
        && lexeme
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '\''))
}

/// Whether `rest` opens with operands joined by `&`, `|`, `<->` or `<n>`.
fn tsquery_expression(rest: &mut &str) -> bool {
    loop {
        if !tsquery_operand(rest) {
            return false;
        }
        let trimmed = rest.trim_start();
        let operator = if trimmed.starts_with(['&', '|']) {
            1
        } else if let Some(distance) = trimmed.strip_prefix('<') {
            match distance.find('>') {
                Some(end)
                    if &distance[..end] == "-"
                        || (end > 0 && distance[..end].bytes().all(|b| b.is_ascii_digit())) =>
                {
                    end + 2
                }
                _ => return false,
            }
        } else {
            return true;
        };
        *rest = &trimmed[operator..];
    }
}

#[test]
fn every_postgres_search_is_a_tsquery() {
    for input in inputs(0x5eed_0102) {
        for query in postgres_queries(&input) {
            let mut rest = query.as_str();
            let parsed =
                query.is_empty() || (tsquery_expression(&mut rest) && rest.trim().is_empty());
            assert!(parsed, "{input:?} gave {query:?}");
        }
    }
}

/// The characters InnoDB's full-text parser gives a meaning.
const MYSQL_OPERATORS: &[char] = &['+', '-', '<', '>', '(', ')', '~', '*', '"', '@'];

/// Whether InnoDB's boolean mode reads `operand` as terms and phrases, each
/// with at most one leading operator, a term with an optional trailing `*`.
fn mysql_boolean_operand(operand: &str) -> bool {
    let mut rest = operand;
    while !rest.is_empty() {
        let unsigned = rest.strip_prefix(['+', '-', '~', '<', '>']).unwrap_or(rest);
        let (searchable, after) = match unsigned.strip_prefix('"') {
            Some(phrase) => match phrase.find('"') {
                Some(end) => (has_searchable_char(&phrase[..end]), &phrase[end + 1..]),
                None => return false,
            },
            None => {
                let end = unsigned.find(' ').unwrap_or(unsigned.len());
                let word = &unsigned[..end];
                let word = word.strip_suffix('*').unwrap_or(word);
                let plain = has_searchable_char(word) && !word.contains(MYSQL_OPERATORS);
                (plain, &unsigned[end..])
            }
        };
        if !searchable {
            return false;
        }
        rest = match after.strip_prefix(' ') {
            Some(next) if !next.is_empty() => next,
            None if after.is_empty() => after,
            _ => return false,
        };
    }
    true
}

#[test]
fn every_mysql_search_is_an_operand_innodb_parses() {
    for input in inputs(0x5eed_0103) {
        let [natural, boolean] = mysql_operands(&input);
        assert!(
            !natural.contains(MYSQL_OPERATORS),
            "{input:?} gave {natural:?}"
        );
        assert!(
            mysql_boolean_operand(&boolean),
            "{input:?} gave {boolean:?}"
        );
    }
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn fts5_parses_every_search_built_from_user_input() {
    let db = crate::test_support::sqlite_memory(&[
        "CREATE VIRTUAL TABLE docs USING fts5(body)",
        "INSERT INTO docs (body) VALUES ('a word é 1')",
    ])
    .await;
    for input in inputs(0x5eed_0104).take(1_500) {
        for query in fts5_queries(&input) {
            if query.is_empty() {
                continue;
            }
            let matched = db
                .query_raw_json_with_params(
                    "SELECT count(*) AS n FROM docs WHERE docs MATCH ?",
                    [crate::internal::Value::from(query.clone())],
                )
                .await;
            if let Err(error) = matched {
                panic!("{input:?} gave {query:?}, which FTS5 refused: {error}");
            }
        }
    }
}
