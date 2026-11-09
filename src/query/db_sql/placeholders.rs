use crate::config::DatabaseType;
use crate::internal::Value;
use crate::internal::sql_lexer::{Kind, Token, tokens};
use crate::orm::sql_builder::BoundSql;

/// Render bound values for display, preserving the SQL when substitution is
/// invalid. The markers are read under the backend's own lexical rules, so a
/// backslash ends a literal where that backend says it does.
pub(crate) fn inline_parameters(db_type: DatabaseType, sql: &str, params: &[Value]) -> String {
    db_type
        .dialect()
        .inline_values(sql, params)
        .unwrap_or_else(|_| sql.to_string())
}

/// Whether `token` is a template's `?` placeholder: quoted text, quoted
/// identifiers and comments hide one.
fn is_placeholder(token: &Token<'_>) -> bool {
    token.text == "?" && token.kind == Kind::Symbol
}

/// A caller's trusted `template` with each `?` placeholder binding the next
/// of `values`, so it follows the statement to whichever backend renders it.
/// Without values it is written as given: reading its placeholders would
/// refuse a `?` meant as an operator. Validation has matched the count.
pub(crate) fn template_bound(template: &str, values: Vec<Value>) -> BoundSql {
    if values.is_empty() {
        return BoundSql::sql(template);
    }
    let mut values = values.into_iter();
    let mut bound = BoundSql::default();
    for token in tokens(template) {
        if is_placeholder(&token)
            && let Some(value) = values.next()
        {
            bound.push(value);
        } else {
            bound.push(token.text);
        }
    }
    bound
}

/// How many parameters a template takes, without allocating rendered SQL.
pub(crate) fn count_template_placeholders(template: &str) -> usize {
    tokens(template).filter(is_placeholder).count()
}
