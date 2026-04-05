use serde_json::json;
use tideorm::types::*;

#[test]
fn test_json_type_casting() {
    let json_value = json!({"key": "value", "number": 42});

    let json_data: Json = json_value.clone();
    assert_eq!(json_data["key"], "value");
    assert_eq!(json_data["number"], 42);

    let jsonb_data: Jsonb = json_value.clone();
    assert_eq!(jsonb_data["key"], "value");
    assert_eq!(jsonb_data["number"], 42);
}

#[test]
fn test_array_types() {
    let int_array: IntArray = vec![1, 2, 3, 4, 5];
    assert_eq!(int_array.len(), 5);
    assert_eq!(int_array[0], 1);

    let text_array: TextArray = vec!["hello".to_string(), "world".to_string()];
    assert_eq!(text_array.len(), 2);
    assert_eq!(text_array[0], "hello");

    let bool_array: BoolArray = vec![true, false, true];
    assert_eq!(bool_array.len(), 3);
    assert!(bool_array[0]);

    let float_array: FloatArray = vec![1.1, 2.2, 3.3];
    assert_eq!(float_array.len(), 3);
    assert_eq!(float_array[0], 1.1);

    let json_array: JsonArray = vec![json!({"id": 1}), json!({"id": 2})];
    assert_eq!(json_array.len(), 2);
    assert_eq!(json_array[0]["id"], 1);
}

#[test]
fn test_array_castable_implementation() {
    use tideorm::types::Castable;

    let json_array = json!(["hello", "world"]);
    let result: Result<Vec<String>, String> = Castable::from_json(&json_array);
    assert!(result.is_ok());
    let vec = result.unwrap();
    assert_eq!(vec, vec!["hello".to_string(), "world".to_string()]);

    let json_int_array = json!([1, 2, 3]);
    let result: Result<Vec<i32>, String> = Castable::from_json(&json_int_array);
    assert!(result.is_ok());
    let vec = result.unwrap();
    assert_eq!(vec, vec![1, 2, 3]);
}
