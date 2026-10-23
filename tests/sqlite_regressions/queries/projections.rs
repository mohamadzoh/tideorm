//! Projections, windows and subqueries decode with the right types and never load an
//! incomplete model.

use crate::fixtures::{
    ChunkProjection, Number, ProjectionFlag, QueryShape, User, chunk_projection_database,
    global_memory_database, memory_database, query_shapes_database, users_and_posts,
};
use tideorm::prelude::*;

#[tideorm::model(table = "schema_filter_rows", schema = "main")]
struct SchemaTextRow {
    #[tideorm(primary_key)]
    id: i64,
    value: String,
}

#[tideorm::model(table = "schema_filter_rows", schema = "aux")]
struct SchemaUuidRow {
    #[tideorm(primary_key)]
    id: i64,
    value: Uuid,
}

#[tokio::test]
async fn joined_filter_types_are_scoped_to_the_tables_schema() {
    let db = memory_database(
        "CREATE TABLE audit_numbers(id INTEGER PRIMARY KEY,value INTEGER); \
         INSERT INTO audit_numbers VALUES(1,1); \
         CREATE TABLE schema_filter_rows(id INTEGER PRIMARY KEY,value TEXT); \
         INSERT INTO schema_filter_rows VALUES(1,'00112233-4455-6677-8899-aabbccddeeff'); \
         ATTACH DATABASE ':memory:' AS aux; \
         CREATE TABLE aux.schema_filter_rows(id INTEGER PRIMARY KEY,value BLOB); \
         INSERT INTO aux.schema_filter_rows VALUES(1,x'00112233445566778899aabbccddeeff')",
    )
    .await;
    let value = "00112233-4455-6677-8899-aabbccddeeff";
    assert_eq!(
        SchemaTextRow::query_with(&db)
            .where_eq("value", value)
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        SchemaUuidRow::query_with(&db)
            .where_eq("value", value)
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    for schema in ["main", "aux"] {
        let rows = Number::query_with(&db)
            .inner_join_as(
                &format!("{schema}.schema_filter_rows"),
                "s",
                "audit_numbers.id",
                "s.id",
            )
            .select(vec!["s.value"])
            .where_eq("s.value", value)
            .get_json()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "{schema}");
        assert_eq!(rows[0]["value"], value);
    }
}

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
        ("main.v", serde_json::json!({"main":1})),
        ("main.chain", serde_json::json!({"main":1})),
        ("v", serde_json::json!("{\"temp\":1}")),
        ("temp.v", serde_json::json!("{\"temp\":1}")),
        ("temp.temp_chain", serde_json::json!("{\"temp\":1}")),
    ] {
        assert_eq!(payload(&db, source).await, expected, "{source}");
    }
    db.exec_raw("DROP VIEW temp.v; CREATE TEMP TABLE v(payload TEXT); INSERT INTO temp.v VALUES('{\"table\":1}')").await.unwrap();
    assert_eq!(payload(&db, "v").await, serde_json::json!("{\"table\":1}"));
    assert_eq!(
        payload(&db, "main.chain").await,
        serde_json::json!({"main":1})
    );
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
        ("v", serde_json::json!("{\"main\":1}")),
        ("\"aux.part\".v", serde_json::json!({"aux":2})),
        ("\"aux.part\".chain", serde_json::json!({"aux":2})),
        ("later.v", serde_json::json!({"later":3})),
    ] {
        assert_eq!(payload(&db, source).await, expected, "{source}");
    }
    db.exec_raw("DROP VIEW main.v").await.unwrap();
    assert_eq!(payload(&db, "v").await, serde_json::json!({"aux":2}));
    db.exec_raw("DETACH DATABASE \"aux.part\"").await.unwrap();
    assert_eq!(payload(&db, "v").await, serde_json::json!({"later":3}));
}

#[tideorm::model(table = "audit_documents")]
struct Document {
    #[tideorm(primary_key)]
    pk: i64,
    value: Json,
    id: Json,
    key: Json,
    r#type: Json,
    atom: Json,
    json: Json,
    parent: Json,
    fullkey: Json,
    path: Json,
    payload: Json,
}

