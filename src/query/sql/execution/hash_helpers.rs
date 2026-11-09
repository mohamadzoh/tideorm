use super::*;

use std::hash::{Hash, Hasher};

fn hash_condition_value<H: Hasher>(value: &ConditionValue, hasher: &mut H) {
    std::mem::discriminant(value).hash(hasher);
    match value {
        ConditionValue::Single(single) => single.hash_binding(hasher),
        ConditionValue::List(values) => hash_bound_values(values, hasher),
        ConditionValue::Range(start, end) => {
            start.hash_binding(hasher);
            end.hash_binding(hasher);
        }
        ConditionValue::None => {}
        ConditionValue::RawExpr(expression) => expression.hash(hasher),
        ConditionValue::Column(other) | ConditionValue::Invalid(other) => other.hash(hasher),
        ConditionValue::RawTemplate { sql, values } => hash_fragment(sql, values, hasher),
        ConditionValue::Subquery(subquery) => subquery.hash_into(hasher),
    }
}

/// Hash the values bound to a parameterized SQL fragment.
///
/// The bound values MUST participate in the key wherever a fragment is
/// parameterized — raw expressions, union operands, CTE bodies. Two fragments
/// that differ only in a bound parameter render byte-identical SQL, so hashing
/// the text alone would let them share one cache entry and serve one caller's
/// rows to another. `Value`'s own `Hash` follows its `Eq`, under which decimals
/// `1.0` and `1.00`, or one instant at two offsets, are equal although they can
/// bind differently; the binding hash preserves those representation details.
fn hash_bound_values<H: Hasher>(values: &[Value], hasher: &mut H) {
    values.len().hash(hasher);
    for value in values {
        value.hash_binding(hasher);
    }
}

/// Hash a parameterized SQL fragment: its text and its bound values.
fn hash_fragment<H: Hasher>(sql: &str, params: &[Value], hasher: &mut H) {
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
