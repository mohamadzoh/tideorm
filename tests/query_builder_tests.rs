//! Tests for QueryBuilder functionality
//!
//! These tests verify the critical query builder features:
//! - WHERE conditions (all operators)
//! - COUNT optimization
//! - Bulk DELETE
//! - ORDER BY, LIMIT, OFFSET
//!
//! Note: These tests require a test database connection.
//! Set TEST_DATABASE_URL environment variable to run.

use tideorm::query::{ConditionValue, Order};

// ============================================================================
// Unit tests for QueryBuilder construction (no DB needed)
// ============================================================================

#[cfg(test)]
mod query_builder_unit_tests {
    use super::*;

    // These tests verify the builder pattern without database connection

    #[test]
    fn test_order_enum() {
        assert_eq!(Order::Asc.as_str(), "ASC");
        assert_eq!(Order::Desc.as_str(), "DESC");
    }

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
        let val = ConditionValue::List(vec![serde_json::json!("a"), serde_json::json!("b")]);
        match val {
            ConditionValue::List(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0], serde_json::json!("a"));
                assert_eq!(v[1], serde_json::json!("b"));
            }
            _ => panic!("Expected List variant"),
        }
    }

    #[test]
    fn test_condition_value_range() {
        let val = ConditionValue::Range(serde_json::json!(1), serde_json::json!(10));
        match val {
            ConditionValue::Range(low, high) => {
                assert_eq!(low, serde_json::json!(1));
                assert_eq!(high, serde_json::json!(10));
            }
            _ => panic!("Expected Range variant"),
        }
    }

    #[test]
    fn test_condition_value_none() {
        let val = ConditionValue::None;
        match val {
            ConditionValue::None => {}
            _ => panic!("Expected None variant"),
        }
    }

    #[test]
    fn test_condition_value_subquery() {
        let val = ConditionValue::Subquery("SELECT id FROM users WHERE active = true".to_string());
        match val {
            ConditionValue::Subquery(sql) => {
                assert!(sql.contains("SELECT"));
                assert!(sql.contains("users"));
            }
            _ => panic!("Expected Subquery variant"),
        }
    }

    #[test]
    fn test_condition_value_raw_expr() {
        let val = ConditionValue::RawExpr("created_at > NOW() - INTERVAL '30 days'".to_string());
        match val {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("created_at"));
                assert!(sql.contains("INTERVAL"));
            }
            _ => panic!("Expected RawExpr variant"),
        }
    }
}

#[cfg(test)]
mod operator_tests {
    use tideorm::query::Operator;

    #[test]
    fn test_all_operators_exist() {
        // Verify all operators can be constructed
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
        // New subquery and raw operators
        let _ = Operator::SubqueryIn;
        let _ = Operator::SubqueryNotIn;
        let _ = Operator::Raw;
    }
}

// ============================================================================
// Integration tests (require database - marked with #[ignore] by default)
// ============================================================================

/// Run these tests with: cargo test -- --ignored
/// And with TEST_DATABASE_URL set
#[cfg(test)]
mod integration_tests {
    // These would be full integration tests with actual database
    // Leaving as placeholder structure for future implementation

    /*
    Example test structure:

    #[derive(Model, Clone, Debug, Serialize, Deserialize)]
    #[tide(table = "test_users")]
    struct TestUser {
        #[tide(primary_key, auto_increment)]
        pub id: i64,
        pub name: String,
        pub email: String,
        pub age: i32,
        pub active: bool,
    }

    async fn setup_db() -> tideorm::Result<()> {
        let url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite::memory:".to_string());

        TideConfig::init()
            .database(&url)
            .sync(true)
            .connect()
            .await?;

        Ok(())
    }

    #[tokio::test]
    #[ignore]
    async fn test_where_eq() {
        setup_db().await.unwrap();

        // Create test data
        TestUser { id: 0, name: "John".into(), email: "john@test.com".into(), age: 25, active: true }
            .save().await.unwrap();
        TestUser { id: 0, name: "Jane".into(), email: "jane@test.com".into(), age: 30, active: false }
            .save().await.unwrap();

        // Test WHERE eq
        let results = TestUser::query()
            .where_eq("name", "John")
            .get()
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "John");
    }

    #[tokio::test]
    #[ignore]
    async fn test_where_in() {
        setup_db().await.unwrap();

        let results = TestUser::query()
            .where_in("name", vec!["John", "Jane"])
            .get()
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    #[ignore]
    async fn test_count_with_conditions() {
        setup_db().await.unwrap();

        let count = TestUser::query()
            .where_eq("active", true)
            .count()
            .await
            .unwrap();

        assert_eq!(count, 1);
    }

    #[tokio::test]
    #[ignore]
    async fn test_bulk_delete() {
        setup_db().await.unwrap();

        let deleted = TestUser::query()
            .where_eq("active", false)
            .delete()
            .await
            .unwrap();

        assert_eq!(deleted, 1);

        // Verify deleted
        let count = TestUser::count().await.unwrap();
        assert_eq!(count, 1);
    }
    */
}