#[tokio::test]
async fn json_subqueries_do_not_capture_outer_column_names() {
    let db=memory_database("CREATE TABLE audit_documents(pk INTEGER PRIMARY KEY,value JSON,id JSON,key JSON,type JSON,atom JSON,json JSON,parent JSON,fullkey JSON,path JSON,payload JSON); INSERT INTO audit_documents VALUES(1,'[1,2]','[1,2]','[1,2]','[1,2]','[1,2]','[1,2]','[1,2]','[1,2]','[1,2]','[1,2]')").await;
    for column in [
        "value", "id", "key", "type", "atom", "json", "parent", "fullkey", "path", "payload",
    ] {
        assert_eq!(
            Document::query_with(&db)
                .where_eq(column, serde_json::json!([1, 2]))
                .count()
                .await
                .unwrap(),
            1,
            "{column}"
        );
        assert_eq!(
            Document::query_with(&db)
                .where_array_contains(column, vec![1])
                .count()
                .await
                .unwrap(),
            1,
            "{column}"
        );
        Document::query_with(&db)
            .where_eq("pk", 1)
            .update_all()
            .array_remove(column, 2)
            .execute()
            .await
            .unwrap();
        let row = Document::find_with(1, &db).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(row).unwrap()[column],
            serde_json::json!([1]),
            "{column}"
        );
    }
}

#[tideorm::model(table = "audit_flags")]
struct Flag {
    #[tideorm(primary_key)]
    id: i64,
    value: bool,
}

#[tokio::test]
async fn compound_outputs_reject_mixed_decoders_and_keep_matching_types() {
    let db=global_memory_database("CREATE TABLE audit_numbers(id INTEGER PRIMARY KEY,value INTEGER); CREATE TABLE audit_flags(id INTEGER PRIMARY KEY,value BOOLEAN); INSERT INTO audit_numbers VALUES(1,42); INSERT INTO audit_flags VALUES(1,1)").await;
    for all in [false, true] {
        let number = Number::query_with(&db).select(vec!["value"]);
        let flag = Flag::query_with(&db).select(vec!["value"]);
        let left = if all {
            flag.clone().union_all(number.clone())
        } else {
            flag.clone().union(number.clone())
        };
        let right = if all {
            number.union_all(flag)
        } else {
            number.union(flag)
        };
        for result in [left.get_json().await, right.get_json().await] {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("incompatible column types"), "{error}");
        }
    }
    let rows = Flag::query_with(&db)
        .select(vec!["value AS flag"])
        .union_all(Flag::query_with(&db).select(vec!["value AS another"]))
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows, vec![serde_json::json!({"flag":true}); 2]);
    // The same check applies to the final selection and survives fragments.
    let fragment = Flag::query_with(&db)
        .select(vec!["value"])
        .union_all(Number::query_with(&db).select(vec!["value"]))
        .consolidate();
    assert!(Flag::query_with(&db).apply(&fragment).validate().is_err());
    let query = Flag::query_with(&db)
        .select(vec!["value"])
        .union_all(Number::query_with(&db).select(vec!["value"]))
        .select(vec!["id"]);
    assert_eq!(query.pluck::<i64>("id").await.unwrap(), vec![1, 42]);
}

#[tokio::test]
async fn window_output_can_complete_a_model_projection() {
    let db = chunk_projection_database().await;
    let query = ChunkProjection::query_with(&db)
        .select(vec!["id"])
        .row_number("rank", None, "id", Order::Asc)
        .order_asc("id");
    let rows = query.get().await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.id, row.rank))
            .collect::<Vec<_>>(),
        vec![(7, 1), (9, 2)]
    );
    // A Rust field alias cannot stand in for a renamed database result name.
    assert!(
        ChunkProjection::query_with(&db)
            .select(vec!["rank"])
            .row_number("id", None, "rank", Order::Asc)
            .get()
            .await
            .is_err()
    );
    let rows = ChunkProjection::query_with(&db)
        .select(vec!["rank"])
        .row_number("stored_id", None, "rank", Order::Asc)
        .get()
        .await
        .unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[tideorm::model(table = "review_projection_people")]
struct ProjectionPerson {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "display_name")]
    name: Option<String>,
}

async fn projection_database() -> Database {
    memory_database(
        "CREATE TABLE review_projection_people(id INTEGER PRIMARY KEY, display_name TEXT);
        INSERT INTO review_projection_people VALUES(1,'Other'),(7,'Alice');
        CREATE TABLE review_projection_flags(id INTEGER PRIMARY KEY, enabled INTEGER);
        INSERT INTO review_projection_flags VALUES(7,1);
        CREATE TABLE review_projection_unknown(id INTEGER PRIMARY KEY, enabled INTEGER);
        INSERT INTO review_projection_unknown VALUES(7,42)",
    )
    .await
}

