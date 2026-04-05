use super::*;

// ============================================================================
// Unit tests for OrGroup construction (no DB needed)
// ============================================================================

#[cfg(test)]
mod or_group_unit_tests {
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
}

// ============================================================================
// Tests for ConditionValue variants (no DB needed)
// ============================================================================

#[cfg(test)]
mod condition_value_tests {
    use super::*;

    #[test]
    fn test_condition_value_single() {
        let val = ConditionValue::Single(serde_json::json!("test"));
        match val {
            ConditionValue::Single(v) => assert_eq!(v, serde_json::json!("test")),
            _ => panic!("Expected Single variant"),
        }
    }

    #[test]
    fn test_condition_value_list() {
        let val = ConditionValue::List(vec![
            serde_json::json!("a"),
            serde_json::json!("b"),
            serde_json::json!("c"),
        ]);
        match val {
            ConditionValue::List(v) => {
                assert_eq!(v.len(), 3);
            }
            _ => panic!("Expected List variant"),
        }
    }

    #[test]
    fn test_condition_value_range() {
        let val = ConditionValue::Range(serde_json::json!(10), serde_json::json!(100));
        match val {
            ConditionValue::Range(low, high) => {
                assert_eq!(low, serde_json::json!(10));
                assert_eq!(high, serde_json::json!(100));
            }
            _ => panic!("Expected Range variant"),
        }
    }

    #[test]
    fn test_condition_value_none() {
        let val = ConditionValue::None;
        assert!(matches!(val, ConditionValue::None));
    }
}

// ============================================================================
// Tests for Operator variants
// ============================================================================

#[cfg(test)]
mod operator_tests {
    use super::*;

    #[test]
    fn test_all_operators_exist() {
        let _ = Operator::Eq;
        let _ = Operator::NotEq;
        let _ = Operator::Gt;
        let _ = Operator::Gte;
        let _ = Operator::Lt;
        let _ = Operator::Lte;
        let _ = Operator::Like;
        let _ = Operator::NotLike;
        let _ = Operator::In;
        let _ = Operator::NotIn;
        let _ = Operator::IsNull;
        let _ = Operator::IsNotNull;
        let _ = Operator::Between;
        let _ = Operator::JsonContains;
        let _ = Operator::JsonContainedBy;
        let _ = Operator::JsonKeyExists;
        let _ = Operator::JsonKeyNotExists;
        let _ = Operator::JsonPathExists;
        let _ = Operator::JsonPathNotExists;
        let _ = Operator::ArrayContains;
        let _ = Operator::ArrayContainedBy;
        let _ = Operator::ArrayOverlaps;
        let _ = Operator::SubqueryIn;
        let _ = Operator::SubqueryNotIn;
        let _ = Operator::Raw;
        let _ = Operator::EqAny;
        let _ = Operator::NeAll;
    }
}

// ============================================================================
// Tests for Order enum
// ============================================================================

#[cfg(test)]
mod order_tests {
    use super::*;

    #[test]
    fn test_order_as_str() {
        assert_eq!(Order::Asc.as_str(), "ASC");
        assert_eq!(Order::Desc.as_str(), "DESC");
    }
}
