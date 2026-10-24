//! Rows read as JSON: SQLite text is a JSON document only when a JSON column,
//! a JSON function or a view over one produced it. How a statement's outputs
//! are classified is unit-tested with `sqlite_outputs`; these read real rows.

use crate::fixtures::memory_database;
use tideorm::prelude::*;

/// The `payload` of the first row `source` holds.
async fn payload(db: &Database, source: &str) -> serde_json::Value {
    db.query_raw_json(&format!("SELECT payload FROM {source}"))
        .await
        .unwrap()[0]["payload"]
        .clone()
}

#[tokio::test]
async fn json_views_keep_schema_identity_and_temp_shadowing() {
    let db = memory_database(
        "CREATE VIEW v AS SELECT json_object('main',1) AS payload; \
         CREATE TEMP VIEW v AS SELECT '{\"temp\":1}' AS payload; \
         CREATE VIEW chain AS SELECT payload FROM v; \
         CREATE TEMP VIEW temp_chain AS SELECT payload FROM v",
    )
    .await;
    for (source, expected) in [
        ("main.v", json!({"main":1})),
        ("main.chain", json!({"main":1})),
        ("v", json!("{\"temp\":1}")),
        ("temp.v", json!("{\"temp\":1}")),
        ("temp.temp_chain", json!("{\"temp\":1}")),
    ] {
        assert_eq!(payload(&db, source).await, expected, "{source}");
    }
    db.exec_raw(
        "DROP VIEW temp.v; CREATE TEMP TABLE v(payload TEXT); \
         INSERT INTO temp.v VALUES('{\"table\":1}')",
    )
    .await
    .unwrap();
    assert_eq!(payload(&db, "v").await, json!("{\"table\":1}"));
    assert_eq!(payload(&db, "main.chain").await, json!({"main":1}));
}

#[tokio::test]
async fn json_views_in_attached_databases_keep_definitions_and_lookup_order() {
    let db = memory_database(
        "CREATE VIEW v AS SELECT '{\"main\":1}' AS payload; \
         ATTACH DATABASE ':memory:' AS \"aux.part\"; \
         ATTACH DATABASE ':memory:' AS later; \
         CREATE VIEW \"aux.part\".v AS SELECT json_object('aux',2) AS payload; \
         CREATE VIEW \"aux.part\".chain AS SELECT payload FROM v; \
         CREATE VIEW later.v AS SELECT json_object('later',3) AS payload",
    )
    .await;
    for (source, expected) in [
        ("v", json!("{\"main\":1}")),
        ("\"aux.part\".v", json!({"aux":2})),
        ("\"aux.part\".chain", json!({"aux":2})),
        ("later.v", json!({"later":3})),
    ] {
        assert_eq!(payload(&db, source).await, expected, "{source}");
    }
    db.exec_raw("DROP VIEW main.v").await.unwrap();
    assert_eq!(payload(&db, "v").await, json!({"aux":2}));
    db.exec_raw("DETACH DATABASE \"aux.part\"").await.unwrap();
    assert_eq!(payload(&db, "v").await, json!({"later":3}));
}

#[tideorm::model(table = "review_json_sources")]
struct JsonSource {
    #[tideorm(primary_key)]
    id: i64,
    doc: Option<Json>,
    label: String,
}

/// Text that looks like JSON is a string unless a JSON column holds it or one
/// of SQLite's JSON functions computed it. Every form of `->` reads a
/// document: chained, after a call or a literal, and with a numbered
/// parameter as its path; so do a JSON call in parentheses and a JSON
/// aggregate with a filter or over a window, which all came back as their
/// text.
#[tokio::test]
async fn expression_text_is_a_json_document_only_from_a_json_source() {
    let db = memory_database(
        "CREATE TABLE review_json_sources(id INTEGER PRIMARY KEY, doc JSON, label TEXT); \
         INSERT INTO review_json_sources VALUES(1, '{\"a\":{\"b\":[1,2]}}', '[3]')",
    )
    .await;
    let rows = db
        .query_raw_json(
            "SELECT '[1,2]' AS literal, label || '' AS joined, label, doc, \
             CAST('{\"c\":3}' AS TEXT) AS cast_text, json_object('b', 2) AS object, \
             json_array(1, 2), doc -> '$.a' AS extracted, doc ->> '$.a' AS extracted_text, \
             (SELECT json_group_array(id) FROM review_json_sources) AS grouped \
             FROM review_json_sources",
        )
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![json!({
            "literal": "[1,2]",
            "joined": "[3]",
            "label": "[3]",
            "doc": {"a": {"b": [1, 2]}},
            "cast_text": "{\"c\":3}",
            "object": {"b": 2},
            "json_array(1, 2)": [1, 2],
            "extracted": {"b": [1, 2]},
            "extracted_text": "{\"b\":[1,2]}",
            "grouped": [1],
        })]
    );
    let rows = db
        .query_raw_json_with_params(
            "SELECT doc -> 'a' -> 'b' AS chained, doc -> ?1 AS numbered, \
             json_extract(doc, '$.a') -> '$.b' AS called, \
             '[[3]]' -> 0 AS literal, (json_object('c', 3)) AS wrapped, \
             json_group_array(id) FILTER (WHERE id > 0) AS filtered, \
             json_group_array(id) OVER (PARTITION BY label) AS windowed, \
             doc -> 'a' ->> 'b' AS text_value \
             FROM review_json_sources",
            vec!["$.a".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![json!({
            "chained": [1, 2],
            "numbered": {"b": [1, 2]},
            "called": [1, 2],
            "literal": [3],
            "wrapped": {"c": 3},
            "filtered": [1],
            "windowed": [1],
            "text_value": "[1,2]",
        })]
    );
    let rows = JsonSource::query_with(&db)
        .select(vec!["id", "doc"])
        .select_raw("'[1,2]' AS label")
        .select_raw("json_object('b', 2) AS object")
        .get_json()
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![json!({"id": 1, "doc": {"a": {"b": [1, 2]}}, "label": "[1,2]", "object": {"b": 2}})]
    );
}