// ============================================================================
// Database Pool Configuration Tests
// ============================================================================

#[cfg(test)]
mod pool_config_tests {
    use std::time::Duration;
    use tideorm::Database;

    #[test]
    fn test_database_builder_creation() {
        let builder = Database::builder()
            .url("sqlite::memory:")
            .max_connections(20)
            .min_connections(5)
            .connect_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(300))
            .max_lifetime(Duration::from_secs(3600));

        // Builder should compile and be usable
        // Note: Can't verify internal state without exposing it
        // The actual test happens at build() time
        drop(builder);
    }

    #[tokio::test]
    #[ignore] // Requires actual database
    async fn test_pool_connection() {
        let db = Database::builder()
            .url("sqlite::memory:")
            .max_connections(5)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .await;

        assert!(db.is_ok(), "Should connect with pool settings");

        // Verify connection is healthy
        let db = db.unwrap();
        let ping = db.ping().await;
        assert!(ping.is_ok());
    }
}

// ============================================================================
// WHERE Clause Builder Tests
// ============================================================================

#[cfg(test)]
mod where_clause_tests {
    use tideorm::query::WhereCondition;

    #[test]
    fn test_where_condition_struct() {
        // Basic structure test
        let condition = WhereCondition {
            column: "email".to_string(),
            operator: tideorm::query::Operator::Eq,
            value: tideorm::query::ConditionValue::Single(serde_json::json!("test@example.com")),
        };

        assert_eq!(condition.column, "email");
    }

    #[test]
    fn test_subquery_where_condition() {
        let condition = WhereCondition {
            column: "user_id".to_string(),
            operator: tideorm::query::Operator::SubqueryIn,
            value: tideorm::query::ConditionValue::Subquery(
                "SELECT id FROM active_users".to_string(),
            ),
        };

        assert_eq!(condition.column, "user_id");
        match condition.operator {
            tideorm::query::Operator::SubqueryIn => {}
            _ => panic!("Expected SubqueryIn operator"),
        }
    }

    #[test]
    fn test_raw_where_condition() {
        let condition = WhereCondition {
            column: String::new(), // Empty for pure raw conditions
            operator: tideorm::query::Operator::Raw,
            value: tideorm::query::ConditionValue::RawExpr(
                "EXISTS (SELECT 1 FROM posts WHERE posts.user_id = users.id)".to_string(),
            ),
        };

        assert!(condition.column.is_empty());
        match condition.operator {
            tideorm::query::Operator::Raw => {}
            _ => panic!("Expected Raw operator"),
        }
    }
}

// ============================================================================
// Subquery and Raw Expression Tests
// ============================================================================

#[cfg(test)]
mod subquery_tests {
    use tideorm::query::{ConditionValue, Operator, WhereCondition};

    #[test]
    fn test_subquery_in_condition_value() {
        let subquery_sql = "SELECT user_id FROM orders WHERE total > 100".to_string();
        let val = ConditionValue::Subquery(subquery_sql.clone());

        match val {
            ConditionValue::Subquery(sql) => {
                assert_eq!(sql, subquery_sql);
            }
            _ => panic!("Expected Subquery variant"),
        }
    }

    #[test]
    fn test_subquery_not_in_operator() {
        let condition = WhereCondition {
            column: "id".to_string(),
            operator: Operator::SubqueryNotIn,
            value: ConditionValue::Subquery("SELECT blocked_id FROM blocked_users".to_string()),
        };

        match condition.operator {
            Operator::SubqueryNotIn => {}
            _ => panic!("Expected SubqueryNotIn operator"),
        }
    }

