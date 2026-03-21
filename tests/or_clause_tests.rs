//! Tests for OR Clause functionality in TideORM
//!
//! These tests verify the OR clause query builder features:
//! - OrGroup construction and methods
//! - or_where and or_where_* methods on QueryBuilder
#![allow(clippy::approx_constant)]
//! - Nested OR groups
//! - OR with BatchUpdateBuilder
//!
//! Note: Some tests require a database connection.
//! Set TEST_DATABASE_URL environment variable to run integration tests.

use tideorm::query::{ConditionValue, LogicalOp, Operator, OrGroup, Order};

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
        assert_eq!(group.condition_count(), 3); // 1 direct + 2 nested
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

        // Count should include all nested conditions
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
        // Standard comparison operators
        let _ = Operator::Eq;
        let _ = Operator::NotEq;
        let _ = Operator::Gt;
        let _ = Operator::Gte;
        let _ = Operator::Lt;
        let _ = Operator::Lte;

        // Pattern matching
        let _ = Operator::Like;
        let _ = Operator::NotLike;

        // Collection operators
        let _ = Operator::In;
        let _ = Operator::NotIn;

        // Null checks
        let _ = Operator::IsNull;
        let _ = Operator::IsNotNull;

        // Range
        let _ = Operator::Between;

        // JSON operators
        let _ = Operator::JsonContains;
        let _ = Operator::JsonContainedBy;
        let _ = Operator::JsonKeyExists;
        let _ = Operator::JsonKeyNotExists;
        let _ = Operator::JsonPathExists;
        let _ = Operator::JsonPathNotExists;

        // Array operators
        let _ = Operator::ArrayContains;
        let _ = Operator::ArrayContainedBy;
        let _ = Operator::ArrayOverlaps;

        // Subquery and raw
        let _ = Operator::SubqueryIn;
        let _ = Operator::SubqueryNotIn;
        let _ = Operator::Raw;

        // PostgreSQL optimizations
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

// ============================================================================
// Integration tests with QueryBuilder (require model but not database)
// ============================================================================

#[cfg(test)]
mod query_builder_or_tests {
    // Note: These tests would normally use actual models
    // For unit testing without DB, we test the structures directly

    use super::*;

