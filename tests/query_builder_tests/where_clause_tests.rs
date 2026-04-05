use tideorm::query::WhereCondition;

#[test]
fn test_where_condition_struct() {
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
        value: tideorm::query::ConditionValue::Subquery("SELECT id FROM active_users".to_string()),
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
        column: String::new(),
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