#[tokio::test]
async fn window_alias_cannot_replace_a_model_primary_key() {
    let db = projection_database().await;
    let base = ProjectionPerson::query_with(&db).where_eq("id", 7);
    for query in [
        base.clone(),
        base.clone().distinct(),
        base.clone().select(vec![]),
        base.clone().select(vec!["*"]),
        base.clone().select(vec!["review_projection_people.*"]),
        base.clone().select_raw("review_projection_people.*"),
    ] {
        let query = query.row_number("id", None, "id", Order::Asc);
        assert!(
            query
                .clone()
                .get_json()
                .await
                .unwrap_err()
                .to_string()
                .contains("twice")
        );
        assert!(query.get().await.is_err());
    }
    let query = base.row_number("position", None, "id", Order::Asc);
    assert_eq!(query.clone().get_json().await.unwrap()[0]["position"], 1);
    let mut row = query.first_or_fail().await.unwrap();
    assert_eq!(row.id, 7);
    row.name = Some("Updated".into());
    Database::set_global(db.clone()).unwrap();
    row.save().await.unwrap();
    assert_eq!(
        db.query_raw_json("SELECT * FROM review_projection_people ORDER BY id")
            .await
            .unwrap(),
        vec![
            serde_json::json!({"id":1,"display_name":"Other"}),
            serde_json::json!({"id":7,"display_name":"Updated"})
        ]
    );
}

#[tokio::test]
async fn model_projection_requires_literal_database_output_names() {
    let db = projection_database().await;
    let base = ProjectionPerson::query_with(&db).where_eq("id", 7);
    let value = || base.clone().select(vec!["name"]);
    for query in [
        base.clone().select(vec!["id", "display_name AS name"]),
        base.clone()
            .select(vec!["id"])
            .select_subquery(value(), "name"),
    ] {
        // Aliases are still useful for DTO/JSON results, but cannot masquerade
        // as the database column that a complete model needs.
        assert_eq!(query.clone().get_json().await.unwrap()[0]["name"], "Alice");
        assert!(
            query
                .get()
                .await
                .unwrap_err()
                .to_string()
                .contains("display_name")
        );
    }
    for query in [
        base.clone().select(vec!["id", "name"]),
        base.clone().select(vec!["id", "name AS display_name"]),
        base.clone()
            .select(vec!["id"])
            .select_subquery(value(), "display_name"),
    ] {
        let row = query.first_or_fail().await.unwrap();
        assert_eq!(row.name.as_deref(), Some("Alice"));
        Database::set_global(db.clone()).unwrap();
        row.save().await.unwrap();
    }
    assert_eq!(
        db.query_raw_json("SELECT display_name FROM review_projection_people WHERE id=7")
            .await
            .unwrap()[0]["display_name"],
        "Alice"
    );
}

#[tokio::test]
async fn joined_json_reads_use_the_registered_source_type() {
    #[derive(serde::Deserialize, Debug)]
    struct FlagDto {
        name: bool,
    }
    #[derive(serde::Deserialize, Debug)]
    struct NullableFlagDto {
        name: Option<bool>,
    }
    let db = projection_database().await;
    let _ = TideConfig::init().models::<(ProjectionPerson, ProjectionFlag)>();
    assert_eq!(
        ProjectionFlag::query_with(&db).get_json().await.unwrap()[0]["enabled"],
        true
    );
    for (table, qualifier) in [
        ("review_projection_flags", "review_projection_flags"),
        ("review_projection_flags", "f"),
    ] {
        let query = if qualifier == table {
            ProjectionPerson::query_with(&db).inner_join(
                table,
                "review_projection_people.id",
                "review_projection_flags.id",
            )
        } else {
            ProjectionPerson::query_with(&db).inner_join_as(
                table,
                qualifier,
                "review_projection_people.id",
                "f.id",
            )
        };
        let projection = format!("{qualifier}.enabled AS name");
        let query = query.select(vec![&projection]);
        assert_eq!(query.clone().get_json().await.unwrap()[0]["name"], true);
        assert!(query.get_as::<FlagDto>().await.unwrap()[0].name);
    }
    let nullable = ProjectionPerson::query_with(&db)
        .left_join(
            "review_projection_flags",
            "review_projection_people.id",
            "review_projection_flags.id",
        )
        .select(vec!["review_projection_flags.enabled AS name"])
        .order_asc("review_projection_people.id")
        .get_as::<NullableFlagDto>()
        .await
        .unwrap();
    assert_eq!(
        nullable.iter().map(|row| row.name).collect::<Vec<_>>(),
        vec![None, Some(true)]
    );
    let unknown = ProjectionPerson::query_with(&db)
        .inner_join(
            "review_projection_unknown",
            "review_projection_people.id",
            "review_projection_unknown.id",
        )
        .select(vec!["review_projection_unknown.enabled AS name"])
        .get_json()
        .await
        .unwrap();
    assert_eq!(unknown[0]["name"], 42);
}

