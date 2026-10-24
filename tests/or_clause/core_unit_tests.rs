use super::*;
use serde_json::json;

/// Each `where_*` method of an `OrGroup` records one condition with its
/// column, operator and value.
#[test]
fn test_or_group_condition_methods_record_their_condition() {
    let single = ConditionValue::Single;
    let cases = [
        (
            "where_eq",
            OrGroup::new().where_eq("role", "admin"),
            "role",
            Operator::Eq,
            single(json!("admin")),
        ),
        (
            "where_not",
            OrGroup::new().where_not("status", "banned"),
            "status",
            Operator::NotEq,
            single(json!("banned")),
        ),
        (
            "where_gt",
            OrGroup::new().where_gt("age", 18),
            "age",
            Operator::Gt,
            single(json!(18)),
        ),
        (
            "where_gte",
            OrGroup::new().where_gte("age", 18),
            "age",
            Operator::Gte,
            single(json!(18)),
        ),
        (
            "where_lt",
            OrGroup::new().where_lt("age", 65),
            "age",
            Operator::Lt,
            single(json!(65)),
        ),
        (
            "where_lte",
            OrGroup::new().where_lte("score", 100),
            "score",
            Operator::Lte,
            single(json!(100)),
        ),
        (
            "where_like",
            OrGroup::new().where_like("email", "%@gmail.com"),
            "email",
            Operator::Like,
            single(json!("%@gmail.com")),
        ),
        (
            "where_not_like",
            OrGroup::new().where_not_like("email", "%@spam.com"),
            "email",
            Operator::NotLike,
            single(json!("%@spam.com")),
        ),
        (
            "where_in",
            OrGroup::new().where_in("role", vec!["admin", "moderator", "editor"]),
            "role",
            Operator::In,
            ConditionValue::List(vec![json!("admin"), json!("moderator"), json!("editor")]),
        ),
        (
            "where_not_in",
            OrGroup::new().where_not_in("status", vec!["banned", "deleted"]),
            "status",
            Operator::NotIn,
            ConditionValue::List(vec![json!("banned"), json!("deleted")]),
        ),
        (
            "where_null",
            OrGroup::new().where_null("deleted_at"),
            "deleted_at",
            Operator::IsNull,
            ConditionValue::None,
        ),
        (
            "where_not_null",
            OrGroup::new().where_not_null("verified_at"),
            "verified_at",
            Operator::IsNotNull,
            ConditionValue::None,
        ),
        (
            "where_between",
            OrGroup::new().where_between("price", 10, 100),
            "price",
            Operator::Between,
            ConditionValue::Range(json!(10), json!(100)),
        ),
        (
            "where_raw",
            OrGroup::new().where_raw("created_at > NOW() - INTERVAL '30 days'"),
            "",
            Operator::Raw,
            ConditionValue::RawExpr("created_at > NOW() - INTERVAL '30 days'".to_string()),
        ),
    ];

    for (method, group, column, operator, value) in cases {
        assert_eq!(group.conditions.len(), 1, "{method}");
        let condition = &group.conditions[0];
        assert_eq!(condition.column, column, "{method}");
        assert_eq!(condition.operator, operator, "{method}");
        assert_eq!(condition.value, value, "{method}");
    }
}

#[test]
fn test_or_group_starts_empty_and_ors_its_members() {
    for group in [OrGroup::new(), OrGroup::default()] {
        assert!(group.conditions.is_empty());
        assert!(group.nested_groups.is_empty());
        assert!(group.is_empty());
        assert_eq!(group.combine_with, LogicalOp::Or);
    }
}

#[test]
fn test_or_group_chaining() {
    let group = OrGroup::new()
        .where_eq("role", "admin")
        .where_eq("role", "moderator")
        .where_gt("age", 21);

    assert_eq!(group.conditions.len(), 3);
    assert!(!group.is_empty());
    assert_eq!(group.condition_count(), 3);
}

#[test]
fn test_or_group_nested_or_and_nested_and_set_their_combinator() {
    let group = OrGroup::new()
        .where_eq("status", "active")
        .nested_or(|inner| {
            inner
                .where_eq("role", "admin")
                .where_eq("role", "moderator")
        })
        .nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 25));

    assert_eq!(group.conditions.len(), 1);
    assert_eq!(group.nested_groups.len(), 2);
    assert_eq!(group.nested_groups[0].combine_with, LogicalOp::Or);
    assert_eq!(group.nested_groups[0].conditions.len(), 2);
    assert_eq!(group.nested_groups[1].combine_with, LogicalOp::And);
    assert_eq!(group.nested_groups[1].conditions.len(), 2);
    assert_eq!(group.condition_count(), 5);
}

#[test]
fn test_or_group_deeply_nested() {
    let group = OrGroup::new().nested_or(|q| {
        q.where_eq("status", "active")
            .nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 30))
    });

    assert_eq!(group.conditions.len(), 0);
    assert_eq!(group.nested_groups.len(), 1);
    assert_eq!(group.nested_groups[0].nested_groups.len(), 1);

    let nested = &group.nested_groups[0];
    assert_eq!(nested.conditions.len(), 1);
    assert_eq!(nested.nested_groups[0].conditions.len(), 2);
    assert_eq!(group.condition_count(), 3);
}

#[test]
fn test_or_group_is_empty_only_without_conditions_at_any_depth() {
    assert!(!OrGroup::new().where_eq("x", 1).is_empty());
    assert!(
        !OrGroup::new()
            .nested_or(|inner| inner.where_eq("y", 2))
            .is_empty()
    );
    assert!(OrGroup::new().nested_or(|inner| inner).is_empty());
}
