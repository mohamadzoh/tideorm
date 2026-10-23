use super::*;
use serde_json::json;

#[test]
fn object_members_match_under_their_own_key_in_parameter_order() {
    let (sql, values) = sqlite_json_contains("\"doc\"", &json!({"a": 1, "b": "x"}));
    assert!(sql.starts_with("(json_type(\"doc\") = 'object' AND EXISTS"));
    assert!(sql.contains("tide_json_1.key = ?") && sql.contains("tide_json_2.key = ?"));
    assert!(sql.contains("tide_json_1.json -> tide_json_1.fullkey"));
    assert_eq!(
        values,
        vec![
            Value::from("a".to_string()),
            Value::from("1".to_string()),
            Value::from(false),
            Value::from(0i64),
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
    assert_eq!(
        values,
        vec![
            Value::from("5".to_string()),
            Value::from(false),
            Value::from(0i64),
            Value::from("5".to_string()),
            Value::from(false),
            Value::from(0i64)
        ]
    );

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
    assert!(
        sql.contains("AND NOT EXISTS (SELECT 1 FROM json_each(\"doc\") AS tide_json_1 WHERE NOT")
    );
    assert!(sql.contains("tide_json_1.key = ?"));
    assert!(sql.contains("tide_json_1.json -> tide_json_1.fullkey"));
    assert_eq!(
        values,
        vec![
            Value::from("a".to_string()),
            Value::from("1".to_string()),
            Value::from(false),
            Value::from(0i64),
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
        top.contains(" OR ((json_type(\"doc\") IN ('integer', 'real')"),
        "{top}"
    );
    assert_eq!(
        values,
        vec![
            Value::from("5".to_string()),
            Value::from(false),
            Value::from(0i64),
            Value::from("6".to_string()),
            Value::from(false),
            Value::from(0i64),
            Value::from("5".to_string()),
            Value::from(false),
            Value::from(0i64)
        ]
    );

    let (nested, _) = sqlite_json_contained_by("\"doc\"", &json!({"a": [5]}));
    assert!(!nested.contains("json_extract"), "{nested}");
}

#[test]
fn equality_needs_as_many_members_each_equal_under_its_key() {
    let (sql, values) = sqlite_json_equals("\"doc\"", &json!({"a": [1], "b": null}));
    assert!(sql.contains("(SELECT COUNT(*) FROM json_each(\"doc\")) = 2"));
    assert!(sql.contains("json_array_length(tide_json_1.value) = 1"));
    assert!(sql.contains("tide_json_2.key = 0") && sql.contains("tide_json_3.type = 'null'"));
    assert_eq!(
        values,
        vec![
            Value::from("a".to_string()),
            Value::from("1".to_string()),
            Value::from(false),
            Value::from(0i64),
            Value::from("b".to_string()),
        ]
    );
}

#[test]
fn an_element_equals_a_scalar_by_type_and_value() {
    assert_eq!(
        sqlite_json_element_equals("item", &json!("a")),
        (
            "(item.type = 'text' AND item.atom = ?)".to_string(),
            vec![Value::from("a".to_string())]
        )
    );
    assert_eq!(
        sqlite_json_element_equals("item", &json!([])),
        (
            "(item.type = 'array' AND json_array_length(item.value) = 0)".to_string(),
            vec![]
        )
    );
}

/// A number whose exponent no `i64` holds, which a request can send once
/// serde_json keeps numbers' text (`arbitrary_precision`), used to panic,
/// or to overflow the scale.
#[test]
fn a_number_past_every_exponent_has_no_scale() {
    assert_eq!(
        super::decimal_parts("-12.50e3"),
        (true, "125".to_string(), Some(2))
    );
    assert_eq!(super::decimal_parts("0.000").1, "");
    for text in [
        "1e99999999999999999999",
        "10e9223372036854775807",
        "1.5e-9223372036854775808",
    ] {
        assert_eq!(super::decimal_parts(text).2, None, "{text}");
    }
}

/// The comparison with such a number is one SQLite runs, and matches no
/// stored number, where a number with an ordinary exponent still matches.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn a_number_past_every_exponent_matches_no_stored_number() {
    let _lock = crate::test_support::global_db_lock().lock().await;
    let db = crate::test_support::install_sqlite_global(&[
        "CREATE TABLE json_numbers(doc TEXT)",
        "INSERT INTO json_numbers VALUES ('10'), ('1e5'), ('[10]')",
    ])
    .await;
    let matching = |text: &'static str| {
        let db = db.clone();
        async move {
            let mut tests = JsonTests::default();
            let predicate = tests.number_equals(&Node::document("doc"), text);
            let rows = db
                .__raw_json_with_params(
                    &format!("SELECT count(*) AS n FROM json_numbers WHERE {predicate}"),
                    tests.values,
                )
                .await
                .unwrap();
            (predicate, rows[0]["n"].as_i64().unwrap())
        }
    };

    let (predicate, found) = matching("10e9223372036854775807").await;
    assert!(
        predicate.contains("SELECT 0 FROM tide_num_normal"),
        "{predicate}"
    );
    assert_eq!(found, 0);
    assert_eq!(matching("1.0e1").await.1, 1);
    assert_eq!(matching("100000").await.1, 1);
    crate::test_support::reset_globals();
}
