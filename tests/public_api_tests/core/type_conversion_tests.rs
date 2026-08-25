use serde_json::json;

#[test]
fn test_json_value_conversions() {
    let str_val = json!("hello");
    assert_eq!(str_val.as_str(), Some("hello"));

    let int_val = json!(42i64);
    assert_eq!(int_val.as_i64(), Some(42));

    let float_val = json!(3.14f64);
    assert_eq!(float_val.as_f64(), Some(3.14));

    let bool_val = json!(true);
    assert_eq!(bool_val.as_bool(), Some(true));

    let null_val = json!(null);
    assert!(null_val.is_null());

    let array_val = json!([1, 2, 3]);
    assert!(array_val.is_array());
    assert_eq!(array_val.as_array().unwrap().len(), 3);

    let object_val = json!({"key": "value"});
    assert!(object_val.is_object());
}

#[test]
fn test_json_from_primitives() {
    let from_str: serde_json::Value = "test".into();
    assert_eq!(from_str, json!("test"));

    let from_string: serde_json::Value = String::from("test").into();
    assert_eq!(from_string, json!("test"));

    let from_i32: serde_json::Value = json!(42i32);
    assert_eq!(from_i32.as_i64(), Some(42));

    let from_i64: serde_json::Value = json!(42i64);
    assert_eq!(from_i64.as_i64(), Some(42));

    let from_bool: serde_json::Value = json!(true);
    assert_eq!(from_bool.as_bool(), Some(true));
}
