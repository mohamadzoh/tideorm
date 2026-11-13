//! The project rules a team writes in `tideorm.toml`, read as a model is
//! derived. `[rules]` sets the limits; `[rules.allow]` exempts models, by
//! struct name, from the rules it lists for them. Without the file, or without
//! a `[rules]` table in it, there are no rules.
//!
//! The file is the first `tideorm.toml` found walking up from the crate's
//! manifest directory; a dependency fetched from crates.io, git or a vendor
//! directory reads none. Its rules are written into each model's code, so the
//! runtime enforces them without reading any file.

use std::path::{Path, PathBuf};

use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};

/// How a rule's value is written.
#[derive(Clone, Copy)]
enum Shape {
    /// A count, `0` or more.
    Count,
    /// A number of rows, at least 1.
    Rows,
    /// `true` to apply the rule; `false` is the same as leaving it out.
    Flag,
    /// A list of the primary key types in [`KEY_TYPES`].
    KeyTypes,
    /// A list of words, matched in any case.
    Words,
}

/// Every rule, as `tideorm.toml` names it.
const RULES: &[(&str, Shape)] = &[
    ("max_columns", Shape::Count),
    ("max_relations", Shape::Count),
    ("max_json_columns", Shape::Count),
    ("max_nullable_columns", Shape::Count),
    ("max_indexes", Shape::Count),
    ("max_identifier_length", Shape::Count),
    ("snake_case_names", Shape::Flag),
    ("avoid_reserved_words", Shape::Flag),
    ("primary_key_types", Shape::KeyTypes),
    ("forbid_composite_primary_keys", Shape::Flag),
    ("require_timestamps", Shape::Flag),
    ("require_soft_delete", Shape::Flag),
    ("require_foreign_key_index", Shape::Flag),
    ("sensitive_fields", Shape::Words),
    ("forbid_float_columns", Shape::Flag),
    ("max_relation_depth", Shape::Count),
    ("forbid_relation_cycles", Shape::Flag),
    ("max_eager_depth", Shape::Count),
    ("max_eager_relations", Shape::Count),
    ("max_rows", Shape::Rows),
    ("max_joins", Shape::Count),
    ("max_in_values", Shape::Count),
    ("forbid_raw_sql", Shape::Flag),
];

/// The names `primary_key_types` takes, as the runtime names a key column's
/// type (`tideorm::rules`).
const KEY_TYPES: &[&str] = &["integer", "bigint", "uuid", "string"];

/// What a model's derive writes for the project rules.
#[derive(Default)]
pub(crate) struct ModelRules {
    /// The `tideorm::rules::Rules` the model is held to; `None` when no rule
    /// applies to it.
    pub(crate) rules: Option<TokenStream2>,
    /// Whether the project forbids raw SQL, whatever this model's exceptions:
    /// what a raw statement that names no model is held to.
    pub(crate) forbids_raw_sql: bool,
    /// The rules file, which the build tracks so an edit recompiles models.
    pub(crate) file: Option<String>,
}

/// The rules `struct_name` is held to, from the crate's `tideorm.toml`.
pub(crate) fn model_rules(struct_name: &str) -> syn::Result<ModelRules> {
    let Some(path) = rules_file() else {
        return Ok(ModelRules::default());
    };
    let error = |message: String| {
        syn::Error::new(Span::call_site(), format!("{}: {message}", path.display()))
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|failure| error(format!("cannot be read: {failure}")))?;
    let mut rules = parse_rules(&text, struct_name).map_err(error)?;
    rules.file = Some(path.display().to_string());
    Ok(rules)
}

/// The first `tideorm.toml` in the crate's directory or one above it; none
/// for a dependency Cargo fetched, whose models are not the project's.
fn rules_file() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    if fetched_by_cargo(&manifest_dir) {
        return None;
    }
    manifest_dir
        .ancestors()
        .map(|dir| dir.join("tideorm.toml"))
        .find(|path| path.is_file())
}

/// Whether `dir` is a crate Cargo fetched: crates.io and git sources sit in
/// its `registry/src` and `git/checkouts`, and `cargo vendor` leaves a
/// `.cargo-checksum.json` in each copy.
fn fetched_by_cargo(dir: &Path) -> bool {
    let names: Vec<_> = dir.components().map(|part| part.as_os_str()).collect();
    names.windows(2).any(|pair| {
        (pair[0] == "registry" && pair[1] == "src") || (pair[0] == "git" && pair[1] == "checkouts")
    }) || dir.join(".cargo-checksum.json").is_file()
}

