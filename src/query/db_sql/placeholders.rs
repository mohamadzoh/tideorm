use crate::config::DatabaseType;
use crate::internal::{
    MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder, Token, Tokenizer, Value,
    inject_parameters, placeholder,
};

/// One marker per bound value, numbered from 1.
pub(crate) fn placeholders(db_type: DatabaseType, count: usize) -> Vec<String> {
    (1..=count)
        .map(|index| placeholder(db_type, index))
        .collect()
}

/// Render `sql` with every bound value written in as a literal, for display only.
///
/// The literals come from sea-query's own [`inject_parameters`], so they are
/// spelled exactly as the backend's query builder spells them. That function
/// indexes `params` by each placeholder it finds and panics on one without a
/// value — which a caller's raw fragment can easily carry — so a statement whose
/// placeholders do not line up with its values is returned unchanged.
pub(crate) fn inline_parameters(db_type: DatabaseType, sql: &str, params: &[Value]) -> String {
    if !placeholders_match_values(db_type, sql, params.len()) {
        return sql.to_string();
    }

    match db_type {
        DatabaseType::Postgres => inject_parameters(sql, params, &PostgresQueryBuilder),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            inject_parameters(sql, params, &MysqlQueryBuilder)
        }
        DatabaseType::SQLite => inject_parameters(sql, params, &SqliteQueryBuilder),
    }
}

/// Whether the placeholders `inject_parameters` will find in `sql` are exactly
/// the `bound` values.
///
/// The public tokenizer always scans like MySQL, and the backends only disagree
/// about a backslash escaping a quote. Generated SQL is backslash-free, so a
/// statement that contains one — only a raw fragment can — is left alone.
fn placeholders_match_values(db_type: DatabaseType, sql: &str, bound: usize) -> bool {
    if sql.contains('\\') {
        return false;
    }

    let mut tokens = Tokenizer::new(sql).iter().peekable();
    let mut seen = vec![false; bound];
    let mut positional = 0;

    while let Some(token) = tokens.next() {
        match (db_type, token) {
            (DatabaseType::Postgres, Token::Punctuation("$")) => {
                let Some(Token::Unquoted(number)) = tokens.peek() else {
                    continue;
                };
                let Ok(index) = number.parse::<usize>() else {
                    continue;
                };
                match index.checked_sub(1).and_then(|slot| seen.get_mut(slot)) {
                    Some(slot) => *slot = true,
                    None => return false,
                }
            }
            (
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite,
                Token::Punctuation("?"),
            ) => positional += 1,
            _ => {}
        }
    }

    match db_type {
        DatabaseType::Postgres => seen.into_iter().all(|used| used),
        DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => positional == bound,
    }
}

/// A fragment rendered on its own, spliced behind `offset` values already
/// bound in the surrounding statement.
///
/// PostgreSQL's `$n` markers are numbered, so they are shifted past those
/// values; MySQL and SQLite's `?` markers depend only on the order values are
/// bound in, so their fragment is spliced in unchanged.
pub(crate) fn rebase_placeholders(db_type: DatabaseType, sql: &str, offset: usize) -> String {
    match db_type {
        DatabaseType::Postgres => offset_postgres_placeholders(sql, offset),
        DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => sql.to_string(),
    }
}

/// Shift every `$n` placeholder in `sql` up by `offset`.
///
/// Fragments rendered on their own number their placeholders from `$1`; spliced
/// behind `offset` values already bound in the surrounding statement, they have
/// to continue that numbering instead. Quoted literals, quoted identifiers,
/// comments and dollar-quoted strings are skipped, so a `$5` inside any of them
/// is left alone.
fn offset_postgres_placeholders(sql: &str, offset: usize) -> String {
    if offset == 0 {
        return sql.to_string();
    }

    let mut output = String::with_capacity(sql.len());
    for token in crate::internal::sql_lexer::tokens(sql) {
        if token.kind == crate::internal::sql_lexer::Kind::Parameter
            && let Ok(number) = token.text[1..].parse::<usize>()
            && number > 0
            && let Some(rebased) = number.checked_add(offset)
        {
            output.push('$');
            output.push_str(&rebased.to_string());
        } else {
            output.push_str(token.text);
        }
    }
    output
}

/// Replace template markers outside quoted text and comments.
pub(crate) fn map_template_placeholders(
    template: &str,
    mut next: impl FnMut() -> String,
) -> String {
    let mut rendered = String::with_capacity(template.len());
    for token in crate::internal::sql_lexer::tokens(template) {
        if token.text == "?" && token.kind == crate::internal::sql_lexer::Kind::Symbol {
            rendered.push_str(&next());
        } else {
            rendered.push_str(token.text);
        }
    }
    rendered
}

/// `template` with each `?` placeholder turned into the backend's marker,
/// numbered from `first`.
pub(crate) fn render_template(db_type: DatabaseType, template: &str, first: usize) -> String {
    let mut index = first;
    map_template_placeholders(template, || {
        let marker = placeholder(db_type, index);
        index += 1;
        marker
    })
}

/// How many parameters a template takes, without allocating rendered SQL.
pub(crate) fn count_template_placeholders(template: &str) -> usize {
    crate::internal::sql_lexer::tokens(template)
        .filter(|token| token.text == "?" && token.kind == crate::internal::sql_lexer::Kind::Symbol)
        .count()
}

#[cfg(test)]
mod lexical_regressions {
    use super::*;
    #[test]
    fn identifiers_and_nested_comments_are_not_parameters() {
        assert_eq!(
            offset_postgres_placeholders("a$1 + $1 /* outer /* $2 */ $3 */ + $2", 1),
            "a$1 + $2 /* outer /* $2 */ $3 */ + $3"
        );
        let template = "? /* ? /* ? */ ? */ + '?' + $$?$$ + \"?\" -- ?\n + ?";
        assert_eq!(count_template_placeholders(template), 2);
        assert_eq!(
            render_template(DatabaseType::Postgres, template, 1),
            "$1 /* ? /* ? */ ? */ + '?' + $$?$$ + \"?\" -- ?\n + $2"
        );
    }
}
