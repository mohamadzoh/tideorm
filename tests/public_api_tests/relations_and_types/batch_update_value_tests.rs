use serde_json::json;
use tideorm::model::UpdateValue;

#[test]
fn test_update_value_variants() {
    // Test all UpdateValue variants can be created
    let _value = UpdateValue::Value(json!("hello"));
    let _trusted_raw = UpdateValue::UnsafeRaw("NOW()".to_string());
    let _inc = UpdateValue::Increment(5);
    let _dec = UpdateValue::Decrement(3);
    let _mul = UpdateValue::Multiply(2.5);
    let _div = UpdateValue::Divide(4.0);
    let _append = UpdateValue::ArrayAppend(json!("item"));
    let _remove = UpdateValue::ArrayRemove(json!("item"));
    let _json_set = UpdateValue::JsonSet("$.path".to_string(), json!("value"));
    let _coalesce = UpdateValue::Coalesce(json!("default"));
}

#[test]
fn test_update_value_value() {
    let value = UpdateValue::Value(json!("hello"));
    match value {
        UpdateValue::Value(v) => assert_eq!(v, json!("hello")),
        _ => panic!("Expected UpdateValue::Value"),
    }
}

#[test]
fn test_update_value_increment() {
    let value = UpdateValue::Increment(5);
    match value {
        UpdateValue::Increment(n) => assert_eq!(n, 5),
        _ => panic!("Expected UpdateValue::Increment"),
    }
}

#[test]
fn test_update_value_decrement() {
    let value = UpdateValue::Decrement(3);
    match value {
        UpdateValue::Decrement(n) => assert_eq!(n, 3),
        _ => panic!("Expected UpdateValue::Decrement"),
    }
}

#[test]
fn test_update_value_multiply() {
    let value = UpdateValue::Multiply(2.5);
    match value {
        UpdateValue::Multiply(n) => assert!((n - 2.5).abs() < f64::EPSILON),
        _ => panic!("Expected UpdateValue::Multiply"),
    }
}

#[test]
fn test_update_value_divide() {
    let value = UpdateValue::Divide(4.0);
    match value {
        UpdateValue::Divide(n) => assert!((n - 4.0).abs() < f64::EPSILON),
        _ => panic!("Expected UpdateValue::Divide"),
    }
}

#[test]
fn test_update_value_unsafe_raw() {
    let value = UpdateValue::UnsafeRaw("NOW()".to_string());
    match value {
        UpdateValue::UnsafeRaw(s) => assert_eq!(s, "NOW()"),
        _ => panic!("Expected UpdateValue::UnsafeRaw"),
    }
}

#[test]
fn test_update_value_array_append() {
    let value = UpdateValue::ArrayAppend(json!("new_item"));
    match value {
        UpdateValue::ArrayAppend(v) => assert_eq!(v, json!("new_item")),
        _ => panic!("Expected UpdateValue::ArrayAppend"),
    }
}

#[test]
fn test_update_value_array_remove() {
    let value = UpdateValue::ArrayRemove(json!("old_item"));
    match value {
        UpdateValue::ArrayRemove(v) => assert_eq!(v, json!("old_item")),
        _ => panic!("Expected UpdateValue::ArrayRemove"),
    }
}

#[test]
fn test_update_value_json_set() {
    let value = UpdateValue::JsonSet("$.path".to_string(), json!("value"));
    match value {
        UpdateValue::JsonSet(path, val) => {
            assert_eq!(path, "$.path");
            assert_eq!(val, json!("value"));
        }
        _ => panic!("Expected UpdateValue::JsonSet"),
    }
}

#[test]
fn test_update_value_coalesce() {
    let value = UpdateValue::Coalesce(json!("default"));
    match value {
        UpdateValue::Coalesce(v) => assert_eq!(v, json!("default")),
        _ => panic!("Expected UpdateValue::Coalesce"),
    }
}

#[test]
fn test_update_value_clone() {
    let original = UpdateValue::Increment(10);
    let cloned = original.clone();
    match cloned {
        UpdateValue::Increment(n) => assert_eq!(n, 10),
        _ => panic!("Expected UpdateValue::Increment"),
    }
}

#[test]
fn test_update_value_debug() {
    let value = UpdateValue::UnsafeRaw("test_expr".to_string());
    let debug_str = format!("{:?}", value);
    assert!(debug_str.contains("UnsafeRaw"));
    assert!(debug_str.contains("test_expr"));
}