/// The rules `text`, a whole `tideorm.toml`, sets for `struct_name`. Every
/// rule and exception is checked, not only the ones for this model, so each
/// model reports a mistake the same way.
fn parse_rules(text: &str, struct_name: &str) -> Result<ModelRules, String> {
    let file: toml::Table = text
        .parse()
        .map_err(|failure| format!("is not valid TOML: {failure}"))?;
    let mut rules = ModelRules::default();
    let Some(section) = file.get("rules") else {
        return Ok(rules);
    };
    let section = section
        .as_table()
        .ok_or("`rules` must be a table: write it as `[rules]`")?;
    let exceptions = exceptions_for(section, struct_name)?;

    let mut fields = Vec::new();
    for (name, value) in section {
        if name == "allow" {
            continue;
        }
        let shape = shape_of(name)?;
        let field = rule_value(shape, value).map_err(|message| format!("`{name}` {message}"))?;
        if name == "forbid_raw_sql" {
            rules.forbids_raw_sql = field.is_some();
        }
        if let Some(field) = field.filter(|_| !exceptions.contains(name)) {
            let ident = format_ident!("{}", name);
            fields.push(quote!(#ident: #field));
        }
    }
    if !fields.is_empty() {
        rules.rules = Some(quote! {
            ::tideorm::rules::Rules { #(#fields,)* ..::tideorm::rules::Rules::NONE }
        });
    }
    Ok(rules)
}

/// The rules `[rules.allow]` exempts `struct_name` from, after checking that
/// every entry names known rules.
fn exceptions_for(section: &toml::Table, struct_name: &str) -> Result<Vec<String>, String> {
    let Some(allow) = section.get("allow") else {
        return Ok(Vec::new());
    };
    let allow = allow
        .as_table()
        .ok_or("`rules.allow` must be a table of model names, each a list of rule names")?;
    let mut exceptions = Vec::new();
    for (model, names) in allow {
        let names = names.as_array().ok_or_else(|| {
            format!("`rules.allow.{model}` must be a list of rule names, like [\"max_columns\"]")
        })?;
        for name in names {
            let name = name
                .as_str()
                .ok_or_else(|| format!("`rules.allow.{model}` must hold rule names as strings"))?;
            shape_of(name).map_err(|message| format!("`rules.allow.{model}` {message}"))?;
            if model == struct_name {
                exceptions.push(name.to_string());
            }
        }
    }
    Ok(exceptions)
}

fn shape_of(name: &str) -> Result<Shape, String> {
    RULES
        .iter()
        .find(|(rule, _)| *rule == name)
        .map(|(_, shape)| *shape)
        .ok_or_else(|| {
            let known: Vec<_> = RULES.iter().map(|(rule, _)| *rule).collect();
            format!("names no rule `{name}`; the rules are {}", known.join(", "))
        })
}

/// The field value a rule's setting writes, `None` when the setting applies
/// nothing (a `false` flag).
fn rule_value(shape: Shape, value: &toml::Value) -> Result<Option<TokenStream2>, String> {
    match shape {
        Shape::Count => {
            let count = value
                .as_integer()
                .and_then(|count| u32::try_from(count).ok())
                .ok_or("must be a whole number, 0 or more")?;
            let count = Literal::u32_suffixed(count);
            Ok(Some(quote!(::core::option::Option::Some(#count))))
        }
        Shape::Rows => {
            let rows = value
                .as_integer()
                .and_then(|rows| u64::try_from(rows).ok())
                .filter(|rows| *rows > 0)
                .ok_or("must be a whole number, 1 or more")?;
            let rows = Literal::u64_suffixed(rows);
            Ok(Some(quote!(::core::option::Option::Some(#rows))))
        }
        Shape::Flag => {
            let flag = value.as_bool().ok_or("must be true or false")?;
            Ok(flag.then(|| quote!(true)))
        }
        Shape::KeyTypes | Shape::Words => {
            let items = value
                .as_array()
                .filter(|items| !items.is_empty())
                .ok_or("must be a list of one or more strings")?;
            let mut words = Vec::new();
            for item in items {
                let word = item
                    .as_str()
                    .map(str::to_lowercase)
                    .filter(|word| !word.is_empty())
                    .ok_or("must hold non-empty strings")?;
                if matches!(shape, Shape::KeyTypes) && !KEY_TYPES.contains(&word.as_str()) {
                    return Err(format!(
                        "names no key type `{word}`; the types are {}",
                        KEY_TYPES.join(", ")
                    ));
                }
                words.push(word);
            }
            Ok(Some(quote!(&[#(#words),*])))
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/rules_tests.rs"]
mod tests;
