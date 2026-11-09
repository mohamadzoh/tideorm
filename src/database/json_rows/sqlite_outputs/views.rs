//! The views a statement reads, resolved from their `CREATE VIEW`
//! definitions in the catalog.

use crate::internal::sql_lexer::{Token, sqlite_significant};

use super::tokens::{column_list, is_symbol, is_word};
use super::{CatalogObject, Output, named_outputs, query_outputs};

pub(super) struct ViewSource<'a> {
    object: &'a CatalogObject,
    outputs: Vec<Output>,
}

#[derive(Clone, Copy)]
pub(super) struct Views<'a> {
    pub(super) sources: &'a [ViewSource<'a>],
    /// Persistent views resolve unqualified names in their own schema. TEMP
    /// views and top-level queries use the connection's normal lookup order.
    pub(super) schema: Option<&'a str>,
}

impl Views<'_> {
    pub(super) fn lookup(self, schema: Option<&str>, name: &str) -> Option<Vec<Output>> {
        let schema = schema.or(self.schema);
        self.sources
            .iter()
            .find(|source| {
                source.object.name.eq_ignore_ascii_case(name)
                    && schema.is_none_or(|schema| source.object.schema.eq_ignore_ascii_case(schema))
            })
            .map(|source| source.outputs.clone())
    }
}

/// Resolve dependent views by their catalog identity, never by bare name.
pub(super) fn view_sources(catalog: &[CatalogObject]) -> Vec<ViewSource<'_>> {
    let lexed: Vec<Vec<Token<'_>>> = catalog
        .iter()
        .map(|object| sqlite_significant(object.sql.as_deref().unwrap_or("")).collect())
        .collect();
    let mut sources: Vec<_> = catalog
        .iter()
        .map(|object| ViewSource {
            object,
            outputs: Vec::new(),
        })
        .collect();
    for _ in 0..catalog.len() {
        let mut changed = false;
        for (index, (object, tokens)) in catalog.iter().zip(&lexed).enumerate() {
            let Some((names, body)) = view_definition(tokens) else {
                continue;
            };
            let views = Views {
                sources: &sources,
                schema: (!object.schema.eq_ignore_ascii_case("temp"))
                    .then_some(object.schema.as_str()),
            };
            let outputs = named_outputs(
                query_outputs(object.sql.as_deref().unwrap_or(""), body, &[], views),
                &names,
            );
            changed |= sources[index].outputs != outputs;
            sources[index].outputs = outputs;
        }
        if !changed {
            break;
        }
    }
    sources
}

/// The column names and the query of a `CREATE [TEMP] VIEW [IF NOT EXISTS]
/// [schema.]name [(columns)] AS query` statement.
fn view_definition<'t, 'a>(tokens: &'t [Token<'a>]) -> Option<(Vec<String>, &'t [Token<'a>])> {
    let mut at = usize::from(is_word(tokens, 0, &["create"]));
    at += usize::from(is_word(tokens, at, &["temp", "temporary"]));
    if !is_word(tokens, at, &["view"]) {
        return None;
    }
    at += 1;
    if is_word(tokens, at, &["if"]) {
        at += 3;
    }
    at += if is_symbol(tokens, at + 1, &["."]) {
        3
    } else {
        1
    };
    let (names, at) = column_list(tokens, at)?;
    is_word(tokens, at, &["as"]).then(|| (names, &tokens[at + 1..]))
}
