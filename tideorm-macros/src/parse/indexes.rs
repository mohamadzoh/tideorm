use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, Lit, LitStr, Meta, Token};

use super::unraw_ident;

#[derive(Debug, Clone)]
pub(crate) struct IndexDef {
    pub(crate) name: Option<String>,
    pub(crate) columns: Vec<String>,
    pub(crate) unique: bool,
    /// Span of the declaring attribute, used to point diagnostics at the index.
    pub(crate) span: Span,
    /// Parse failure for a malformed attribute, surfaced by `BuildContext::new`.
    pub(crate) error: Option<syn::Error>,
}

impl IndexDef {
    fn new(name: Option<String>, columns: Vec<String>, unique: bool, span: Span) -> Self {
        Self {
            name,
            columns,
            unique,
            span,
            error: None,
        }
    }

    fn invalid(unique: bool, span: Span, error: syn::Error) -> Self {
        Self {
            name: None,
            columns: Vec::new(),
            unique,
            span,
            error: Some(error),
        }
    }

    /// The attribute spelling that declared this index, used in diagnostics.
    pub(crate) fn attribute_name(&self) -> &'static str {
        attribute_name(self.unique)
    }

    pub(crate) fn get_name(&self, table_name: &str) -> String {
        self.name.clone().unwrap_or_else(|| {
            let prefix = if self.unique { "uidx" } else { "idx" };
            format!("{}_{}_{}", prefix, table_name, self.columns.join("_"))
        })
    }
}

pub(crate) fn parse_index_attributes(attrs: &[Attribute]) -> (Vec<IndexDef>, Vec<IndexDef>) {
    let mut indexes = Vec::new();
    let mut unique_indexes = Vec::new();

    for attr in attrs {
        let is_index = attr.path().is_ident("index");
        let is_unique_index = attr.path().is_ident("unique_index");
        if !is_index && !is_unique_index {
            continue;
        }

        let unique = is_unique_index;
        let span = attr.span();
        let parsed = parse_index_attribute(attr, unique)
            .unwrap_or_else(|error| IndexDef::invalid(unique, span, error));

        if unique {
            unique_indexes.push(parsed);
        } else {
            indexes.push(parsed);
        }
    }

    (indexes, unique_indexes)
}

fn parse_index_attribute(attr: &Attribute, unique: bool) -> syn::Result<IndexDef> {
    let span = attr.span();
    let attribute = attribute_name(unique);

    if !matches!(&attr.meta, Meta::List(_)) {
        return Err(syn::Error::new_spanned(attr, usage_error(attribute)));
    }

    // `#[index("col")]` / `#[index("col_a,col_b")]` / `#[index("col_a", "col_b")]`
    if let Ok(literals) = attr.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated) {
        if literals.is_empty() {
            return Err(syn::Error::new(span, usage_error(attribute)));
        }

        let mut columns = Vec::new();
        for literal in &literals {
            let parsed = parse_column_list(&literal.value(), literal.span(), attribute)?;
            columns.extend(parsed);
        }

        return Ok(IndexDef::new(None, columns, unique, span));
    }

    let entries = match attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
        Ok(entries) => entries,
        Err(mut error) => {
            error.combine(syn::Error::new(span, usage_error(attribute)));
            return Err(error);
        }
    };

    if entries.is_empty() {
        return Err(syn::Error::new(span, usage_error(attribute)));
    }

    // `#[index(col_a, col_b)]` — bare identifiers are treated as column names.
    if entries.iter().all(|entry| matches!(entry, Meta::Path(_))) {
        let mut columns = Vec::new();
        for entry in &entries {
            if let Meta::Path(path) = entry {
                let ident = path
                    .get_ident()
                    .ok_or_else(|| syn::Error::new_spanned(path, usage_error(attribute)))?;
                columns.push(unraw_ident(ident));
            }
        }

        return Ok(IndexDef::new(None, columns, unique, span));
    }

    // `#[index(name = "idx_name", columns = "col_a,col_b")]`
    let mut name: Option<String> = None;
    let mut columns: Option<LitStr> = None;
    for entry in &entries {
        let Meta::NameValue(pair) = entry else {
            return Err(syn::Error::new_spanned(entry, usage_error(attribute)));
        };

        let key = match pair.path.get_ident() {
            Some(ident) => unraw_ident(ident),
            None => return Err(syn::Error::new_spanned(&pair.path, usage_error(attribute))),
        };
        match key.as_str() {
            "name" => {
                if name.is_some() {
                    return Err(syn::Error::new_spanned(
                        &pair.path,
                        format!("#[{attribute}(..)] option 'name' is declared twice"),
                    ));
                }
                let literal = literal_string(&pair.value, attribute, "name")?;
                if literal.value().trim().is_empty() {
                    return Err(syn::Error::new_spanned(
                        &literal,
                        format!("#[{attribute}(..)] option 'name' must not be empty"),
                    ));
                }
                name = Some(literal.value());
            }
            "columns" => {
                if columns.is_some() {
                    return Err(syn::Error::new_spanned(
                        &pair.path,
                        format!("#[{attribute}(..)] option 'columns' is declared twice"),
                    ));
                }
                columns = Some(literal_string(&pair.value, attribute, "columns")?);
            }
            unknown => {
                let message = format!(
                    "unknown #[{attribute}(..)] option '{unknown}'; expected 'name' or 'columns'"
                );
                return Err(syn::Error::new_spanned(&pair.path, message));
            }
        }
    }

    let Some(columns) = columns else {
        let message = format!(
            "#[{attribute}(..)] requires a 'columns' option; {}",
            usage_error(attribute)
        );
        return Err(syn::Error::new(span, message));
    };

    let parsed_columns = parse_column_list(&columns.value(), columns.span(), attribute)?;
    Ok(IndexDef::new(name, parsed_columns, unique, span))
}

fn literal_string(expr: &Expr, attribute: &str, key: &str) -> syn::Result<LitStr> {
    let value = match expr {
        Expr::Lit(literal) => match &literal.lit {
            Lit::Str(value) => Some(value.clone()),
            _ => None,
        },
        _ => None,
    };

    value.ok_or_else(|| {
        let message = format!("#[{attribute}(..)] '{key}' must be a string literal");
        syn::Error::new_spanned(expr, message)
    })
}

fn parse_column_list(value: &str, span: Span, attribute: &str) -> syn::Result<Vec<String>> {
    let columns: Vec<String> = value
        .split(',')
        .map(|part| part.trim().to_string())
        .collect();
    if columns.iter().any(String::is_empty) {
        return Err(syn::Error::new(
            span,
            format!("#[{attribute}(..)] contains an empty column name in \"{value}\""),
        ));
    }

    Ok(columns)
}

fn attribute_name(unique: bool) -> &'static str {
    if unique { "unique_index" } else { "index" }
}

fn usage_error(attribute: &str) -> String {
    format!(
        "expected #[{attribute}(\"column\")], #[{attribute}(\"col_a,col_b\")] or #[{attribute}(name = \"idx_name\", columns = \"col_a,col_b\")]"
    )
}
