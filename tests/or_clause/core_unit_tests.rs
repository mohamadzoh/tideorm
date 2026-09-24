use super::*;

#[test]
fn test_or_group_new() {
    let group = OrGroup::new();

    assert!(group.conditions.is_empty());
    assert!(group.nested_groups.is_empty());
    assert_eq!(group.combine_with, LogicalOp::Or);
}

#[test]
fn test_or_group_where_eq() {
    let group = OrGroup::new().where_eq("role", "admin");

    assert_eq!(group.conditions.len(), 1);
    assert_eq!(group.conditions[0].column, "role");
    assert!(matches!(group.conditions[0].operator, Operator::Eq));

    if let ConditionValue::Single(val) = &group.conditions[0].value {
        assert_eq!(val, &serde_json::json!("admin"));
    } else {
        panic!("Expected Single value");
    }
}

#[test]
fn test_or_group_where_gt() {
    let group = OrGroup::new().where_gt("age", 18);

    assert!(matches!(group.conditions[0].operator, Operator::Gt));
}

#[test]
fn test_or_group_where_like() {
    let group = OrGroup::new().where_like("email", "%@gmail.com");

    assert!(matches!(group.conditions[0].operator, Operator::Like));

    if let ConditionValue::Single(val) = &group.conditions[0].value {
        assert_eq!(val, &serde_json::json!("%@gmail.com"));
    } else {
        panic!("Expected Single value");
    }
}

#[test]
fn test_or_group_where_not_like() {
    let group = OrGroup::new().where_not_like("email", "%@spam.com");

    assert!(matches!(group.conditions[0].operator, Operator::NotLike));
}

#[test]
fn test_or_group_where_in() {
    let group = OrGroup::new().where_in("role", vec!["admin", "moderator", "editor"]);

    assert!(matches!(group.conditions[0].operator, Operator::In));

    if let ConditionValue::List(vals) = &group.conditions[0].value {
        assert_eq!(vals.len(), 3);
    } else {
        panic!("Expected List value");
    }
}

#[test]
fn test_or_group_where_not_in() {
    let group = OrGroup::new().where_not_in("status", vec!["banned", "deleted"]);

    assert!(matches!(group.conditions[0].operator, Operator::NotIn));
}

#[test]
fn test_or_group_where_null() {
    let group = OrGroup::new().where_null("deleted_at");

    assert_eq!(group.conditions[0].column, "deleted_at");
    assert!(matches!(group.conditions[0].operator, Operator::IsNull));
    assert!(matches!(group.conditions[0].value, ConditionValue::None));
}

#[test]
fn test_or_group_where_not_null() {
    let group = OrGroup::new().where_not_null("verified_at");

    assert!(matches!(group.conditions[0].operator, Operator::IsNotNull));
}

#[test]
fn test_or_group_where_between() {
    let group = OrGroup::new().where_between("price", 10, 100);

    assert!(matches!(group.conditions[0].operator, Operator::Between));

    if let ConditionValue::Range(low, high) = &group.conditions[0].value {
        assert_eq!(low, &serde_json::json!(10));
        assert_eq!(high, &serde_json::json!(100));
    } else {
        panic!("Expected Range value");
    }
}

#[test]
fn test_or_group_where_raw() {
    let group = OrGroup::new().where_raw("created_at > NOW() - INTERVAL '30 days'");

    assert!(matches!(group.conditions[0].operator, Operator::Raw));

    if let ConditionValue::RawExpr(expr) = &group.conditions[0].value {
        assert!(expr.contains("INTERVAL"));
    } else {
        panic!("Expected RawExpr value");
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
fn test_or_group_nested_or() {
    let group = OrGroup::new()
        .where_eq("status", "active")
        .nested_or(|inner| {
            inner
                .where_eq("role", "admin")
                .where_eq("role", "moderator")
        });

    assert_eq!(group.conditions.len(), 1);
    assert_eq!(group.nested_groups.len(), 1);
    assert_eq!(group.nested_groups[0].combine_with, LogicalOp::Or);
    assert_eq!(group.nested_groups[0].conditions.len(), 2);
    assert_eq!(group.condition_count(), 3);
}

#[test]
fn test_or_group_nested_and() {
    let group = OrGroup::new()
        .where_eq("status", "active")
        .nested_and(|inner| inner.where_eq("role", "admin").where_gt("age", 25));

    assert_eq!(group.nested_groups.len(), 1);
    assert_eq!(group.nested_groups[0].combine_with, LogicalOp::And);
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
fn test_or_group_is_empty() {
    let empty_group = OrGroup::new();
    assert!(empty_group.is_empty());

    let with_condition = OrGroup::new().where_eq("x", 1);
    assert!(!with_condition.is_empty());

    let with_nested = OrGroup::new().nested_or(|inner| inner.where_eq("y", 2));
    assert!(!with_nested.is_empty());
}

#[test]
fn test_or_group_default() {
    let group: OrGroup = Default::default();
    assert!(group.is_empty());
    assert_eq!(group.combine_with, LogicalOp::Or);
}

#[test]
fn test_or_group_where_not() {
    let group = OrGroup::new().where_not("status", "banned");

    assert!(matches!(group.conditions[0].operator, Operator::NotEq));
}

#[test]
fn test_or_group_ordering_comparisons() {
    let group = OrGroup::new()
        .where_gte("age", 18)
        .where_lt("age", 65)
        .where_lte("score", 100);

    assert!(matches!(group.conditions[0].operator, Operator::Gte));
    assert!(matches!(group.conditions[1].operator, Operator::Lt));
    assert!(matches!(group.conditions[2].operator, Operator::Lte));
}
