use super::*;

// ============================================================================
// Integration tests with QueryBuilder (require model but not database)
// ============================================================================

#[cfg(test)]
mod query_builder_or_tests {
    use super::*;

    #[test]
    fn test_or_group_complex_scenario() {
        let status_or_group = OrGroup::new()
            .where_eq("status", "active")
            .nested_and(|inner| {
                inner
                    .where_gt("age", 25)
                    .where_eq("department", "Engineering")
            });

        let role_or_group = OrGroup::new()
            .where_eq("role", "admin")
            .where_eq("role", "moderator");

        assert_eq!(status_or_group.conditions.len(), 1);
        assert_eq!(status_or_group.nested_groups.len(), 1);
        assert_eq!(role_or_group.conditions.len(), 2);
        assert_eq!(role_or_group.nested_groups.len(), 0);
    }

    #[test]
    fn test_or_group_with_various_types() {
        let group = OrGroup::new()
            .where_eq("string_field", "value")
            .where_eq("int_field", 42)
            .where_eq("float_field", 3.14)
            .where_eq("bool_field", true);

        assert_eq!(group.conditions.len(), 4);

        if let ConditionValue::Single(val) = &group.conditions[0].value {
            assert!(val.is_string());
        }
        if let ConditionValue::Single(val) = &group.conditions[1].value {
            assert!(val.is_number());
        }
        if let ConditionValue::Single(val) = &group.conditions[2].value {
            assert!(val.is_number());
        }
        if let ConditionValue::Single(val) = &group.conditions[3].value {
            assert!(val.is_boolean());
        }
    }
}

// ============================================================================
// Complex scenario tests
// ============================================================================

#[cfg(test)]
mod complex_scenario_tests {
    use super::*;

    #[test]
    fn test_complex_business_logic_structure() {
        let role_group = OrGroup::new()
            .where_eq("role", "admin")
            .where_eq("role", "moderator");

        let dept_age_group = OrGroup::new()
            .where_eq("department", "Engineering")
            .where_gt("age", 30);

        assert_eq!(role_group.condition_count(), 2);
        assert_eq!(dept_age_group.condition_count(), 2);
    }

    #[test]
    fn test_email_domain_search() {
        let group = OrGroup::new()
            .where_like("email", "%@gmail.com")
            .where_like("email", "%@yahoo.com")
            .where_like("email", "%@outlook.com");

        assert_eq!(group.conditions.len(), 3);
        for cond in &group.conditions {
            assert!(matches!(cond.operator, Operator::Like));
        }
    }

    #[test]
    fn test_price_range_search() {
        let group = OrGroup::new()
            .where_between("price", 0, 50)
            .where_between("price", 200, 500);

        assert_eq!(group.conditions.len(), 2);
        for cond in &group.conditions {
            assert!(matches!(cond.operator, Operator::Between));
        }
    }

    #[test]
    fn test_null_or_default() {
        let group = OrGroup::new()
            .where_null("profile_picture")
            .where_eq("profile_picture", "default.png");

        assert_eq!(group.conditions.len(), 2);
        assert!(matches!(group.conditions[0].operator, Operator::IsNull));
        assert!(matches!(group.conditions[1].operator, Operator::Eq));
    }
}

// ============================================================================
// Edge case tests
// ============================================================================

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[test]
    fn test_empty_or_group() {
        let group = OrGroup::new();
        assert!(group.is_empty());
        assert_eq!(group.condition_count(), 0);
    }

    #[test]
    fn test_single_condition_or_group() {
        let group = OrGroup::new().where_eq("x", 1);

        assert!(!group.is_empty());
        assert_eq!(group.condition_count(), 1);
    }

    #[test]
    fn test_empty_nested_or_group() {
        let group = OrGroup::new().nested_or(|inner| inner);

        assert_eq!(group.nested_groups.len(), 1);
        assert!(group.nested_groups[0].is_empty());
    }

    #[test]
    fn test_special_characters_in_values() {
        let group = OrGroup::new()
            .where_eq("name", "O'Reilly")
            .where_like("description", "%test's%")
            .where_eq("path", "C:\\Users\\test");

        assert_eq!(group.conditions.len(), 3);
    }

    #[test]
    fn test_unicode_values() {
        let group = OrGroup::new()
            .where_eq("name", "日本語")
            .where_like("description", "%émoji 🎉%");

        assert_eq!(group.conditions.len(), 2);
    }

    #[test]
    fn test_empty_string_values() {
        let group = OrGroup::new()
            .where_eq("field", "")
            .where_like("pattern", "");

        assert_eq!(group.conditions.len(), 2);
    }

    #[test]
    fn test_large_in_list() {
        let values: Vec<i32> = (0..1000).collect();
        let group = OrGroup::new().where_in("id", values);

        if let ConditionValue::List(vals) = &group.conditions[0].value {
            assert_eq!(vals.len(), 1000);
        }
    }
}

// ============================================================================
// Unit tests for OrBranch (no DB needed)
// ============================================================================

