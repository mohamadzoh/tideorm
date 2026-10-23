//! JSON setters and array operations keep JSON numbers exact and tell JSON `null`
//! from SQL `NULL`.

use crate::fixtures::memory_database;
use serde_json::json;
use tideorm::prelude::*;

#[tideorm::model(table = "resume_docs")]
struct Doc {
    #[tideorm(primary_key)]
    id: i64,
    payload: Json,
    amount: i64,
}

#[tokio::test]
async fn array_remove_preserves_non_arrays_and_sql_null() {
    let db = memory_database(
        r#"CREATE TABLE resume_docs(id INTEGER PRIMARY KEY,payload TEXT,amount INTEGER);
        INSERT INTO resume_docs VALUES
        (1,'{"a":1,"b":2}',0),(2,'5',0),(3,'null',0),(4,NULL,0),
        (5,'[999,1,999,null,{"a":1}]',0),(6,'[]',0),(7,'"text"',0),(8,'true',0)"#,
    )
    .await;
    for candidate in [json!(999), json!(null)] {
        Doc::query_with(&db)
            .where_gte("id", 1)
            .update_all()
            .array_remove("payload", candidate.clone())
            .execute()
            .await
            .unwrap();
        let values = db
            .query_raw_json("SELECT payload FROM resume_docs ORDER BY id")
            .await
            .unwrap();
        for (index, expected) in [
            (0, json!(r#"{"a":1,"b":2}"#)),
            (1, json!("5")),
            (2, json!("null")),
            (3, json!(null)),
            (5, json!("[]")),
            (6, json!(r#""text""#)),
            (7, json!("true")),
        ] {
            assert_eq!(values[index]["payload"], expected);
        }
        let array: Json = serde_json::from_str(values[4]["payload"].as_str().unwrap()).unwrap();
        assert_eq!(
            array,
            if candidate.is_null() {
                json!([1,{"a":1}])
            } else {
                json!([1,null,{"a":1}])
            }
        );
    }
}

#[tokio::test]
async fn structured_setters_reject_scalar_destinations() {
    let db = memory_database("CREATE TABLE resume_docs(id INTEGER PRIMARY KEY,payload TEXT,amount INTEGER); INSERT INTO resume_docs VALUES(1,'[1,2]',10)").await;
    for operation in 0..3 {
        let update = Doc::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .set("payload", json!([99]));
        let update = match operation {
            0 => update.array_remove("amount", 999),
            1 => update.array_append("amount", 999),
            _ => update.json_set("amount", "$.a", 1),
        };
        assert!(update.execute().await.is_err());
        let row = Doc::find_with(1, &db).await.unwrap().unwrap();
        assert_eq!((row.amount, row.payload), (10, json!([1, 2])));
    }
    Doc::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .array_remove("payload", 2)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Doc::find_with(1, &db).await.unwrap().unwrap().payload,
        json!([1])
    );
}

#[tokio::test]
async fn json_decimal_tokens_compare_exactly_for_reads_deletes_and_removals() {
    let db = memory_database("CREATE TABLE resume_docs(id INTEGER PRIMARY KEY,payload TEXT,amount INTEGER); INSERT INTO resume_docs VALUES(1,'[9223372036854775808]',0),(2,'[9223372036854775809.0]',0),(3,'[9223372036854775808.0]',0),(4,'[9.223372036854775808e18]',0),(5,'[9.223372036854775809e18]',0)").await;
    let candidate = json!([9223372036854775808_u64]);
    assert_eq!(
        Doc::query_with(&db)
            .where_eq("payload", candidate.clone())
            .order_asc("id")
            .pluck::<i64>("id")
            .await
            .unwrap(),
        vec![1, 3, 4]
    );
    assert_eq!(
        Doc::query_with(&db)
            .where_json_contains("payload", candidate.clone())
            .count()
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Doc::query_with(&db)
            .where_json_contained_by("payload", candidate.clone())
            .count()
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Doc::query_with(&db)
            .where_eq("payload", candidate)
            .delete()
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        Doc::query_with(&db)
            .order_asc("id")
            .pluck::<i64>("id")
            .await
            .unwrap(),
        vec![2, 5]
    );
    db.exec_raw("DELETE FROM resume_docs; INSERT INTO resume_docs VALUES(1,'[9223372036854775808,9223372036854775809.0,7]',0)").await.unwrap();
    Doc::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .array_remove("payload", 9223372036854775808_u64)
        .execute()
        .await
        .unwrap();
    // Compare original text through SQLite, since serde_json itself rounds this decimal.
    assert_eq!(
        Doc::query_with(&db)
            .select_raw("'raw:' || CAST(payload AS TEXT) AS original_text")
            .get_json()
            .await
            .unwrap()[0]["original_text"],
        "raw:[9223372036854775809.0,7]"
    );
    assert_eq!(
        Doc::query_with(&db)
            .select_raw("json_array_length(payload) AS n")
            .get_json()
            .await
            .unwrap()[0]["n"],
        2
    );
    assert_eq!(
        Doc::query_with(&db)
            .where_eq("payload", json!([9223372036854775808_u64, 7]))
            .count()
            .await
            .unwrap(),
        0
    );
    for (raw, candidate, equal) in [
        ("[1.00e0]", json!([1]), true),
        ("[-0e999]", json!([0]), true),
        ("[0.10000000000000000001]", json!([0.1]), false),
        ("[1e-1]", json!([0.1]), true),
        (
            "[-9223372036854775809.0]",
            json!([-9223372036854775808_i64]),
            false,
        ),
        ("[18446744073709551615.0]", json!([u64::MAX]), true),
    ] {
        db.exec_raw(&format!("UPDATE resume_docs SET payload='{raw}'"))
            .await
            .unwrap();
        assert_eq!(
            Doc::query_with(&db)
                .where_eq("payload", candidate)
                .count()
                .await
                .unwrap(),
            u64::from(equal),
            "{raw}"
        );
    }
    for key in ["nul\0key", "a\"b", "a.b\"c", "a\\b", "🙂", ""] {
        let document = json!({key:u64::MAX});
        Doc::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .set("payload", document.clone())
            .execute()
            .await
            .unwrap();
        assert_eq!(
            Doc::query_with(&db)
                .where_eq("payload", document.clone())
                .count()
                .await
                .unwrap(),
            1,
            "{key:?}"
        );
        assert_eq!(
            Doc::query_with(&db)
                .where_json_contains("payload", document)
                .count()
                .await
                .unwrap(),
            1,
            "{key:?}"
        );
    }
}

#[tideorm::model(table = "review_exact_json")]
struct ExactJson {
    #[tideorm(primary_key)]
    id: i64,
    items: serde_json::Value,
}

async fn exact_json_database() -> Database {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_exact_json(id INTEGER PRIMARY KEY, items JSON)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_exact_json VALUES(1,'[18446744073709551615,1]'),(2,'[18446744073709551614,1]')").await.unwrap();
    db
}

#[tokio::test]
async fn sqlite_large_json_integer_filters_and_deletes_are_exact() {
    let db = exact_json_database().await;
    for query in [
        ExactJson::query_with(&db).where_array_contains("items", vec![u64::MAX]),
        ExactJson::query_with(&db).where_array_overlaps("items", vec![u64::MAX]),
        ExactJson::query_with(&db).where_array_contained_by("items", vec![u64::MAX, 1]),
        ExactJson::query_with(&db).where_eq("items", serde_json::json!([u64::MAX, 1])),
    ] {
        let rows = query.get().await.unwrap();
        assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), vec![1]);
    }
    // A REAL near u64::MAX must not match integers rounded to that REAL.
    assert_eq!(
        ExactJson::query_with(&db)
            .where_array_contains("items", vec![u64::MAX as f64])
            .count()
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        ExactJson::query_with(&db)
            .where_array_contains("items", vec![u64::MAX])
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(ExactJson::query_with(&db).get().await.unwrap()[0].id, 2);
}

#[tokio::test]
async fn sqlite_array_removal_keeps_original_json_numbers() {
    let db = exact_json_database().await;
    ExactJson::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .array_remove("items", 1)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        ExactJson::find_with(1, &db).await.unwrap().unwrap().items,
        serde_json::json!([u64::MAX])
    );
    assert_eq!(
        ExactJson::find_with(2, &db).await.unwrap().unwrap().items,
        serde_json::json!([u64::MAX - 1])
    );
    ExactJson::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .array_remove("items", u64::MAX)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        ExactJson::find_with(1, &db).await.unwrap().unwrap().items,
        serde_json::json!([])
    );
    assert_eq!(
        ExactJson::find_with(2, &db).await.unwrap().unwrap().items,
        serde_json::json!([u64::MAX - 1])
    );
}

