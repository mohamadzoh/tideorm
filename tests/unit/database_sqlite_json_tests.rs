use super::*;
use SqliteDecodedValue as V;
use serde_json::json;

#[test]
fn text_is_a_document_only_in_a_document_column() {
    for (text, document, expected) in [
        ("[1]", true, json!([1])),
        (r#"{"a":1}"#, true, json!({"a": 1})),
        ("[1]", false, json!("[1]")),
        // A scalar is text either way: `json_extract` returns string members
        // as plain text.
        ("2", true, json!("2")),
        ("not json", true, json!("not json")),
    ] {
        assert_eq!(to_json(V::Text(text.into()), document), Some(expected));
    }
}

#[test]
fn stored_values_map_to_json() {
    for (value, expected) in [
        (V::Null, json!(null)),
        (V::Bool(true), json!(true)),
        (V::Integer(7), json!(7)),
        (V::Real(1.5), json!(1.5)),
        (V::Real(f64::INFINITY), json!("inf")),
        (V::Blob(vec![1, 2]), json!([1, 2])),
    ] {
        assert_eq!(to_json(value, false), Some(expected));
    }
    let at = chrono::NaiveDate::from_ymd_opt(2024, 2, 29)
        .and_then(|date| date.and_hms_opt(10, 30, 0))
        .unwrap();
    assert_eq!(
        to_json(V::DateTime(at), false),
        Some(json!("2024-02-29T10:30:00"))
    );
}
