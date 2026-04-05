use tideorm::query::{ConditionValue, Operator, WhereCondition};

#[test]
fn test_delete_conditions_can_be_built() {
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
    let condition = WhereCondition {
        column: "post_id".to_string(),
        operator: Operator::SubqueryIn,
        value: ConditionValue::Subquery("SELECT id FROM posts WHERE deleted = true".to_string()),
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
