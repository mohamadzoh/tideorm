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
