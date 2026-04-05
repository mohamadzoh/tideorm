use tideorm::query::{ConditionValue, Operator, Order, WhereCondition};

#[test]
fn test_order_as_str() {
    assert_eq!(Order::Asc.as_str(), "ASC");
    assert_eq!(Order::Desc.as_str(), "DESC");
}

#[test]
fn test_order_clone_eq() {
    let order1 = Order::Asc;
    let order2 = order1;
    assert_eq!(order1, order2);
}

#[test]
fn test_order_debug() {
    let debug = format!("{:?}", Order::Desc);
    assert_eq!(debug, "Desc");
}

#[test]
fn test_operator_variants() {
    let operators = vec![
        Operator::Eq,
        Operator::NotEq,
        Operator::Gt,
        Operator::Gte,
        Operator::Lt,
        Operator::Lte,
        Operator::Like,
        Operator::NotLike,
        Operator::In,
        Operator::NotIn,
        Operator::IsNull,
        Operator::IsNotNull,
        Operator::Between,
    ];
    assert_eq!(operators.len(), 13);
}

#[test]
fn test_condition_value_single() {
    let val = ConditionValue::Single(serde_json::json!("test"));
    if let ConditionValue::Single(v) = val {
        assert_eq!(v, serde_json::json!("test"));
    } else {
        panic!("Expected Single variant");
    }
}

#[test]
fn test_condition_value_single_numbers() {
    let val_int = ConditionValue::Single(serde_json::json!(42));
    let val_float = ConditionValue::Single(serde_json::json!(3.14));
    let val_bool = ConditionValue::Single(serde_json::json!(true));

    if let ConditionValue::Single(v) = val_int {
        assert_eq!(v.as_i64(), Some(42));
    }
    if let ConditionValue::Single(v) = val_float {
        assert_eq!(v.as_f64(), Some(3.14));
    }
    if let ConditionValue::Single(v) = val_bool {
        assert_eq!(v.as_bool(), Some(true));
    }
}

#[test]
fn test_condition_value_list() {
    let val = ConditionValue::List(vec![
        serde_json::json!("a"),
        serde_json::json!("b"),
        serde_json::json!("c"),
    ]);
    if let ConditionValue::List(v) = val {
        assert_eq!(v.len(), 3);
    } else {
        panic!("Expected List variant");
    }
}

#[test]
fn test_condition_value_range() {
    let val = ConditionValue::Range(serde_json::json!(1), serde_json::json!(100));
    if let ConditionValue::Range(low, high) = val {
        assert_eq!(low.as_i64(), Some(1));
        assert_eq!(high.as_i64(), Some(100));
    } else {
        panic!("Expected Range variant");
    }
}

#[test]
fn test_condition_value_none() {
    let val = ConditionValue::None;
    assert!(matches!(val, ConditionValue::None));
}

#[test]
fn test_where_condition_struct() {
    let condition = WhereCondition {
        column: "status".to_string(),
        operator: Operator::Eq,
        value: ConditionValue::Single(serde_json::json!("active")),
    };

    assert_eq!(condition.column, "status");
    assert!(matches!(condition.operator, Operator::Eq));
}

#[test]
fn test_where_condition_in_operator() {
    let condition = WhereCondition {
        column: "role".to_string(),
        operator: Operator::In,
        value: ConditionValue::List(vec![
            serde_json::json!("admin"),
            serde_json::json!("moderator"),
        ]),
    };

    assert_eq!(condition.column, "role");
    assert!(matches!(condition.operator, Operator::In));
}

#[test]
fn test_where_condition_between_operator() {
    let condition = WhereCondition {
        column: "age".to_string(),
        operator: Operator::Between,
        value: ConditionValue::Range(serde_json::json!(18), serde_json::json!(65)),
    };

    assert_eq!(condition.column, "age");
    assert!(matches!(condition.operator, Operator::Between));
}

#[test]
fn test_where_condition_is_null() {
    let condition = WhereCondition {
        column: "deleted_at".to_string(),
        operator: Operator::IsNull,
        value: ConditionValue::None,
    };

    assert!(matches!(condition.operator, Operator::IsNull));
    assert!(matches!(condition.value, ConditionValue::None));
}