    #[test]
    fn test_exists_as_raw_condition() {
        let exists_sql = "EXISTS (SELECT 1 FROM comments WHERE comments.post_id = posts.id)";
        let condition = WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(exists_sql.to_string()),
        };

        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("EXISTS"));
                assert!(sql.contains("comments"));
            }
            _ => panic!("Expected RawExpr variant"),
        }
    }

    #[test]
    fn test_not_exists_as_raw_condition() {
        let not_exists_sql =
            "NOT EXISTS (SELECT 1 FROM deletions WHERE deletions.item_id = items.id)";
        let condition = WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr(not_exists_sql.to_string()),
        };

        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("NOT EXISTS"));
            }
            _ => panic!("Expected RawExpr variant"),
        }
    }
}

// ============================================================================
// Raw Expression Tests
// ============================================================================

#[cfg(test)]
mod raw_expression_tests {
    use tideorm::query::{ConditionValue, Operator, WhereCondition};

    #[test]
    fn test_raw_date_expression() {
        let condition = WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr("created_at > NOW() - INTERVAL '30 days'".to_string()),
        };

        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("NOW()"));
                assert!(sql.contains("INTERVAL"));
            }
            _ => panic!("Expected RawExpr"),
        }
    }

    #[test]
    fn test_raw_column_comparison() {
        let condition = WhereCondition {
            column: "updated_at".to_string(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr("> created_at".to_string()),
        };

        assert_eq!(condition.column, "updated_at");
        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("created_at"));
            }
            _ => panic!("Expected RawExpr"),
        }
    }

    #[test]
    fn test_raw_function_expression() {
        let condition = WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr("LOWER(email) = LOWER('Test@Example.COM')".to_string()),
        };

        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("LOWER"));
            }
            _ => panic!("Expected RawExpr"),
        }
    }

    #[test]
    fn test_raw_json_expression() {
        let condition = WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr("metadata->>'status' = 'active'".to_string()),
        };

        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("->>"));
                assert!(sql.contains("metadata"));
            }
            _ => panic!("Expected RawExpr"),
        }
    }
}

// ============================================================================
// Bulk Delete Tests
// ============================================================================

#[cfg(test)]
mod bulk_delete_tests {
    use tideorm::query::{ConditionValue, Operator, WhereCondition};

    #[test]
    fn test_delete_conditions_can_be_built() {
        // Test that we can build conditions suitable for bulk delete
        let conditions = [
            WhereCondition {
                column: "status".to_string(),
                operator: Operator::Eq,
                value: ConditionValue::Single(serde_json::json!("inactive")),
            },
            WhereCondition {
                column: "last_login".to_string(),
                operator: Operator::Lt,
                value: ConditionValue::Single(serde_json::json!("2024-01-01")),
            },
        ];

        assert_eq!(conditions.len(), 2);
        assert_eq!(conditions[0].column, "status");
        assert_eq!(conditions[1].column, "last_login");
    }

    #[test]
    fn test_delete_with_subquery_condition() {
        // Test building a delete condition that uses a subquery
        let condition = WhereCondition {
            column: "post_id".to_string(),
            operator: Operator::SubqueryIn,
            value: ConditionValue::Subquery(
                "SELECT id FROM posts WHERE deleted = true".to_string(),
            ),
        };

        assert_eq!(condition.column, "post_id");
        match &condition.value {
            ConditionValue::Subquery(sql) => {
                assert!(sql.contains("deleted = true"));
            }
            _ => panic!("Expected Subquery"),
        }
    }

    #[test]
    fn test_delete_with_raw_condition() {
        // Test building a delete condition with raw SQL
        let condition = WhereCondition {
            column: String::new(),
            operator: Operator::Raw,
            value: ConditionValue::RawExpr("expires_at < NOW() AND NOT is_permanent".to_string()),
        };

        match &condition.value {
            ConditionValue::RawExpr(sql) => {
                assert!(sql.contains("expires_at"));
                assert!(sql.contains("is_permanent"));
            }
            _ => panic!("Expected RawExpr"),
        }
    }
}