#[tokio::test]
async fn subquery_projections_cannot_load_incomplete_models() {
    let db = query_shapes_database().await;
    let id = || {
        QueryShape::query_with(&db)
            .select(vec!["id"])
            .where_eq("id", 1)
    };
    let name = || {
        QueryShape::query_with(&db)
            .select(vec!["name"])
            .where_eq("id", 1)
    };
    assert!(id().first().await.is_err());
    assert!(
        QueryShape::query_with(&db)
            .select_subquery(id(), "id")
            .where_eq("id", 1)
            .first()
            .await
            .is_err()
    );
    assert_eq!(
        QueryShape::find_with(1, &db)
            .await
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("A")
    );
    let mixed = QueryShape::query_with(&db)
        .select(vec!["name"])
        .select_subquery(id(), "id")
        .where_eq("id", 1)
        .first_or_fail()
        .await
        .unwrap();
    assert_eq!((mixed.id, mixed.name.as_deref()), (1, Some("A")));
    let complete = QueryShape::query_with(&db)
        .select_subquery(id(), "id")
        .select_subquery(name(), "name")
        .where_eq("id", 1)
        .first_or_fail()
        .await
        .unwrap();
    assert_eq!((complete.id, complete.name.as_deref()), (1, Some("A")));
    // Raw SQL remains the caller's documented projection escape hatch.
    assert!(
        QueryShape::query_with(&db)
            .select_raw("id,name")
            .get()
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn window_alias_uses_the_expression_type() {
    let db = users_and_posts("tenant", 10).await;
    db.exec_raw("INSERT INTO review_users VALUES(2, 'second', 0), (3, 'third', 1)")
        .await
        .unwrap();
    let rows = User::query_with(&db)
        .select(vec!["id"])
        .row_number("active", None, "id", Order::Asc)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["active"].clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(2), json!(3)]
    );
    assert!(
        User::query_with(&db)
            .select(vec!["active"])
            .row_number("active", None, "id", Order::Asc)
            .validate()
            .is_err()
    );
}

#[tokio::test]
async fn joined_column_keeps_its_type_when_a_model_field_is_renamed() {
    #[tideorm::model(table = "review_renamed_flag")]
    struct Renamed {
        #[tideorm(primary_key)]
        id: i64,
        #[tideorm(column = "stored_flag")]
        flag: bool,
    }
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_renamed_flag(id INTEGER PRIMARY KEY, stored_flag BOOLEAN)")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_other_flag(id INTEGER PRIMARY KEY, flag INTEGER)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_renamed_flag VALUES(1, 1)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_other_flag VALUES(1, 42)")
        .await
        .unwrap();
    let query = Renamed::query_with(&db).inner_join(
        "review_other_flag",
        "review_renamed_flag.id",
        "review_other_flag.id",
    );
    for column in ["review_other_flag.flag", "review_other_flag.flag AS flag"] {
        let rows = query.clone().select(vec![column]).get_json().await.unwrap();
        assert_eq!(rows[0]["flag"], 42);
    }
    let rows = query
        .select(vec!["review_renamed_flag.flag"])
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["stored_flag"], true);
}

#[tokio::test]
async fn empty_selection_loads_the_default_model_columns() {
    let db = chunk_projection_database().await;
    for distinct in [false, true] {
        let query = ChunkProjection::query_with(&db)
            .select(vec![])
            .order_asc("id");
        let query = if distinct { query.distinct() } else { query };
        assert_eq!(query.clone().get_json().await.unwrap().len(), 2);
        let rows = query.get().await.unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.id, row.rank))
                .collect::<Vec<_>>(),
            vec![(7, 80), (9, 90)]
        );
    }
}

