use super::*;

fn hash_json_value<H: std::hash::Hasher>(value: &serde_json::Value, hasher: &mut H) {
    use std::hash::Hash;

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
            values.len().hash(hasher);
            for item in values {
                hash_json_value(item, hasher);
            }
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

fn hash_operator<H: std::hash::Hasher>(operator: &Operator, hasher: &mut H) {
    use std::hash::Hash;

    let tag = match operator {
        Operator::Eq => 0_u8,
        Operator::NotEq => 1,
        Operator::Gt => 2,
        Operator::Gte => 3,
        Operator::Lt => 4,
        Operator::Lte => 5,
        Operator::Like => 6,
        Operator::LikeEscaped => 7,
        Operator::NotLike => 8,
        Operator::In => 9,
        Operator::NotIn => 10,
        Operator::IsNull => 11,
        Operator::IsNotNull => 12,
        Operator::Between => 13,
        Operator::JsonContains => 14,
        Operator::JsonContainedBy => 15,
        Operator::JsonKeyExists => 16,
        Operator::JsonKeyNotExists => 17,
        Operator::JsonPathExists => 18,
        Operator::JsonPathNotExists => 19,
        Operator::ArrayContains => 20,
        Operator::ArrayContainedBy => 21,
        Operator::ArrayOverlaps => 22,
        Operator::ArrayContainsAny => 23,
        Operator::ArrayContainsAll => 24,
        Operator::SubqueryIn => 25,
        Operator::SubqueryNotIn => 26,
        Operator::Raw => 27,
        Operator::EqAny => 28,
        Operator::NeAll => 29,
    };
    tag.hash(hasher);
}

fn hash_condition_value<H: std::hash::Hasher>(value: &ConditionValue, hasher: &mut H) {
    use std::hash::Hash;

    match value {
        ConditionValue::Single(single) => {
            0_u8.hash(hasher);
            hash_json_value(single, hasher);
        }
        ConditionValue::List(values) => {
            1_u8.hash(hasher);
            values.len().hash(hasher);
            for value in values {
                hash_json_value(value, hasher);
            }
        }
        ConditionValue::Range(start, end) => {
            2_u8.hash(hasher);
            hash_json_value(start, hasher);
            hash_json_value(end, hasher);
        }
        ConditionValue::None => 3_u8.hash(hasher),
        ConditionValue::Subquery(query_sql) => {
            4_u8.hash(hasher);
            query_sql.hash(hasher);
        }
        ConditionValue::RawExpr(expression) => {
            5_u8.hash(hasher);
            expression.hash(hasher);
        }
    }
}

pub(super) fn hash_having_clause<H: std::hash::Hasher>(
    sql: &str,
    params: &[serde_json::Value],
    hasher: &mut H,
) {
    use std::hash::Hash;

    sql.hash(hasher);
    params.len().hash(hasher);
    for value in params {
        hash_json_value(value, hasher);
    }
}

pub(super) fn hash_where_condition<H: std::hash::Hasher>(
    condition: &WhereCondition,
    hasher: &mut H,
) {
    use std::hash::Hash;

    condition.column.hash(hasher);
    hash_operator(&condition.operator, hasher);
    hash_condition_value(&condition.value, hasher);
}

fn hash_logical_op<H: std::hash::Hasher>(logical_op: LogicalOp, hasher: &mut H) {
    use std::hash::Hash;

    match logical_op {
        LogicalOp::And => 0_u8.hash(hasher),
        LogicalOp::Or => 1_u8.hash(hasher),
    }
}

pub(super) fn hash_or_group<H: std::hash::Hasher>(group: &OrGroup, hasher: &mut H) {
    use std::hash::Hash;

    hash_logical_op(group.combine_with, hasher);
    group.conditions.len().hash(hasher);
    for condition in &group.conditions {
        hash_where_condition(condition, hasher);
    }
    group.nested_groups.len().hash(hasher);
    for nested_group in &group.nested_groups {
        hash_or_group(nested_group, hasher);
    }
}

fn hash_frame_bound<H: std::hash::Hasher>(frame_bound: &FrameBound, hasher: &mut H) {
    use std::hash::Hash;

    match frame_bound {
        FrameBound::UnboundedPreceding => 0_u8.hash(hasher),
        FrameBound::UnboundedFollowing => 1_u8.hash(hasher),
        FrameBound::CurrentRow => 2_u8.hash(hasher),
        FrameBound::Preceding(value) => {
            3_u8.hash(hasher);
            value.hash(hasher);
        }
        FrameBound::Following(value) => {
            4_u8.hash(hasher);
            value.hash(hasher);
        }
    }
}

fn hash_frame_type<H: std::hash::Hasher>(frame_type: FrameType, hasher: &mut H) {
    use std::hash::Hash;

    match frame_type {
        FrameType::Rows => 0_u8.hash(hasher),
        FrameType::Range => 1_u8.hash(hasher),
        FrameType::Groups => 2_u8.hash(hasher),
    }
}

fn hash_window_function_type<H: std::hash::Hasher>(
    function_type: &WindowFunctionType,
    hasher: &mut H,
) {
    use std::hash::Hash;

    match function_type {
        WindowFunctionType::RowNumber => 0_u8.hash(hasher),
        WindowFunctionType::Rank => 1_u8.hash(hasher),
        WindowFunctionType::DenseRank => 2_u8.hash(hasher),
        WindowFunctionType::Ntile(buckets) => {
            3_u8.hash(hasher);
            buckets.hash(hasher);
        }
        WindowFunctionType::Lag(column, offset, default) => {
            4_u8.hash(hasher);
            column.hash(hasher);
            offset.hash(hasher);
            default.hash(hasher);
        }
        WindowFunctionType::Lead(column, offset, default) => {
            5_u8.hash(hasher);
            column.hash(hasher);
            offset.hash(hasher);
            default.hash(hasher);
        }
        WindowFunctionType::FirstValue(column) => {
            6_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::LastValue(column) => {
            7_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::NthValue(column, nth) => {
            8_u8.hash(hasher);
            column.hash(hasher);
            nth.hash(hasher);
        }
        WindowFunctionType::Sum(column) => {
            9_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::Avg(column) => {
            10_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::Count(column) => {
            11_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::Min(column) => {
            12_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::Max(column) => {
            13_u8.hash(hasher);
            column.hash(hasher);
        }
        WindowFunctionType::Custom(expression) => {
            14_u8.hash(hasher);
            expression.hash(hasher);
        }
    }
}

pub(super) fn hash_window_function<H: std::hash::Hasher>(
    window_function: &WindowFunction,
    hasher: &mut H,
) {
    use std::hash::Hash;

    hash_window_function_type(&window_function.function, hasher);
    window_function.partition_by.hash(hasher);
    for (column, direction) in &window_function.order_by {
        column.hash(hasher);
        direction.as_str().hash(hasher);
    }
    if let Some(frame_type) = window_function.frame_type {
        1_u8.hash(hasher);
        hash_frame_type(frame_type, hasher);
    } else {
        0_u8.hash(hasher);
    }
    if let Some(frame_start) = &window_function.frame_start {
        1_u8.hash(hasher);
        hash_frame_bound(frame_start, hasher);
    } else {
        0_u8.hash(hasher);
    }
    if let Some(frame_end) = &window_function.frame_end {
        1_u8.hash(hasher);
        hash_frame_bound(frame_end, hasher);
    } else {
        0_u8.hash(hasher);
    }
    window_function.alias.hash(hasher);
}
