use super::*;
use serde_json::json;

#[test]
fn object_members_match_under_their_own_key_in_parameter_order() {
    let (sql, values) = sqlite_json_contains("\"doc\"", &json!({"a": 1, "b": "x"}));
    assert_eq!(
        sql,
        "(json_type(\"doc\") = 'object' \
         AND EXISTS (SELECT 1 FROM json_each(\"doc\") AS tide_json_1 WHERE tide_json_1.key = ? \
         AND (tide_json_1.type IN ('integer', 'real') AND tide_json_1.atom = ?)) \
         AND EXISTS (SELECT 1 FROM json_each(\"doc\") AS tide_json_2 WHERE tide_json_2.key = ? \
         AND (tide_json_2.type = 'text' AND tide_json_2.atom = ?)))"
    );
    assert_eq!(
        values,
        vec![
            Value::from("a".to_string()),
            Value::from(1i64),
            Value::from("b".to_string()),
            Value::from("x".to_string()),
        ]
    );
}

#[test]
fn array_elements_are_each_matched_by_some_element() {
    let (sql, values) = sqlite_json_contains("\"tags\"", &json!(["x", true]));
    assert_eq!(
        sql,
        "(json_type(\"tags\") = 'array' \
         AND EXISTS (SELECT 1 FROM json_each(\"tags\") AS tide_json_1 WHERE (tide_json_1.type = 'text' AND tide_json_1.atom = ?)) \
         AND EXISTS (SELECT 1 FROM json_each(\"tags\") AS tide_json_2 WHERE tide_json_2.type = 'true'))"
    );
    assert_eq!(values, vec![Value::from("x".to_string())]);
}

#[test]
fn a_scalar_is_an_element_of_a_top_level_array_but_not_of_a_nested_one() {
    let (top, values) = sqlite_json_contains("\"doc\"", &json!(5));
    assert!(
        top.contains("OR (json_type(\"doc\") = 'array' AND EXISTS"),
        "{top}"
    );
    assert_eq!(values, vec![Value::from(5i64), Value::from(5i64)]);

    let (nested, _) = sqlite_json_contains("\"doc\"", &json!({"a": 5}));
    assert!(!nested.contains(" OR "), "{nested}");
}

#[test]
fn empty_candidates_only_check_the_container_type() {
    assert_eq!(
        sqlite_json_contains("\"doc\"", &json!({})),
        ("(json_type(\"doc\") = 'object')".to_string(), vec![])
    );
    assert_eq!(
        sqlite_json_contains("\"doc\"", &json!([])),
        ("(json_type(\"doc\") = 'array')".to_string(), vec![])
    );
}

#[test]
fn contained_by_places_every_member_under_a_key_of_the_container() {
    let (sql, values) = sqlite_json_contained_by("\"doc\"", &json!({"a": 1, "b": "x"}));
    assert_eq!(
        sql,
        "(json_type(\"doc\") = 'object' \
         AND NOT EXISTS (SELECT 1 FROM json_each(\"doc\") AS tide_json_1 WHERE NOT \
         ((tide_json_1.key = ? AND (tide_json_1.type IN ('integer', 'real') AND tide_json_1.atom = ?)) \
         OR (tide_json_1.key = ? AND (tide_json_1.type = 'text' AND tide_json_1.atom = ?)))))"
    );
    assert_eq!(
        values,
        vec![
            Value::from("a".to_string()),
            Value::from(1i64),
            Value::from("b".to_string()),
            Value::from("x".to_string()),
        ]
    );
}

#[test]
fn contained_by_an_empty_container_needs_an_empty_document() {
    let (sql, values) = sqlite_json_contained_by("\"doc\"", &json!({}));
    assert!(sql.ends_with("WHERE NOT (0)))"), "{sql}");
    assert!(values.is_empty());
}

#[test]
fn contained_by_a_top_level_array_also_takes_one_of_its_scalars() {
    let (top, values) = sqlite_json_contained_by("\"doc\"", &json!([5, [6]]));
    assert!(
        top.ends_with(
            " OR ((json_type(\"doc\") IN ('integer', 'real') AND json_extract(\"doc\", '$') = ?)))"
        ),
        "{top}"
    );
    assert_eq!(
        values,
        vec![Value::from(5i64), Value::from(6i64), Value::from(5i64)]
    );

    let (nested, _) = sqlite_json_contained_by("\"doc\"", &json!({"a": [5]}));
    assert!(!nested.contains("json_extract"), "{nested}");
}