/// A table column's text is a document only when the column is declared
/// `JSON`: an untyped column, or one of a type the driver does not name,
/// holding text that looks like JSON reads as that text. A JSON expression in
/// a view reads as a document through the view.
#[tokio::test]
async fn sqlite_documents_follow_the_declared_type_and_views() {
    let db = memory_database(
        "CREATE TABLE review_declared(id INTEGER PRIMARY KEY, doc JSON, plain, tag CITEXT, \
             bin JSONB); \
         INSERT INTO review_declared VALUES(1, '{\"a\":1}', '[1,2]', '{\"b\":2}', '[3]'); \
         CREATE VIEW review_declared_view AS \
             SELECT id, json_object('k', id) AS built, doc, plain FROM review_declared; \
         CREATE VIEW review_view_of_view(n, made) AS SELECT id, built FROM review_declared_view",
    )
    .await;
    let rows = db
        .query_raw_json("SELECT doc, plain, tag, bin FROM review_declared")
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![json!({"doc": {"a": 1}, "plain": "[1,2]", "tag": "{\"b\":2}", "bin": [3]})]
    );
    for sql in [
        "SELECT built, doc, plain FROM review_declared_view",
        "SELECT v.built, v.doc, v.plain FROM review_declared_view AS v",
        "SELECT * FROM review_declared_view",
    ] {
        let rows = db.query_raw_json(sql).await.unwrap();
        assert_eq!(rows[0]["built"], json!({"k": 1}), "{sql}");
        assert_eq!(rows[0]["doc"], json!({"a": 1}), "{sql}");
        assert_eq!(rows[0]["plain"], json!("[1,2]"), "{sql}");
    }
    let rows = db
        .query_raw_json("SELECT made FROM review_view_of_view")
        .await
        .unwrap();
    assert_eq!(rows, vec![json!({"made": {"k": 1}})]);
}

/// A JSON function returning SQL text or a number reads as that value: only
/// the document-returning ones (`json`, `json_quote`, `json_extract` of
/// several paths) read as documents.
#[tokio::test]
async fn json_scalar_functions_preserve_sql_text() {
    let db = memory_database("").await;
    for (sql, expected) in [
        ("SELECT json_type('null') AS value", json!("null")),
        ("SELECT json_type('true') AS value", json!("true")),
        ("SELECT json_type('false') AS value", json!("false")),
        (
            r#"SELECT json_extract('{"s":"[2]"}', '$.s') AS value"#,
            json!("[2]"),
        ),
        (
            r#"SELECT json_extract('{"s":"null"}', '$.s') AS value"#,
            json!("null"),
        ),
        (
            r#"SELECT json_extract('{"s":"true"}', '$.s') AS value"#,
            json!("true"),
        ),
        (
            r#"SELECT json_extract('{"s":"42"}', '$.s') AS value"#,
            json!("42"),
        ),
        ("SELECT json_extract('[2]', '$') AS value", json!("[2]")),
        ("SELECT json_extract('[2]', '$[0]') AS value", json!(2)),
        ("SELECT json_extract('[2]', '$[1]') AS value", json!(null)),
        (
            r#"SELECT json_extract('{"s":"[2]"}', '$.s', '$.absent') AS value"#,
            json!(["[2]", null]),
        ),
        ("SELECT json_quote(json('[2]')) AS value", json!([2])),
        ("SELECT json(json_extract('[2]', '$')) AS value", json!([2])),
    ] {
        assert_eq!(
            db.query_raw_json(sql).await.unwrap(),
            vec![json!({"value": expected})],
            "{sql}"
        );
    }
}

/// Each column of a repeated name decodes by its own type before the last
/// one wins.
#[tokio::test]
async fn duplicate_json_aliases_decode_each_column_before_last_value_wins() {
    let db = memory_database("").await;
    for (sql, expected) in [
        (
            "SELECT json_array(1) AS value, '[2]' AS value",
            json!("[2]"),
        ),
        ("SELECT '[2]' AS value, json_array(1) AS value", json!([1])),
    ] {
        assert_eq!(
            db.query_raw_json(sql).await.unwrap()[0]["value"],
            expected,
            "{sql}"
        );
    }
}
