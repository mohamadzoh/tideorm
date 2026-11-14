//! The model rules the derive checks from the model's own declaration: its
//! counts, names, keys and flags. Each broken one is a compile error at what
//! breaks it. The rules that need a field's type or another model are the
//! `const` items `entity_gen::rules` writes.

use proc_macro2::Span;

use crate::context::BuildContext;
use crate::meta_support::has_managed_timestamp_columns;
use crate::parse::{IndexDef, ModelField, is_optional_type};

/// Every model rule `ctx`'s model breaks that its declaration shows.
pub(crate) fn broken_rules(ctx: &BuildContext) -> Vec<syn::Error> {
    let limits = &ctx.project_rules.limits;
    let name = &ctx.struct_name_str;
    let model = ctx.struct_name.span();
    let mut broken = Vec::new();
    let mut fail = |span: Span, rule: &str, problem: String| {
        broken.push(syn::Error::new(
            span,
            format!("{problem} ({rule} in tideorm.toml)"),
        ));
    };

    let nullable = ctx
        .db_fields
        .iter()
        .filter(|field| field.nullable || is_optional_type(&field.ty))
        .count();
    let counts = [
        (
            "max_columns",
            "columns",
            ctx.db_fields.len(),
            limits.max_columns,
        ),
        (
            "max_relations",
            "relations",
            ctx.relation_fields.len(),
            limits.max_relations,
        ),
        (
            "max_nullable_columns",
            "nullable columns",
            nullable,
            limits.max_nullable_columns,
        ),
        (
            "max_indexes",
            "indexes",
            ctx.indexes.len(),
            limits.max_indexes,
        ),
    ];
    for (rule, what, count, max) in counts {
        if let Some(max) = max.filter(|max| count > *max as usize) {
            fail(
                model,
                rule,
                format!("{name} has {count} {what}, over the limit of {max}"),
            );
        }
    }

    let table = ctx.table_name.rsplit('.').next().unwrap_or(&ctx.table_name);
    let mut names = vec![("table", table.to_string(), model)];
    names.extend(
        ctx.db_fields
            .iter()
            .map(|field| ("column", field.column_name(), field.ident().span())),
    );
    if let Some(max) = limits.max_identifier_length {
        let indexes = ctx
            .indexes
            .iter()
            .map(|index| ("index", index_name(ctx, index), index.span));
        for (kind, identifier, span) in names.iter().cloned().chain(indexes) {
            if identifier.len() > max as usize {
                fail(
                    span,
                    "max_identifier_length",
                    format!(
                        "{name}'s {kind} name `{identifier}` is {} bytes, over the limit of {max}",
                        identifier.len()
                    ),
                );
            }
        }
    }
    for (kind, identifier, span) in &names {
        if limits.snake_case_names && !is_snake_case(identifier) {
            fail(
                *span,
                "snake_case_names",
                format!("{name}'s {kind} name `{identifier}` is not lowercase snake_case"),
            );
        }
        if limits.avoid_reserved_words && is_reserved(identifier) {
            fail(
                *span,
                "avoid_reserved_words",
                format!("{name}'s {kind} name `{identifier}` is an SQL reserved word"),
            );
        }
    }

    if limits.forbid_composite_primary_keys && ctx.pk_idents.len() > 1 {
        fail(
            model,
            "forbid_composite_primary_keys",
            format!("{name}'s primary key spans {} columns", ctx.pk_idents.len()),
        );
    }
    // What `ModelMeta::has_timestamps` answers.
    let timestamps = ctx.timestamps_enabled || has_managed_timestamp_columns(&ctx.db_fields);
    if limits.require_timestamps && !timestamps {
        fail(
            model,
            "require_timestamps",
            format!("{name} has no created_at/updated_at timestamps"),
        );
    }
    if limits.require_soft_delete && ctx.soft_delete.is_none() {
        fail(
            model,
            "require_soft_delete",
            format!("{name} does not soft delete"),
        );
    }
    for field in &ctx.db_fields {
        let column = field.column_name();
        let lower = column.to_lowercase();
        let sensitive = limits
            .sensitive_fields
            .iter()
            .any(|word| lower.contains(word.as_str()));
        if sensitive && !ctx.hidden_attrs.contains(&field.name()) && !ctx.is_encrypted(field) {
            fail(
                field.ident().span(),
                "sensitive_fields",
                format!(
                    "{name}'s column `{column}` looks sensitive and is neither hidden nor encrypted"
                ),
            );
        }
    }
    broken
}

/// The name the index gets: its own, else the one
/// `IndexDefinition::__generated` gives it.
fn index_name(ctx: &BuildContext, index: &IndexDef) -> String {
    if let Some(name) = &index.name {
        return name.clone();
    }
    let columns: Vec<String> = index
        .resolved_columns(&ctx.db_fields)
        .map(|(column, field)| field.map_or_else(|| column.clone(), ModelField::column_name))
        .collect();
    let suffix = if index.unique { "_unique" } else { "" };
    bounded_index_name(format!(
        "idx_{}_{}{}",
        ctx.table_name,
        columns.join("_"),
        suffix
    ))
}

/// `tideorm::migration::ddl::bounded_index_name`: a name past 63 bytes cut
/// and ended with its FNV-1a hash.
fn bounded_index_name(name: String) -> String {
    const MAX_IDENTIFIER_BYTES: usize = 63;
    if name.len() <= MAX_IDENTIFIER_BYTES {
        return name;
    }
    let hash = name.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    let mut cut = MAX_IDENTIFIER_BYTES - 9;
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}_{:08x}", &name[..cut], hash)
}

fn is_snake_case(identifier: &str) -> bool {
    let mut chars = identifier.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !identifier.contains("__")
        && !identifier.ends_with('_')
}

/// Words PostgreSQL, MySQL or SQLite reserve, which a table or column named by
/// one makes every raw statement quote.
const RESERVED: &[&str] = &[
    "add",
    "all",
    "alter",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "between",
    "both",
    "by",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "cross",
    "current_date",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "except",
    "exists",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "in",
    "index",
    "inner",
    "insert",
    "intersect",
    "interval",
    "into",
    "is",
    "join",
    "key",
    "keys",
    "leading",
    "left",
    "like",
    "limit",
    "natural",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "range",
    "references",
    "rename",
    "returning",
    "right",
    "row",
    "rows",
    "select",
    "session_user",
    "set",
    "some",
    "table",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "update",
    "user",
    "using",
    "values",
    "when",
    "where",
    "window",
    "with",
];

fn is_reserved(identifier: &str) -> bool {
    RESERVED.contains(&identifier.to_lowercase().as_str())
}

#[cfg(test)]
#[path = "../../tests/unit/rule_checks_tests.rs"]
mod tests;
