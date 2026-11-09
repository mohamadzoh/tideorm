//! Which result columns of a SQLite statement hold JSON documents.
//!
//! SQLite hands a JSON function's result over as text. Its JSON subtype
//! reaches the result row only when the call is computed in that row: a
//! sorter (`ORDER BY` another column), a view, a `FROM` subquery, a CTE, a
//! `UNION` or `RETURNING` drops it (measured on SQLite 3.51). So the
//! statement is read instead, lexically, for select items that are such a
//! call, and so are the definitions of the views it reads, which SQLite
//! treats as named subqueries.

use crate::internal::sql_lexer::{Kind, Token, sqlite_significant};

mod tokens;
mod views;

use tokens::{
    closing_parenthesis, column_list, ends_select_item, is_symbol, is_word, json_expression_end,
    top_level, unquoted,
};
use views::{Views, view_sources};

/// Whether each result column holds a JSON document, and whether that is
/// unknown because the column is read from a source the statement does not
/// describe, such as a view, whose definition can tell. Repeated aliases
/// match their own occurrences; unknown wildcard expansions may add columns,
/// so occurrences match by name only when the statement and the result agree
/// on how many there are. The catalog retains schema identity and lookup order.
pub(super) fn classify(sql: &str, names: &[&str], catalog: &[CatalogObject]) -> Vec<(bool, bool)> {
    use std::collections::{HashMap, VecDeque};

    // Read as SQLite reads it: its block comments do not nest, and it has no
    // escapes in strings nor dollar quotes.
    let tokens: Vec<Token<'_>> = sqlite_significant(sql).collect();
    let sources = view_sources(catalog);
    let views = Views {
        sources: &sources,
        schema: None,
    };
    let parsed = query_outputs(sql, &tokens, &[], views);
    if let Some(columns) = positional_columns(&parsed, names) {
        return columns;
    }
    let mut outputs: HashMap<String, VecDeque<(bool, bool)>> = HashMap::new();
    for output in parsed.into_iter().filter(|output| !output.wildcard) {
        outputs
            .entry(output.name)
            .or_default()
            .push_back((output.document, output.unresolved));
    }
    let mut counts = HashMap::new();
    for &name in names {
        *counts.entry(name).or_insert(0) += 1;
    }
    outputs.retain(|name, documents| counts.get(name.as_str()) == Some(&documents.len()));
    names
        .iter()
        .map(|name| {
            outputs
                .get_mut(*name)
                .and_then(VecDeque::pop_front)
                .unwrap_or((false, true))
        })
        .collect()
}

#[derive(Clone, PartialEq, Eq)]
struct Output {
    name: String,
    document: bool,
    /// A wildcard over a source whose columns are not known from the SQL.
    wildcard: bool,
    /// Read from a source whose columns are not known from the SQL, such as a
    /// view's: whether it holds a document is unknown.
    unresolved: bool,
}

impl Output {
    fn known(name: String, document: bool) -> Self {
        Self {
            name,
            document,
            wildcard: false,
            unresolved: false,
        }
    }
}

/// Align known select items around one unknown wildcard expansion. This
/// preserves an expression's position even if `table.*` repeats its alias.
fn positional_columns(outputs: &[Output], names: &[&str]) -> Option<Vec<(bool, bool)>> {
    let wildcards = outputs.iter().filter(|output| output.wildcard).count();
    let width = names.len().checked_sub(outputs.len() - wildcards)?;
    if wildcards > 1 || (wildcards == 0 && width != 0) {
        return None;
    }
    let mut columns = Vec::with_capacity(names.len());
    for output in outputs {
        if output.wildcard {
            columns.resize(columns.len() + width, (false, true));
        } else if names.get(columns.len()) == Some(&output.name.as_str()) {
            columns.push((output.document, output.unresolved));
        } else {
            return None;
        }
    }
    Some(columns)
}

/// Columns known to come from one derived table, CTE or view, under its local
/// name.
type Source = (String, Vec<Output>);