#[cfg(test)]
mod or_branch_unit_tests {
    use tideorm::query::{ConditionValue, Operator, OrBranch};

    #[test]
    fn test_or_branch_new() {
        let branch = OrBranch::new();
        assert!(branch.is_empty());
        assert_eq!(branch.len(), 0);
    }

    #[test]
    fn test_or_branch_where_eq() {
        let branch = OrBranch::new().where_eq("role", "admin");

        assert_eq!(branch.len(), 1);
        assert!(!branch.is_empty());
        assert_eq!(branch.conditions[0].column, "role");
        assert!(matches!(branch.conditions[0].operator, Operator::Eq));
    }

    #[test]
    fn test_or_branch_chained_conditions() {
        let branch = OrBranch::new()
            .where_eq("role", "admin")
            .where_eq("active", true)
            .where_gt("age", 18);

        assert_eq!(branch.len(), 3);
        assert_eq!(branch.conditions[0].column, "role");
        assert_eq!(branch.conditions[1].column, "active");
        assert_eq!(branch.conditions[2].column, "age");
    }

    #[test]
    fn test_or_branch_where_not() {
        let branch = OrBranch::new().where_not("status", "banned");

        assert_eq!(branch.len(), 1);
        assert!(matches!(branch.conditions[0].operator, Operator::NotEq));
    }

    #[test]
    fn test_or_branch_where_gt() {
        let branch = OrBranch::new().where_gt("age", 18);

        assert!(matches!(branch.conditions[0].operator, Operator::Gt));
    }

    #[test]
    fn test_or_branch_where_gte() {
        let branch = OrBranch::new().where_gte("age", 18);

        assert!(matches!(branch.conditions[0].operator, Operator::Gte));
    }

    #[test]
    fn test_or_branch_where_lt() {
        let branch = OrBranch::new().where_lt("age", 65);

        assert!(matches!(branch.conditions[0].operator, Operator::Lt));
    }

    #[test]
    fn test_or_branch_where_lte() {
        let branch = OrBranch::new().where_lte("age", 65);

        assert!(matches!(branch.conditions[0].operator, Operator::Lte));
    }

    #[test]
    fn test_or_branch_where_like() {
        let branch = OrBranch::new().where_like("name", "%john%");

        assert!(matches!(branch.conditions[0].operator, Operator::Like));
        if let ConditionValue::Single(val) = &branch.conditions[0].value {
            assert_eq!(val.as_str().unwrap(), "%john%");
        }
    }

    #[test]
    fn test_or_branch_where_not_like() {
        let branch = OrBranch::new().where_not_like("name", "%test%");

        assert!(matches!(branch.conditions[0].operator, Operator::NotLike));
    }

    #[test]
    fn test_or_branch_where_in() {
        let branch = OrBranch::new().where_in("status", vec!["active", "pending"]);

        assert!(matches!(branch.conditions[0].operator, Operator::In));
        if let ConditionValue::List(vals) = &branch.conditions[0].value {
            assert_eq!(vals.len(), 2);
        }
    }

    #[test]
    fn test_or_branch_where_not_in() {
        let branch = OrBranch::new().where_not_in("status", vec!["banned", "deleted"]);

        assert!(matches!(branch.conditions[0].operator, Operator::NotIn));
    }

    #[test]
    fn test_or_branch_where_null() {
        let branch = OrBranch::new().where_null("deleted_at");

        assert!(matches!(branch.conditions[0].operator, Operator::IsNull));
    }

    #[test]
    fn test_or_branch_where_not_null() {
        let branch = OrBranch::new().where_not_null("verified_at");

        assert!(matches!(branch.conditions[0].operator, Operator::IsNotNull));
    }

    #[test]
    fn test_or_branch_where_between() {
        let branch = OrBranch::new().where_between("age", 18, 65);

        assert!(matches!(branch.conditions[0].operator, Operator::Between));
        if let ConditionValue::Range(min, max) = &branch.conditions[0].value {
            assert_eq!(min.as_i64().unwrap(), 18);
            assert_eq!(max.as_i64().unwrap(), 65);
        }
    }

    #[test]
    fn test_or_branch_where_raw() {
        let branch = OrBranch::new().where_raw("created_at > NOW()");

        assert!(matches!(branch.conditions[0].operator, Operator::Raw));
    }

    #[test]
    fn test_or_branch_complex_chain() {
        let branch = OrBranch::new()
            .where_eq("role", "admin")
            .where_eq("active", true)
            .where_gt("age", 18)
            .where_not_null("verified_at");

        assert_eq!(branch.len(), 4);
        assert_eq!(branch.conditions[0].column, "role");
        assert_eq!(branch.conditions[1].column, "active");
        assert_eq!(branch.conditions[2].column, "age");
        assert_eq!(branch.conditions[3].column, "verified_at");
    }

    #[test]
    fn test_or_branch_default() {
        let branch = OrBranch::default();
        assert!(branch.is_empty());
    }
}
