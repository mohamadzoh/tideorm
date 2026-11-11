//! Property tests over generated SQL: the lexer, the raw-fragment validator
//! and template binding, each against what every backend's lexer reads.

use super::*;
use crate::config::DatabaseType;
use crate::internal::Value;
use crate::orm::sql::{Dialect, Kind, tokens};
use crate::orm::sql_builder::BoundPart;
use crate::query::db_sql::{count_template_placeholders, template_bound};
use crate::test_support::Rng;

const DIALECTS: [Dialect; 4] = [
    Dialect::Any,
    Dialect::MySql,
    Dialect::Postgres,
    Dialect::Sqlite,
];

/// Pieces each backend lexes differently, and plain text between them.
const SQL_PIECES: &[&str] = &[
    "'", "\"", "`", "[", "]", "\\", "--", "-", "/*", "*/", "/", "*", "#", ";", "?", "$", "$$",
    "$a$", "E", "e", "a", "x1", " ", "\n", "\r", "\t", "(", ")", ":", "::", "@", "=", "1", "$1",
    "\0", "é", "🎉", "\u{feff}",
];

/// Mostly fragments a validator accepts, with the pieces that could end one
/// early on some backend.
const FRAGMENT_PIECES: &[&str] = &[
    "a", "b", " ", " ", "=", "1", "(", ")", ",", "'x'", "'", "\"", "`", "[", "]", "\\", "--", "-",
    "/*", "*/", "#", ";", "$", "$a", "E", "@a(", ":a(", "$a(", "#a(", "::", "\n", "é",
];

#[test]
fn every_dialect_lexes_any_text_into_tokens_that_spell_it() {
    let mut rng = Rng::new(0x5eed_0001);
    for _ in 0..20_000 {
        let sql = rng.text(SQL_PIECES, 16);
        for dialect in DIALECTS {
            let mut offset = 0;
            for token in tokens(&sql, dialect) {
                assert!(!token.text.is_empty(), "{dialect:?} {sql:?}");
                assert_eq!(token.start, offset, "{dialect:?} {sql:?}");
                assert_eq!(&sql[offset..offset + token.text.len()], token.text);
                offset += token.text.len();
            }
            assert_eq!(offset, sql.len(), "{dialect:?} {sql:?}");
        }
    }
}

/// The first token of `fragment` that would end it or hide what follows it,
/// read under `dialect`.
fn escape_under(fragment: &str, dialect: Dialect) -> Option<String> {
    tokens(fragment, dialect)
        .find(|token| token.kind == Kind::Comment || !token.terminated || token.is_symbol(&[";"]))
        .map(|token| token.text.to_string())
}

#[test]
fn a_fragment_the_validator_accepts_ends_where_every_backend_reads_it_ending() {
    let mut rng = Rng::new(0x5eed_0002);
    let mut accepted = 0;
    for _ in 0..50_000 {
        let fragment = rng.text(FRAGMENT_PIECES, 10);
        if validate_raw_sql_fragment("WHERE raw SQL", &fragment).is_err() {
            continue;
        }
        accepted += 1;
        for dialect in [Dialect::MySql, Dialect::Postgres, Dialect::Sqlite] {
            if let Some(token) = escape_under(&fragment, dialect) {
                panic!("accepted {fragment:?}, which {dialect:?} reads as {token:?}");
            }
        }
    }
    assert!(accepted > 1_000, "only {accepted} fragments were accepted");
}

/// Placeholders, and `?`s that quotes, comments or a neighbour make no
/// placeholder.
const TEMPLATE_PIECES: &[&str] = &[
    "?", "?", "'?'", "\"?\"", "`?`", "--?\n", "/*?*/", "??", "?|", "a", " ", "=", "'", "$1", "(",
    ")",
];

#[test]
fn a_template_binds_each_placeholder_to_its_value_and_keeps_values_out_of_the_text() {
    let mut rng = Rng::new(0x5eed_0003);
    for _ in 0..10_000 {
        let template = rng.text(TEMPLATE_PIECES, 12);
        let values: Vec<Value> = (0..count_template_placeholders(&template))
            .map(|index| Value::from(format!("§{index}'; -- ")))
            .collect();
        let bound = template_bound(&template, values.clone());

        let mut text = String::new();
        let mut bound_values = Vec::new();
        for part in bound.parts() {
            match part {
                BoundPart::Sql(sql) => text.push_str(sql),
                BoundPart::Value(value) => {
                    text.push('?');
                    bound_values.push(value.clone());
                }
            }
        }
        assert_eq!(text, template);
        assert_eq!(bound_values, values, "{template:?}");

        for backend in [
            DatabaseType::Postgres,
            DatabaseType::MySQL,
            DatabaseType::SQLite,
        ] {
            let (sql, params) = bound.clone().build(backend.dialect());
            assert_eq!(params, values, "{backend:?} {template:?}");
            assert!(!sql.contains('§'), "{backend:?} wrote a value into {sql:?}");
        }
    }
}