pub(super) struct CatalogObject {
    pub schema: String,
    pub name: String,
    /// None for a table, which can shadow a later schema's view.
    pub sql: Option<String>,
}

/// Resolve CTEs that may read each other into `sources`: each pass can
/// resolve one more link of a chain, and
/// a cycle stays unknown rather than taking an unrelated declaration's
/// columns. Names match in any case, as SQLite matches them (it refuses two
/// CTEs whose names differ only so).
fn resolve_definitions<D>(
    definitions: &[D],
    sources: &mut [Source],
    name: impl Fn(&D) -> &str,
    outputs: impl Fn(&D, &[Source]) -> Vec<Output>,
) {
    for _ in 0..definitions.len() {
        let mut changed = false;
        for definition in definitions {
            let resolved = outputs(definition, sources);
            let Some((_, previous)) = sources
                .iter_mut()
                .find(|(source, _)| source.eq_ignore_ascii_case(name(definition)))
            else {
                continue;
            };
            changed |= *previous != resolved;
            *previous = resolved;
        }
        if !changed {
            break;
        }
    }
}

/// `outputs` under the column names a CTE or view gives them, if any. Without
/// the table schema, a wildcard's width is unknown, so a later expression is
/// not bound to an earlier alias then.
fn named_outputs(mut outputs: Vec<Output>, names: &[String]) -> Vec<Output> {
    if names.is_empty() {
        return outputs;
    }
    if outputs.iter().any(|output| output.wildcard) {
        return names
            .iter()
            .map(|name| Output {
                unresolved: true,
                ..Output::known(name.clone(), false)
            })
            .collect();
    }
    for (output, name) in outputs.iter_mut().zip(names) {
        output.name.clone_from(name);
    }
    outputs
}

fn query_outputs(
    sql: &str,
    tokens: &[Token<'_>],
    inherited: &[Source],
    views: Views<'_>,
) -> Vec<Output> {
    let mut ctes = inherited.to_vec();
    let Some((definitions, at)) = with_clause(tokens, &mut ctes) else {
        return Vec::new();
    };
    // SQLite permits a CTE to refer to a later definition: the whole local
    // scope resolves together, without a same-named CTE of an outer scope.
    resolve_definitions(
        &definitions,
        &mut ctes,
        |(name, ..)| name,
        |(_, names, body), ctes| named_outputs(query_outputs(sql, body, ctes, views), names),
    );
    if !is_word(tokens, at, &["select"]) {
        return Vec::new();
    }
    select_outputs(sql, &tokens[at + 1..], &ctes, views)
}

/// A CTE: its name, the column names it gives, and its query.
type Definition<'t, 'a> = (String, Vec<String>, &'t [Token<'a>]);

/// The CTEs a `WITH` clause opening `tokens` defines, put in `ctes` in place
/// of an outer scope's of the same name, and where the statement goes on;
/// `None` when the clause cannot be read.
fn with_clause<'t, 'a>(
    tokens: &'t [Token<'a>],
    ctes: &mut Vec<Source>,
) -> Option<(Vec<Definition<'t, 'a>>, usize)> {
    let mut definitions = Vec::new();
    if !is_word(tokens, 0, &["with"]) {
        return Some((definitions, 0));
    }
    let mut at = 1 + usize::from(is_word(tokens, 1, &["recursive"]));
    loop {
        let name = unquoted(tokens.get(at)?);
        at += 1;
        let names;
        (names, at) = column_list(tokens, at)?;
        if !is_word(tokens, at, &["as"]) {
            return None;
        }
        at += 1;
        while is_word(tokens, at, &["not", "materialized"]) {
            at += 1;
        }
        if !is_symbol(tokens, at, &["("]) {
            return None;
        }
        let close = closing_parenthesis(tokens, at)?;
        ctes.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        ctes.push((name.clone(), Vec::new()));
        definitions.push((name, names, &tokens[at + 1..close]));
        at = close + 1;
        if !is_symbol(tokens, at, &[","]) {
            return Some((definitions, at));
        }
        at += 1;
    }
}

