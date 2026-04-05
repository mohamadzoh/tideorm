use super::*;

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
