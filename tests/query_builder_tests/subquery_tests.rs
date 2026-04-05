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
    let not_exists_sql = "NOT EXISTS (SELECT 1 FROM deletions WHERE deletions.item_id = items.id)";
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