/// The columns of the `SELECT` whose list `tokens` holds, after an optional
/// `DISTINCT` or `ALL`, including what the arms of a `UNION` add.
fn select_outputs(
    sql: &str,
    tokens: &[Token<'_>],
    ctes: &[Source],
    views: Views<'_>,
) -> Vec<Output> {
    let tokens = &tokens[usize::from(is_word(tokens, 0, &["distinct", "all"]))..];
    let end = top_level(tokens)
        .find(|&at| !is_symbol(tokens, at, &[",", "("]) && ends_select_item(tokens, at))
        .unwrap_or(tokens.len());
    let sources = query_sources(sql, &tokens[end..], ctes, views);
    let mut outputs = Vec::new();
    let mut start = 0;
    for end in top_level(&tokens[..end])
        .filter(|&at| is_symbol(tokens, at, &[","]))
        .chain(std::iter::once(end))
    {
        outputs.extend(select_item(sql, &tokens[start..end], &sources, ctes, views));
        start = end + 1;
    }
    // UNION adds rows from its right arm; INTERSECT and EXCEPT only filter
    // existing rows. Decode as JSON only if every contributing arm does.
    let compounds: Vec<_> = top_level(tokens)
        .filter(|&at| is_word(tokens, at, &["union", "intersect", "except"]))
        .collect();
    for (position, &compound) in compounds.iter().enumerate() {
        if !is_word(tokens, compound, &["union"]) {
            continue;
        }
        let next = compound + 1 + usize::from(is_word(tokens, compound + 1, &["all", "distinct"]));
        let end = compounds.get(position + 1).copied().unwrap_or(tokens.len());
        let other = query_outputs(sql, &tokens[next..end], ctes, views);
        let unknown_width = outputs.iter().chain(&other).any(|output| output.wildcard);
        for (index, output) in outputs.iter_mut().enumerate() {
            let arm = other.get(index);
            output.document &= !unknown_width && arm.is_some_and(|output| output.document);
            output.unresolved |= unknown_width || arm.is_none_or(|output| output.unresolved);
        }
    }
    outputs
}

fn query_sources(
    sql: &str,
    tokens: &[Token<'_>],
    ctes: &[Source],
    views: Views<'_>,
) -> Vec<Source> {
    let mut sources = Vec::new();
    let mut expect_source = false;
    let mut skip_until = 0;
    for at in top_level(tokens) {
        if at < skip_until {
            continue;
        }
        if (at == 0 && is_word(tokens, at, &["from"]))
            || is_word(tokens, at, &["join"])
            || is_symbol(tokens, at, &[","])
        {
            expect_source = true;
            continue;
        }
        if ends_select_item(tokens, at) {
            break;
        }
        if !expect_source {
            continue;
        }
        expect_source = false;
        let (mut name, outputs, mut end) = if is_symbol(tokens, at, &["("]) {
            let Some(close) = closing_parenthesis(tokens, at) else {
                break;
            };
            (
                String::new(),
                query_outputs(sql, &tokens[at + 1..close], ctes, views),
                close + 1,
            )
        } else {
            let name = unquoted(&tokens[at]);
            // A schema-qualified name is a table or view, never a CTE.
            if is_symbol(tokens, at + 1, &["."]) {
                let schema = name;
                let name = tokens.get(at + 2).map(unquoted).unwrap_or_default();
                let outputs = views.lookup(Some(&schema), &name).unwrap_or_default();
                (name, outputs, at + 3)
            } else {
                let outputs = ctes
                    .iter()
                    .rev()
                    .find(|(cte, _)| cte.eq_ignore_ascii_case(&name))
                    .map(|(_, outputs)| outputs.clone())
                    .or_else(|| views.lookup(None, &name))
                    .unwrap_or_default();
                (name, outputs, at + 1)
            }
        };
        if is_word(tokens, end, &["as"]) {
            end += 1;
            if let Some(alias) = tokens.get(end) {
                name = unquoted(alias);
                end += 1;
            }
        } else if tokens
            .get(end)
            .is_some_and(|token| matches!(token.kind, Kind::Word | Kind::Identifier))
            && !ends_select_item(tokens, end)
            && !is_word(
                tokens,
                end,
                &[
                    "join", "on", "using", "left", "right", "inner", "outer", "cross", "full",
                    "natural", "indexed", "not",
                ],
            )
        {
            name = unquoted(&tokens[end]);
            end += 1;
        }
        sources.push((name, outputs));
        skip_until = end;
    }
    sources
}

fn select_item(
    sql: &str,
    tokens: &[Token<'_>],
    sources: &[Source],
    ctes: &[Source],
    views: Views<'_>,
) -> Vec<Output> {
    let Some(last) = tokens.last() else {
        return Vec::new();
    };
    let expression_end = json_expression_end(tokens, 0);
    let (expression, alias) = if tokens.len() >= 2 && is_word(tokens, tokens.len() - 2, &["as"]) {
        (&tokens[..tokens.len() - 2], Some(unquoted(last)))
    } else if tokens.len() >= 2
        && expression_end != Some(tokens.len())
        && matches!(last.kind, Kind::Word | Kind::Identifier | Kind::Literal)
        && (expression_end == Some(tokens.len() - 1)
            || !is_symbol(tokens, tokens.len() - 2, &["."]))
    {
        (&tokens[..tokens.len() - 1], Some(unquoted(last)))
    } else {
        (tokens, None)
    };
    let reference = expression.iter().enumerate().all(|(index, token)| {
        if index % 2 == 0 {
            matches!(token.kind, Kind::Word | Kind::Identifier) || token.text == "*"
        } else {
            token.kind == Kind::Symbol && token.text == "."
        }
    }) && expression.len() % 2 == 1;
    if reference {
        let column = unquoted(expression.last().unwrap());
        let qualifier = (expression.len() > 1).then(|| unquoted(&expression[expression.len() - 3]));
        let matching_sources: Vec<&Source> = sources
            .iter()
            .filter(|(name, _)| {
                qualifier
                    .as_ref()
                    .is_none_or(|qualifier| name.eq_ignore_ascii_case(qualifier))
            })
            .collect();
        if column == "*" {
            return matching_sources
                .into_iter()
                .flat_map(|(_, outputs)| {
                    if outputs.is_empty() {
                        vec![Output {
                            wildcard: true,
                            unresolved: true,
                            ..Output::known(String::new(), false)
                        }]
                    } else {
                        outputs.clone()
                    }
                })
                .collect();
        }
        let matching: Vec<&Output> = matching_sources
            .iter()
            .flat_map(|(_, outputs)| outputs)
            .filter(|output| !output.wildcard && output.name.eq_ignore_ascii_case(&column))
            .collect();
        let unresolved = match matching.as_slice() {
            [output] => output.unresolved,
            _ => matching_sources.iter().any(|(_, outputs)| {
                outputs.is_empty() || outputs.iter().any(|output| output.wildcard)
            }),
        };
        return vec![Output {
            unresolved,
            ..Output::known(
                alias.unwrap_or_else(|| {
                    if matching.len() == 1 {
                        matching[0].name.clone()
                    } else {
                        column
                    }
                }),
                matching.len() == 1 && matching[0].document,
            )
        }];
    }
    let name =
        alias.unwrap_or_else(|| sql[tokens[0].start..last.start + last.text.len()].to_string());
    if json_expression_end(expression, 0) == Some(expression.len()) {
        return vec![Output::known(name, true)];
    }
    // A parenthesized subquery is the one column it selects.
    if is_symbol(expression, 0, &["("])
        && closing_parenthesis(expression, 0) == expression.len().checked_sub(1)
    {
        let nested = query_outputs(sql, &expression[1..expression.len() - 1], ctes, views);
        if let [output] = nested.as_slice() {
            return vec![Output {
                unresolved: output.unresolved,
                ..Output::known(name, output.document)
            }];
        }
    }
    vec![Output::known(name, false)]
}

#[cfg(test)]
#[path = "../../../tests/unit/sqlite_json_outputs_tests.rs"]
mod tests;
