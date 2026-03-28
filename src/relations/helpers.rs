use std::ops::Deref;

use crate::error::{Error, Result};
use crate::internal::Value;
use crate::model::Model;

pub(crate) use crate::internal::sql_safety::quote_ident;

pub(crate) fn ensure_relation_configured(
    relation_name: &str,
    required_values: &[&str],
) -> Result<()> {
    if required_values.iter().any(|value| value.is_empty()) {
        return Err(Error::query(format!(
            "{relation_name} relation is not configured; use {relation_name}::new(...) or a macro-generated relation field",
        )));
    }

    Ok(())
}

pub(crate) fn preserve_cached_value<C: Clone>(
    cached: &mut Option<C>,
    previous_cached: &Option<C>,
    allow_cached_without_context: bool,
    same_runtime_context: bool,
) {
    if (allow_cached_without_context && previous_cached.is_some()) || same_runtime_context {
        *cached = previous_cached.clone();
    }
}

pub(crate) fn cached_ref<C>(cached: &Option<C>) -> Option<&C::Target>
where
    C: Deref,
{
    cached.as_deref()
}

pub(crate) fn resolve_model_column_name<E: Model>(name: &str) -> Result<&'static str> {
    if E::column_from_str(name).is_none() {
        return Err(Error::query(format!(
            "Unknown self-reference column '{}' for table '{}'",
            name,
            E::table_name()
        )));
    }

    E::field_names()
        .iter()
        .zip(E::column_names().iter())
        .find_map(|(field_name, column_name)| {
            if *field_name == name || *column_name == name {
                Some(*column_name)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Error::query(format!(
                "Unknown self-reference column '{}' for table '{}'",
                name,
                E::table_name()
            ))
        })
}

pub(crate) fn json_to_db_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::String(None),
        serde_json::Value::Bool(boolean) => Value::Bool(Some(*boolean)),
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Value::BigInt(Some(integer))
            } else if let Some(float) = number.as_f64() {
                Value::Double(Some(float))
            } else {
                Value::String(Some(number.to_string()))
            }
        }
        serde_json::Value::String(text) => Value::String(Some(text.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Value::String(Some(value.to_string()))
        }
    }
}

pub(crate) fn push_param(
    db_type: crate::config::DatabaseType,
    params: &mut Vec<Value>,
    value: Value,
) -> String {
    let placeholder = match db_type {
        crate::config::DatabaseType::Postgres => format!("${}", params.len() + 1),
        crate::config::DatabaseType::MySQL
        | crate::config::DatabaseType::MariaDB
        | crate::config::DatabaseType::SQLite => "?".to_string(),
    };
    params.push(value);
    placeholder
}

pub(crate) fn scoped_column(
    db_type: crate::config::DatabaseType,
    scope: &str,
    column: &str,
) -> String {
    format!(
        "{}.{}",
        quote_ident(db_type, scope),
        quote_ident(db_type, column)
    )
}

pub(crate) fn soft_delete_clause<E: Model>(
    db_type: crate::config::DatabaseType,
    scope: &str,
) -> Option<String> {
    if E::soft_delete_enabled() {
        Some(format!(
            "{} IS NULL",
            scoped_column(db_type, scope, E::deleted_at_column())
        ))
    } else {
        None
    }
}

pub(crate) fn build_self_ref_tree_sql<E: Model>(
    foreign_key: &str,
    local_key: &str,
    parent_pk: &serde_json::Value,
    max_depth: usize,
    db_type: crate::config::DatabaseType,
) -> Result<(String, Vec<Value>)> {
    let foreign_key = resolve_model_column_name::<E>(foreign_key)?;
    let local_key = resolve_model_column_name::<E>(local_key)?;
    let primary_key = resolve_model_column_name::<E>(E::primary_key_name())?;

    let table = quote_ident(db_type, E::table_name());
    let cte = quote_ident(db_type, "tide_tree");
    let node = quote_ident(db_type, "node");
    let child = quote_ident(db_type, "child");
    let tree = quote_ident(db_type, "tree");
    let result = quote_ident(db_type, "result_node");
    let pk_alias = quote_ident(db_type, "pk");
    let tree_key_alias = quote_ident(db_type, "tree_key");
    let depth_alias = quote_ident(db_type, "depth");

    let mut params = Vec::with_capacity(2);
    let parent_placeholder = push_param(db_type, &mut params, json_to_db_value(parent_pk));
    let max_depth = i64::try_from(max_depth)
        .map_err(|_| Error::query("Self-reference tree depth exceeds i64 range"))?;
    let depth_placeholder = push_param(db_type, &mut params, Value::BigInt(Some(max_depth)));

    let mut base_predicates = vec![format!(
        "{} = {}",
        scoped_column(db_type, "node", foreign_key),
        parent_placeholder
    )];
    if let Some(clause) = soft_delete_clause::<E>(db_type, "node") {
        base_predicates.push(clause);
    }

    let mut recursive_predicates =
        vec![format!("{}.{} < {}", tree, depth_alias, depth_placeholder)];
    if let Some(clause) = soft_delete_clause::<E>(db_type, "child") {
        recursive_predicates.push(clause);
    }

    let sql = format!(
        "WITH RECURSIVE {cte} ({pk_alias}, {tree_key_alias}, {depth_alias}) AS ( \
         SELECT {node_pk} AS {pk_alias}, {node_local_key} AS {tree_key_alias}, 1 AS {depth_alias} \
         FROM {table} {node} \
         WHERE {base_where} \
         UNION ALL \
         SELECT {child_pk} AS {pk_alias}, {child_local_key} AS {tree_key_alias}, {tree}.{depth_alias} + 1 AS {depth_alias} \
         FROM {table} {child} \
         INNER JOIN {cte} {tree} ON {child_foreign_key} = {tree}.{tree_key_alias} \
         WHERE {recursive_where} \
         ) \
         SELECT {result}.* \
         FROM {table} {result} \
         INNER JOIN {cte} {result_tree} ON {result_pk} = {result_tree}.{pk_alias} \
         ORDER BY {result_tree}.{depth_alias}",
        cte = cte,
        pk_alias = pk_alias,
        tree_key_alias = tree_key_alias,
        depth_alias = depth_alias,
        node_pk = scoped_column(db_type, "node", primary_key),
        node_local_key = scoped_column(db_type, "node", local_key),
        table = table,
        node = node,
        base_where = base_predicates.join(" AND "),
        child_pk = scoped_column(db_type, "child", primary_key),
        child_local_key = scoped_column(db_type, "child", local_key),
        child = child,
        tree = tree,
        child_foreign_key = scoped_column(db_type, "child", foreign_key),
        recursive_where = recursive_predicates.join(" AND "),
        result = result,
        result_tree = quote_ident(db_type, "result_tree"),
        result_pk = scoped_column(db_type, "result_node", primary_key),
    );

    Ok((sql, params))
}