#[tokio::test]
async fn value_windows_keep_boolean_types_and_nulls() {
    #[tideorm::model(table = "review_window_types")]
    struct WindowRow {
        #[tideorm(primary_key)]
        id: i64,
        active: Option<bool>,
    }
    #[derive(serde::Deserialize, Debug)]
    struct Output {
        id: i64,
        val: Option<bool>,
    }
    let db = memory_database("CREATE TABLE review_window_types(id INTEGER PRIMARY KEY, active BOOLEAN); INSERT INTO review_window_types VALUES(1,1),(2,0),(3,NULL)").await;
    for function in [
        WindowFunctionType::FirstValue("active".into()),
        WindowFunctionType::LastValue("active".into()),
        WindowFunctionType::NthValue("active".into(), 1),
        WindowFunctionType::Lag("active".into(), Some(0), None),
        WindowFunctionType::Lead("active".into(), Some(0), None),
        WindowFunctionType::Min("active".into()),
        WindowFunctionType::Max("active".into()),
    ] {
        let rows = WindowRow::query_with(&db)
            .select(vec!["id"])
            .window(
                WindowFunction::new(function, "val")
                    .partition_by("id")
                    .order_by("id", Order::Asc),
            )
            .order_asc("id")
            .get_as::<Output>()
            .await
            .unwrap();
        assert_eq!(
            rows.iter().map(|r| (r.id, r.val)).collect::<Vec<_>>(),
            vec![(1, Some(true)), (2, Some(false)), (3, None)]
        );
    }
}

#[tideorm::model(table = "review_json_sources")]
struct JsonSource {
    #[tideorm(primary_key)]
    id: i64,
    doc: Option<Json>,
    label: String,
}

/// Text that looks like JSON is a string unless it is a JSON column's or one
/// of SQLite's JSON functions computed it.
#[tokio::test]
async fn expression_text_is_a_json_document_only_from_a_json_source() {
    let db = memory_database(
        "CREATE TABLE review_json_sources(id INTEGER PRIMARY KEY, doc JSON, label TEXT); \
         INSERT INTO review_json_sources VALUES(1, '{\"a\":[1,2]}', '[3]')",
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
            "doc": {"a": [1, 2]},
            "cast_text": "{\"c\":3}",
            "object": {"b": 2},
            "json_array(1, 2)": [1, 2],
            "extracted": [1, 2],
            "extracted_text": "[1,2]",
            "grouped": [1],
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
        vec![json!({"id": 1, "doc": {"a": [1, 2]}, "label": "[1,2]", "object": {"b": 2}})]
    );
}

/// A JSON alias in a nested query types only the columns that reference it;
/// the statement tables are unit-tested with `sqlite_outputs`.
#[tokio::test]
async fn nested_json_aliases_do_not_reclassify_outer_text() {
    let db = memory_database("").await;
    for (sql, expected) in [
        (
            "SELECT '[2]' AS doc FROM (SELECT json_array(1) AS doc)",
            json!("[2]"),
        ),
        (
            "WITH nested AS (SELECT json_array(2) AS doc) SELECT doc FROM nested",
            json!([2]),
        ),
    ] {
        assert_eq!(
            db.query_raw_json(sql).await.unwrap(),
            vec![json!({ "doc": expected })],
            "{sql}"
        );
    }
    assert_eq!(
        db.query_raw_json("SELECT json_array(1) AS doc UNION ALL SELECT '[2]' AS doc")
            .await
            .unwrap(),
        vec![json!({"doc": "[1]"}), json!({"doc": "[2]"})]
    );
}