    #[test]
    fn test_or_group_complex_scenario() {
        // Simulate building a complex OR query:
        // WHERE active = true AND (role = 'admin' OR role = 'moderator')
        //   AND (status = 'active' OR (age > 25 AND department = 'Engineering'))

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

        // Verify structure
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

        // Check types are preserved
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
// Integration tests (require database - marked with #[ignore])
// ============================================================================

/// Run these tests with: cargo test -- --ignored
/// And with TEST_DATABASE_URL set
#[cfg(test)]
mod integration_tests {
    /*
    Example integration test structure:

    use tideorm::prelude::*;

    #[derive(Model, Clone, Debug)]
    #[tideorm(table = "or_test_users")]
    struct OrTestUser {
        #[tideorm(primary_key, auto_increment)]
        pub id: i64,
        pub name: String,
        pub role: String,
        pub status: String,
        pub age: i32,
    }

    #[tokio::test]
    #[ignore]
    async fn test_or_where_query() {
        // Setup database connection
        // ...

        // Test simple OR query
        let results = OrTestUser::query()
            .or_where(|q| q
                .where_eq("role", "admin")
                .where_eq("role", "moderator")
            )
            .get()
            .await
            .unwrap();

        // Verify results contain only admins and moderators
        for user in &results {
            assert!(user.role == "admin" || user.role == "moderator");
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_or_with_and_conditions() {
        // Test: WHERE active = true AND (role = 'admin' OR status = 'vip')
        let results = OrTestUser::query()
            .where_eq("active", true)
            .or_where(|q| q
                .where_eq("role", "admin")
                .where_eq("status", "vip")
            )
            .get()
            .await
            .unwrap();

        // All results should be active
        // And either admin OR vip
    }

    #[tokio::test]
    #[ignore]
    async fn test_or_count() {
        let count = OrTestUser::query()
            .or_where(|q| q
                .where_eq("status", "active")
                .where_eq("status", "pending")
            )
            .count()
            .await
            .unwrap();

        assert!(count > 0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_batch_update_with_or() {
        let affected = OrTestUser::update_all()
            .set("status", "updated")
            .where_eq("role", "user")
            .or_where_eq("age", 25)
            .execute()
            .await
            .unwrap();

        assert!(affected > 0);
    }
    */
}

// ============================================================================
// Complex scenario tests
// ============================================================================

#[cfg(test)]
mod complex_scenario_tests {
    use super::*;

    /// Test representing: Find users who are:
    /// - Active AND
    /// - Either (admin OR moderator) AND
    /// - Either (from Engineering OR age > 30)
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

    /// Test representing search with multiple optional filters:
    /// Search where email contains @gmail.com OR @yahoo.com OR @outlook.com
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

    /// Test price range search:
    /// Find products in cheap (0-50) OR premium (200-500) price ranges
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

    /// Test null OR value conditions:
    /// Find users where profile_picture IS NULL OR profile_picture = 'default.png'
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
        let group = OrGroup::new().nested_or(|inner| inner); // Empty nested group

        // Empty nested group should still be added but with 0 conditions
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
        // Complex branch: (role = 'admin' AND active = true AND age > 18 AND verified_at IS NOT NULL)
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

// ============================================================================
// Integration tests for Fluent OR API with database queries
// ============================================================================

#[cfg(test)]
mod fluent_or_integration_tests {
    use std::time::Duration;
    use tideorm::prelude::*;
    use tideorm::{Database, TideConfig};

    fn test_database_url() -> &'static str {
        let _ = dotenvy::dotenv();
        // Use environment variable or default
        Box::leak(
            std::env::var("POSTGRESQL_DATABASE_URL")
                .unwrap_or_else(|_| {
                    "postgres://postgres:postgres@localhost:5432/test_tide_orm".to_string()
                })
                .into_boxed_str(),
        )
    }

    // Test model for OR clause integration tests
    #[derive(Model, PartialEq)]
    #[tideorm(table = "or_test_users")]
    pub struct OrTestUser {
        #[tideorm(primary_key, auto_increment)]
        pub id: i64,
        pub name: String,
        pub email: String,
        pub role: String,
        pub department: String,
        pub age: i32,
        pub active: bool,
        pub verified: bool,
    }

    /// Single integration test that runs all OR clause scenarios sequentially
    /// This avoids issues with parallel test execution and shared database state
    #[tokio::test]
    async fn test_all_fluent_or_scenarios() {
        println!("\n========================================");
        println!(" Fluent OR API Integration Tests");
        println!("========================================\n");

        // =====================================================================
        // SETUP
        // =====================================================================

        TideConfig::init()
            .database(test_database_url())
            .max_connections(10)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(30))
            .connect()
            .await
            .expect("Failed to connect to database");

        let _ = Database::execute("DROP TABLE IF EXISTS or_test_users CASCADE").await;

        Database::execute(
            r#"
            CREATE TABLE or_test_users (
                id BIGSERIAL PRIMARY KEY,
                name VARCHAR(255) NOT NULL,
                email VARCHAR(255) NOT NULL,
                role VARCHAR(50) NOT NULL,
                department VARCHAR(100) NOT NULL,
                age INTEGER NOT NULL,
                active BOOLEAN NOT NULL DEFAULT true,
                verified BOOLEAN NOT NULL DEFAULT false
            )
        "#,
        )
        .await
        .expect("Failed to create table");

        // Insert test data with various combinations
        let test_users = vec![
            // Active admins
            (
                "Alice Admin",
                "alice@example.com",
                "admin",
                "Engineering",
                30,
                true,
                true,
            ),
            (
                "Bob Admin",
                "bob@example.com",
                "admin",
                "Marketing",
                35,
                true,
                false,
            ),
            (
                "Carl Admin Inactive",
                "carl@example.com",
                "admin",
                "Sales",
                28,
                false,
                true,
            ),
            // Active moderators
            (
                "Diana Mod",
                "diana@example.com",
                "moderator",
                "Support",
                25,
                true,
                true,
            ),
            (
                "Eve Mod Inactive",
                "eve@example.com",
                "moderator",
                "HR",
                40,
                false,
                true,
            ),
            (
                "Frank Mod",
                "frank@example.com",
                "moderator",
                "Engineering",
                32,
                true,
                false,
            ),
            // Editors
            (
                "Grace Editor",
                "grace@example.com",
                "editor",
                "Marketing",
                27,
                true,
                true,
            ),
            (
                "Henry Editor Inactive",
                "henry@example.com",
                "editor",
                "Sales",
                45,
                false,
                false,
            ),
            (
                "Ivy Editor",
                "ivy@example.com",
                "editor",
                "Engineering",
                29,
                true,
                true,
            ),
            // Regular users
            (
                "Jack User",
                "jack@example.com",
                "user",
                "Support",
                22,
                true,
                false,
            ),
            (
                "Kate User Inactive",
                "kate@example.com",
                "user",
                "HR",
                38,
                false,
                true,
            ),
            (
                "Leo User",
                "leo@example.com",
                "user",
                "Engineering",
                31,
                true,
                true,
            ),
            // Guests
            (
                "Mike Guest",
                "mike@example.com",
                "guest",
                "Marketing",
                24,
                true,
                false,
            ),
            (
                "Nancy Guest Inactive",
                "nancy@example.com",
                "guest",
                "Sales",
                50,
                false,
                false,
            ),
        ];

        for (name, email, role, dept, age, active, verified) in test_users {
            let user = OrTestUser {
                id: 0,
                name: name.to_string(),
                email: email.to_string(),
                role: role.to_string(),
                department: dept.to_string(),
                age,
                active,
                verified,
            };
            let _ = OrTestUser::create(user).await;
        }

        println!(" Setup complete: 14 test users created\n");

        // =====================================================================
        // TEST 1: Simple OR with multiple roles
        // =====================================================================

        println!("TEST 1: Simple OR with multiple roles");
        println!("--------------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .or_where_eq("role", "moderator")
            .or_where_eq("role", "editor")
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        // Should return all admins, moderators, and editors (9 users)
        assert_eq!(
            results.len(),
            9,
            "Expected 9 users (3 admin + 3 moderator + 3 editor)"
        );

        for user in &results {
            assert!(
                user.role == "admin" || user.role == "moderator" || user.role == "editor",
                "Unexpected role: {}",
                user.role
            );
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 2: User's exact example - OR with AND conditions
        // =====================================================================

        println!("TEST 2: User's exact example - OR with AND conditions");
        println!("------------------------------------------------------");
        println!("Pattern: where_eq(active, true)");
        println!("         .begin_or()");
        println!("           .or_where_eq(role, admin).and_where_eq(active, true)");
        println!("           .or_where_eq(role, moderator).and_where_eq(active, false)");
        println!("           .or_where_eq(role, editor)");
        println!("         .end_or()");

        let query = OrTestUser::query()
            .where_eq("active", true)
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_eq("active", true)
            .or_where_eq("role", "moderator")
            .and_where_eq("active", false)
            .or_where_eq("role", "editor")
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        for user in &results {
            println!(
                "  - {} (role: {}, active: {})",
                user.name, user.role, user.active
            );
        }

        // Verify results - outer active=true filters, then OR conditions
        // Expected: Active admins + Active editors
        // (moderator AND active=false) can't match because outer active=true
        for user in &results {
            assert!(user.active, "User {} should be active", user.name);
            assert!(
                user.role == "admin" || user.role == "editor",
                "User {} has unexpected role {}",
                user.name,
                user.role
            );
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 3: Privileged active users - business logic
        // =====================================================================

        println!("TEST 3: Privileged active users - business logic");
        println!("-------------------------------------------------");

        let query = OrTestUser::query()
            .where_eq("active", true)
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_eq("verified", true)
            .or_where_eq("role", "moderator")
            .and_where_eq("department", "Engineering")
            .or_where_eq("role", "editor")
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        for user in &results {
            println!(
                "  - {} (role: {}, dept: {}, verified: {})",
                user.name, user.role, user.department, user.verified
            );
        }

        // Expected:
        // - Active verified admins: Alice = 1
        // - Active moderators in Engineering: Frank = 1
        // - Active editors: Grace, Ivy = 2
        // Total = 4
        assert_eq!(results.len(), 4, "Expected 4 privileged active users");

        for user in &results {
            assert!(user.active, "Must be active");
            let matches_criteria = (user.role == "admin" && user.verified)
                || (user.role == "moderator" && user.department == "Engineering")
                || user.role == "editor";
            assert!(
                matches_criteria,
                "User {} doesn't match criteria",
                user.name
            );
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 4: Age-based OR conditions
        // =====================================================================

        println!("TEST 4: Age-based OR conditions");
        println!("--------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_lt("age", 30)
            .or_where_eq("role", "moderator")
            .and_where_gt("age", 35)
            .or_where_eq("role", "editor")
            .and_where_eq("verified", true)
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        for user in &results {
            println!(
                "  - {} (role: {}, age: {}, verified: {})",
                user.name, user.role, user.age, user.verified
            );
        }

        // Verify
        for user in &results {
            let matches = (user.role == "admin" && user.age < 30)
                || (user.role == "moderator" && user.age > 35)
                || (user.role == "editor" && user.verified);
            assert!(matches, "User {} doesn't match age criteria", user.name);
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 5: Multiple AND conditions per branch
        // =====================================================================

        println!("TEST 5: Multiple AND conditions per branch");
        println!("-------------------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_eq("active", true)
            .and_where_eq("verified", true)
            .and_where_gt("age", 25)
            .or_where_eq("role", "moderator")
            .and_where_eq("active", true)
            .and_where_eq("department", "Engineering")
            .or_where_eq("role", "editor")
            .and_where_eq("verified", true)
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        for user in &results {
            println!(
                "  - {} (role: {}, active: {}, verified: {}, age: {}, dept: {})",
                user.name, user.role, user.active, user.verified, user.age, user.department
            );
        }

        for user in &results {
            let branch1 = user.role == "admin" && user.active && user.verified && user.age > 25;
            let branch2 =
                user.role == "moderator" && user.active && user.department == "Engineering";
            let branch3 = user.role == "editor" && user.verified;
            assert!(
                branch1 || branch2 || branch3,
                "User {} doesn't match any branch",
                user.name
            );
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 6: OR with IN conditions
        // =====================================================================

        println!("TEST 6: OR with IN conditions");
        println!("------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_in("department", vec!["Engineering", "Marketing"])
            .or_where_in("department", vec!["HR", "Support"])
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        for user in &results {
            println!(
                "  - {} (role: {}, dept: {})",
                user.name, user.role, user.department
            );
        }

        for user in &results {
            let matches = (user.role == "admin"
                && (user.department == "Engineering" || user.department == "Marketing"))
                || (user.department == "HR" || user.department == "Support");
            assert!(matches, "User {} doesn't match IN criteria", user.name);
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 7: OR with BETWEEN
        // =====================================================================

        println!("TEST 7: OR with BETWEEN conditions");
        println!("-----------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_between("age", 25, 35)
            .or_where_eq("role", "moderator")
            .and_where_between("age", 30, 45)
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        for user in &results {
            println!("  - {} (role: {}, age: {})", user.name, user.role, user.age);
        }

        for user in &results {
            let matches = (user.role == "admin" && user.age >= 25 && user.age <= 35)
                || (user.role == "moderator" && user.age >= 30 && user.age <= 45);
            assert!(
                matches,
                "User {} (age: {}) doesn't match BETWEEN criteria",
                user.name, user.age
            );
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 8: Count with OR conditions
        // =====================================================================

        println!("TEST 8: Count with OR conditions");
        println!("---------------------------------");

        let query = OrTestUser::query()
            .where_eq("active", true)
            .begin_or()
            .or_where_eq("role", "admin")
            .or_where_eq("role", "moderator")
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let count = query.count().await.expect("Count failed");
        println!("Count: {}", count);

        // Verify by getting actual results
        let verify_query = OrTestUser::query()
            .where_eq("active", true)
            .begin_or()
            .or_where_eq("role", "admin")
            .or_where_eq("role", "moderator")
            .end_or();

        let results = verify_query.get().await.expect("Query failed");
        assert_eq!(
            count as usize,
            results.len(),
            "Count should match actual results"
        );
        println!(" PASSED\n");

        // =====================================================================
        // TEST 9: First with OR conditions
        // =====================================================================

        println!("TEST 9: First with OR conditions");
        println!("---------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .or_where_eq("role", "moderator")
            .end_or()
            .order_by("name", Order::Asc);

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let first = query.first().await.expect("First failed");
        assert!(first.is_some(), "Should find at least one user");

        if let Some(user) = first {
            println!("First user: {} (role: {})", user.name, user.role);
            assert!(user.role == "admin" || user.role == "moderator");
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 10: Single branch OR
        // =====================================================================

        println!("TEST 10: Single branch OR");
        println!("--------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        assert_eq!(results.len(), 3, "Should find 3 admins");
        for user in &results {
            assert_eq!(user.role, "admin", "User should be admin");
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 11: OR with ORDER BY and LIMIT
        // =====================================================================

        println!("TEST 11: OR with ORDER BY and LIMIT");
        println!("------------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .or_where_eq("role", "moderator")
            .or_where_eq("role", "editor")
            .end_or()
            .order_by("age", Order::Desc)
            .limit(5);

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users (limited to 5)", results.len());

        assert!(results.len() <= 5, "Should return at most 5 users");

        for i in 1..results.len() {
            assert!(
                results[i - 1].age >= results[i].age,
                "Results should be ordered by age descending"
            );
        }

        for user in &results {
            println!("  - {} (role: {}, age: {})", user.name, user.role, user.age);
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 12: Empty begin_or().end_or()
        // =====================================================================

        println!("TEST 12: Empty begin_or().end_or()");
        println!("-----------------------------------");

        let query = OrTestUser::query()
            .where_eq("active", true)
            .begin_or()
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        // Should return all active users (empty OR shouldn't affect query)
        for user in &results {
            assert!(user.active, "User should be active");
        }
        println!(" PASSED\n");

        // =====================================================================
        // TEST 13: SQL structure verification
        // =====================================================================

        println!("TEST 13: SQL structure verification");
        println!("------------------------------------");

        let query = OrTestUser::query()
            .where_eq("active", true)
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_eq("verified", true)
            .or_where_eq("role", "moderator")
            .and_where_gt("age", 30)
            .end_or();

        let sql = query.build_sql_preview();
        println!("Full SQL: {}", sql);

        let sql_lower = sql.to_lowercase();

        assert!(sql_lower.contains("select"), "SQL should contain SELECT");
        assert!(sql_lower.contains("from"), "SQL should contain FROM");
        assert!(sql_lower.contains("where"), "SQL should contain WHERE");
        assert!(
            sql_lower.contains("active"),
            "SQL should contain active column"
        );
        assert!(sql_lower.contains("role"), "SQL should contain role column");
        println!(" PASSED\n");

        // =====================================================================
        // TEST 14: OR with LIKE
        // =====================================================================

        println!("TEST 14: OR with LIKE conditions");
        println!("---------------------------------");

        let query = OrTestUser::query()
            .begin_or()
            .or_where_eq("role", "admin")
            .and_where_like("name", "A%")
            .or_where_like("email", "%example%")
            .end_or();

        let sql = query.build_sql_preview();
        println!("SQL: {}", sql);

        let results = query.get().await.expect("Query failed");
        println!("Results: {} users", results.len());

        // All users have @example.com emails, so all should match
        assert!(!results.is_empty(), "Should find at least one user");

        for user in &results {
            let matches = (user.role == "admin" && user.name.starts_with("A"))
                || user.email.contains("example");
            assert!(matches, "User {} doesn't match LIKE criteria", user.name);
        }
        println!(" PASSED\n");

        // =====================================================================
        // SUMMARY
        // =====================================================================

        println!("========================================");
        println!(" ALL 14 TESTS PASSED!");
        println!("========================================\n");

        // Cleanup
        let _ = Database::execute("DROP TABLE IF EXISTS or_test_users CASCADE").await;
    }
}
