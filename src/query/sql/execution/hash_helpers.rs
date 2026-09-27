use super::*;

use std::hash::{Hash, Hasher};

/// Hash a JSON value by structure.
///
/// Numbers are tagged by their representation, so `-1` and `u64::MAX` — which
/// serde_json can hold as distinct `i64`/`u64` payloads — never collide.
fn hash_json_value<H: Hasher>(value: &serde_json::Value, hasher: &mut H) {
    match value {
        serde_json::Value::Null => 0_u8.hash(hasher),
        serde_json::Value::Bool(boolean) => {
            1_u8.hash(hasher);
            boolean.hash(hasher);
        }
        serde_json::Value::Number(number) => {
            2_u8.hash(hasher);
            if let Some(integer) = number.as_i64() {
                0_u8.hash(hasher);
                integer.hash(hasher);
            } else if let Some(integer) = number.as_u64() {
                1_u8.hash(hasher);
                integer.hash(hasher);
            } else if let Some(float) = number.as_f64() {
                2_u8.hash(hasher);
                float.to_bits().hash(hasher);
            }
        }
        serde_json::Value::String(string) => {
            3_u8.hash(hasher);
            string.hash(hasher);
        }
        serde_json::Value::Array(values) => {
            4_u8.hash(hasher);
            hash_json_values(values, hasher);
        }
        serde_json::Value::Object(map) => {
            5_u8.hash(hasher);
            map.len().hash(hasher);
            for (key, value) in map {
                key.hash(hasher);
                hash_json_value(value, hasher);
            }
        }
    }
}

fn hash_json_values<H: Hasher>(values: &[serde_json::Value], hasher: &mut H) {
    values.len().hash(hasher);
    for value in values {
        hash_json_value(value, hasher);
    }
}

fn hash_condition_value<H: Hasher>(value: &ConditionValue, hasher: &mut H) {
    std::mem::discriminant(value).hash(hasher);
    match value {
        ConditionValue::Single(single) => hash_json_value(single, hasher),
        ConditionValue::List(values) => hash_json_values(values, hasher),
        ConditionValue::Range(start, end) => {
            hash_json_value(start, hasher);
            hash_json_value(end, hasher);
        }
        ConditionValue::None => {}
        ConditionValue::RawExpr(expression) => expression.hash(hasher),
        ConditionValue::Column(other) | ConditionValue::Invalid(other) => other.hash(hasher),
        ConditionValue::RawExprWithValues { sql, values }
        | ConditionValue::RawTemplate { sql, values } => {
            sql.hash(hasher);
            hash_bound_values(values, hasher);
        }
    }
}

/// Hash the values bound to a parameterized SQL fragment.
///
/// The bound values MUST participate in the key wherever a fragment is
/// parameterized — raw expressions, union operands, CTE bodies. Two fragments
/// that differ only in a bound parameter render byte-identical SQL, so hashing
/// the text alone would let them share one cache entry and serve one caller's
/// rows to another. sea-query's `Value` implements neither `Hash` nor `Eq`, but
/// its `Debug` rendering is variant- and payload-distinct, which is what the key
/// needs.
pub(super) fn hash_bound_values<H: Hasher>(values: &[Value], hasher: &mut H) {
    values.len().hash(hasher);
    for value in values {
        format!("{:?}", value).hash(hasher);
    }
}

pub(super) fn hash_having_clause<H: Hasher>(sql: &str, params: &[Value], hasher: &mut H) {
    sql.hash(hasher);
    hash_bound_values(params, hasher);
}

pub(super) fn hash_where_condition<H: Hasher>(condition: &WhereCondition, hasher: &mut H) {
    condition.column.hash(hasher);
    condition.operator.hash(hasher);
    hash_condition_value(&condition.value, hasher);
}

pub(super) fn hash_or_group<H: Hasher>(group: &OrGroup, hasher: &mut H) {
    group.combine_with.hash(hasher);
    group.conditions.len().hash(hasher);
    for condition in &group.conditions {
        hash_where_condition(condition, hasher);
    }
    group.nested_groups.len().hash(hasher);
    for nested_group in &group.nested_groups {
        hash_or_group(nested_group, hasher);
    }
}