/// Every form of `->` reads a document: chained, after a call or a literal,
/// and with a numbered parameter as its path; so do a JSON call in
/// parentheses and a JSON aggregate with a filter or over a window. All came
/// back as their text.
#[tokio::test]
async fn every_form_of_a_json_expression_reads_as_a_document() {
    let db = memory_database(
        "CREATE TABLE review_json_sources(id INTEGER PRIMARY KEY, doc JSON, label TEXT); \
         INSERT INTO review_json_sources VALUES(1, '{\"a\":{\"b\":[1,2]}}', 'x')",
    )
    .await;
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

/// An aliased raw expression counts as the column it names, so it cannot hide
/// the model columns a projection leaves out; an unaliased one, such as a
/// wildcard, stays the caller's to vouch for.
#[tokio::test]
async fn raw_projections_cannot_hide_omitted_model_columns() {
    let db = chunk_projection_database().await;
    for incomplete in [
        ChunkProjection::query_with(&db)
            .select(vec!["stored_id"])
            .select_raw("rank * 2 AS doubled"),
        ChunkProjection::query_with(&db).select_raw("COUNT(*) AS total"),
    ] {
        let error = incomplete.get().await.unwrap_err().to_string();
        assert!(
            error.contains("leaves out") && error.contains("rank"),
            "{error}"
        );
    }
    let complete = ChunkProjection::query_with(&db)
        .select(vec!["stored_id"])
        .select_raw("rank + 0 AS rank")
        .order_asc("stored_id")
        .get()
        .await
        .unwrap();
    assert_eq!(
        complete.iter().map(|row| row.rank).collect::<Vec<_>>(),
        [80, 90]
    );
    assert_eq!(
        ChunkProjection::query_with(&db)
            .select_raw("*")
            .get()
            .await
            .unwrap()
            .len(),
        2
    );
}

/// A key lookup on a paged query is refused: the page would decide whether
/// the row is found.
#[tokio::test]
async fn find_refuses_a_paged_query() {
    let db = chunk_projection_database().await;
    for paged in [
        ChunkProjection::query_with(&db).limit(1),
        ChunkProjection::query_with(&db).offset(1),
        ChunkProjection::query_with(&db).page(2, 1),
    ] {
        let error = paged.find(9).await.unwrap_err().to_string();
        assert!(error.contains("takes no limit() or offset()"), "{error}");
    }
    assert_eq!(
        ChunkProjection::query_with(&db)
            .find(9)
            .await
            .unwrap()
            .unwrap()
            .rank,
        90
    );
}

/// The eager form refuses it alike: `find` on a `with(..)` query kept its page
/// and looked the row up inside it.
#[tokio::test]
async fn an_eager_find_refuses_a_paged_query() {
    let db = users_and_posts("eager", 5).await;
    for paged in [
        User::query_with(&db).limit(1),
        User::query_with(&db).offset(1),
    ] {
        let error = paged.with("posts").find(1).await.unwrap_err().to_string();
        assert!(error.contains("takes no limit() or offset()"), "{error}");
    }
    assert!(
        User::query_with(&db)
            .with("posts")
            .find(1)
            .await
            .unwrap()
            .is_some()
    );
}

#[tideorm::model(table = "union_reordered")]
struct Reordered {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
}

/// A raw union arm makes the default projection `table.*`, in the table's
/// column order; a typed arm in the model's declared order beside it would
/// swap the columns, so the mix is refused, and each kind alone still works.
#[tokio::test]
async fn raw_and_typed_union_arms_do_not_mix_over_the_default_projection() {
    let db = memory_database(
        "CREATE TABLE union_reordered(name TEXT, id INTEGER PRIMARY KEY); \
         INSERT INTO union_reordered VALUES ('one', 1), ('two', 2)",
    )
    .await;
    let mixed = Reordered::query_with(&db)
        .where_eq("id", 1)
        .union_all_raw("SELECT * FROM union_reordered WHERE id = 2")
        .union_all(Reordered::query_with(&db).where_eq("id", 2))
        .get()
        .await;
    let error = mixed.unwrap_err().to_string();
    assert!(error.contains("raw union arm"), "{error}");

    let names = |rows: Vec<Reordered>| {
        let mut names: Vec<String> = rows.into_iter().map(|row| row.name).collect();
        names.sort();
        names
    };
    let raw = Reordered::query_with(&db)
        .where_eq("id", 1)
        .union_all_raw("SELECT * FROM union_reordered WHERE id = 2")
        .get()
        .await
        .unwrap();
    assert_eq!(names(raw), ["one", "two"]);
    let typed = Reordered::query_with(&db)
        .where_eq("id", 1)
        .union_all(Reordered::query_with(&db).where_eq("id", 2))
        .get()
        .await
        .unwrap();
    assert_eq!(names(typed), ["one", "two"]);

    // A typed arm whose own query has a raw arm reads `table.*` as well, so
    // the query around it does too, where its declared order swapped them.
    let nested = Reordered::query_with(&db)
        .where_eq("id", 1)
        .union_all(
            Reordered::query_with(&db)
                .where_eq("id", 2)
                .union_all_raw("SELECT * FROM union_reordered WHERE id = 1"),
        )
        .get()
        .await
        .unwrap();
    assert_eq!(names(nested), ["one", "one", "two"]);
    let mixed = Reordered::query_with(&db)
        .union_all(Reordered::query_with(&db).where_eq("id", 2))
        .union_all(
            Reordered::query_with(&db)
                .where_eq("id", 2)
                .union_all_raw("SELECT * FROM union_reordered WHERE id = 1"),
        )
        .get()
        .await;
    let error = mixed.unwrap_err().to_string();
    assert!(error.contains("raw union arm"), "{error}");
}

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
/// one wins; the wildcard cases are unit-tested with `sqlite_outputs`.
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
