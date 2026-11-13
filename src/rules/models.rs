//! The rules one model's own shape is checked against: its columns, keys and
//! indexes, as the registration declares them.

use super::{ModelOutline, short_name};
use crate::orm::ColumnType;

/// Every rule `model` breaks on its own, one line each.
pub(super) fn broken_rules(model: &ModelOutline) -> Vec<String> {
    let rules = &model.rules;
    let name = short_name(model.name);
    let mut broken = Vec::new();
    let mut over = |rule: &str, what: &str, count: usize, max: Option<u32>| {
        if let Some(max) = max.filter(|max| count > *max as usize) {
            broken.push(format!(
                "{name} has {count} {what}, over the {max} {rule} allows"
            ));
        }
    };
    over(
        "max_columns",
        "columns",
        model.columns.len(),
        rules.max_columns,
    );
    over(
        "max_relations",
        "relations",
        model.relations.len(),
        rules.max_relations,
    );
    let json = model
        .columns
        .iter()
        .filter(|column| is_json(&column.col_type));
    over(
        "max_json_columns",
        "JSON columns",
        json.count(),
        rules.max_json_columns,
    );
    let nullable = model.columns.iter().filter(|column| column.nullable);
    over(
        "max_nullable_columns",
        "nullable columns",
        nullable.count(),
        rules.max_nullable_columns,
    );
    over(
        "max_indexes",
        "indexes",
        model.indexes.len(),
        rules.max_indexes,
    );

    let table = model.table.rsplit('.').next().unwrap_or(&model.table);
    let columns = model.columns.iter().map(|column| column.name.as_str());
    let names: Vec<(&str, &str)> = std::iter::once(("table", table))
        .chain(columns.map(|column| ("column", column)))
        .collect();
    let index_names = model
        .indexes
        .iter()
        .map(|index| ("index", index.name.as_str()));
    if let Some(max) = rules.max_identifier_length {
        for (kind, identifier) in names.iter().copied().chain(index_names) {
            if identifier.len() > max as usize {
                broken.push(format!(
                    "{name}'s {kind} name `{identifier}` is {} bytes, over the {max} max_identifier_length allows",
                    identifier.len()
                ));
            }
        }
    }
    for (kind, identifier) in &names {
        if rules.snake_case_names && !is_snake_case(identifier) {
            broken.push(format!(
                "{name}'s {kind} name `{identifier}` is not lowercase snake_case (snake_case_names)"
            ));
        }
        if rules.avoid_reserved_words && is_reserved(identifier) {
            broken.push(format!(
                "{name}'s {kind} name `{identifier}` is an SQL reserved word (avoid_reserved_words)"
            ));
        }
    }

    if !rules.primary_key_types.is_empty() {
        for column in model.columns.iter().filter(|column| column.primary_key) {
            let key_type = key_type_name(&column.col_type);
            if !rules.primary_key_types.contains(&key_type) {
                broken.push(format!(
                    "{name}'s primary key `{}` is {key_type}, not one of {} (primary_key_types)",
                    column.name,
                    rules.primary_key_types.join(", ")
                ));
            }
        }
    }
    if rules.forbid_composite_primary_keys && model.primary_keys.len() > 1 {
        broken.push(format!(
            "{name}'s primary key spans {} columns (forbid_composite_primary_keys)",
            model.primary_keys.len()
        ));
    }
    if rules.require_timestamps && !model.timestamps {
        broken.push(format!(
            "{name} has no created_at/updated_at timestamps (require_timestamps)"
        ));
    }
    if rules.require_soft_delete && !model.soft_delete {
        broken.push(format!("{name} does not soft delete (require_soft_delete)"));
    }
    for column in &model.columns {
        let lower = column.name.to_lowercase();
        let sensitive = rules
            .sensitive_fields
            .iter()
            .any(|word| lower.contains(word));
        if sensitive && !model.protected_columns.contains(&column.name) {
            broken.push(format!(
                "{name}'s column `{}` looks sensitive and is neither hidden nor encrypted (sensitive_fields)",
                column.name
            ));
        }
        if rules.forbid_float_columns
            && matches!(column.col_type, ColumnType::Float | ColumnType::Double)
        {
            broken.push(format!(
                "{name}'s column `{}` is a floating-point number; use a decimal or an integer (forbid_float_columns)",
                column.name
            ));
        }
    }
    broken
}

fn is_json(column_type: &ColumnType) -> bool {
    matches!(column_type, ColumnType::Json | ColumnType::JsonBinary)
}

/// A key column's type as `primary_key_types` names it.
pub(super) fn key_type_name(column_type: &ColumnType) -> &'static str {
    match column_type {
        ColumnType::TinyInteger
        | ColumnType::SmallInteger
        | ColumnType::Integer
        | ColumnType::TinyUnsigned
        | ColumnType::SmallUnsigned
        | ColumnType::Unsigned => "integer",
        ColumnType::BigInteger | ColumnType::BigUnsigned => "bigint",
        ColumnType::Uuid => "uuid",
        ColumnType::Char(_) | ColumnType::String(_) | ColumnType::Text => "string",
        _ => "another type",
    }
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