#[tokio::test]
async fn nonfinite_array_values_never_become_json_null() {
    let db = exact_json_database().await;
    db.exec_raw("UPDATE review_exact_json SET items='[null,1]' WHERE id=1")
        .await
        .unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            ExactJson::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .array_remove("items", value)
                .execute()
                .await
                .is_err()
        );
        assert!(
            ExactJson::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .array_append("items", vec![value])
                .execute()
                .await
                .is_err()
        );
        assert_eq!(
            ExactJson::find_with(1, &db).await.unwrap().unwrap().items,
            serde_json::json!([null, 1])
        );
    }
    ExactJson::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .array_remove("items", serde_json::Value::Null)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        ExactJson::find_with(1, &db).await.unwrap().unwrap().items,
        serde_json::json!([1])
    );
}

#[tokio::test]
async fn json_set_rejects_nonfinite_values_and_keeps_intentional_null() {
    let db = exact_json_database().await;
    db.exec_raw("UPDATE review_exact_json SET items='{\"a\":123}' WHERE id=1")
        .await
        .unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            ExactJson::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .json_set("items", "$.a", value)
                .execute()
                .await
                .is_err()
        );
        assert!(
            ExactJson::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .json_set("items", "$.a", vec![Some(value)])
                .execute()
                .await
                .is_err()
        );
        assert_eq!(
            ExactJson::find_with(1, &db).await.unwrap().unwrap().items,
            serde_json::json!({"a":123})
        );
    }
    ExactJson::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .json_set("items", "$.a", serde_json::Value::Null)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        ExactJson::find_with(1, &db).await.unwrap().unwrap().items,
        serde_json::json!({"a":null})
    );
}

#[tokio::test]
async fn json_path_escapes_are_decoded_before_mutation() {
    let db = exact_json_database().await;
    for (path, key) in [
        (r#"$."\u0061""#, "a"),
        (r#"$['line\nbreak']"#, "line\nbreak"),
        (r#"$."\uD83D\uDE00""#, "😀"),
        (r#"$['it\'s']"#, "it's"),
        (r#"$."back\\slash""#, "back\\slash"),
    ] {
        db.exec_raw("UPDATE review_exact_json SET items='{\"a\":1,\"u0061\":2}' WHERE id=1")
            .await
            .unwrap();
        ExactJson::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .json_set("items", path, 99)
            .execute()
            .await
            .unwrap();
        let value = ExactJson::query_with(&db)
            .where_eq("id", 1)
            .pluck::<serde_json::Value>("items")
            .await
            .unwrap()
            .remove(0);
        assert_eq!(value[key], 99, "{path}: {value}");
        assert_eq!(value["u0061"], 2);
    }
    for path in [r#"$."\uZZZZ""#, r#"$."\q""#, r#"$."\uD800""#] {
        assert!(
            ExactJson::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .json_set("items", path, 0)
                .execute()
                .await
                .is_err()
        );
    }
}
