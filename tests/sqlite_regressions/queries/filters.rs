//! Filters on binary, JSON and text values, and soft-delete scopes.

use crate::fixtures::{AuditRow, memory_database};
use tideorm::prelude::*;

#[tokio::test]
async fn trash_scope_requires_soft_delete_before_reads_or_writes() {
    let db=memory_database("CREATE TABLE audit_rows(id INTEGER PRIMARY KEY,amount INTEGER,aggregate_1 INTEGER); INSERT INTO audit_rows VALUES(1,20,7)").await;
    assert!(
        AuditRow::query_with(&db)
            .only_trashed()
            .get()
            .await
            .is_err()
    );
    assert!(
        AuditRow::query_with(&db)
            .only_trashed()
            .count()
            .await
            .is_err()
    );
    assert!(
        AuditRow::query_with(&db)
            .only_trashed()
            .delete()
            .await
            .is_err()
    );
    assert!(
        AuditRow::query_with(&db)
            .only_trashed()
            .where_eq("id", 1)
            .update_all()
            .set("amount", 99)
            .execute()
            .await
            .is_err()
    );
    assert_eq!(
        AuditRow::find_with(1, &db).await.unwrap().unwrap().amount,
        20
    );
}

#[tideorm::model(table = "resume_names")]
struct NameRow {
    #[tideorm(primary_key)]
    id: i64,
    name: Option<String>,
}

/// `where_contains`, `where_starts_with` and `where_ends_with` match their
/// text literally, past a NUL the stored text holds; a NUL in the text
/// searched for is refused, for a delete as for a read.
#[tokio::test]
async fn literal_text_filters_match_past_a_stored_nul_and_refuse_one_in_the_pattern() {
    let db = memory_database(
        "CREATE TABLE resume_names(id INTEGER PRIMARY KEY,name TEXT); \
         INSERT INTO resume_names VALUES(1,'prefix'||char(0)||'suffix'),(2,'prefix'),\
         (3,'A_%!tail'),(4,NULL),(5,'🙂suffix')",
    )
    .await;
    for query in [
        NameRow::query_with(&db).where_contains("name", "a\0missing"),
        NameRow::query_with(&db).where_starts_with("name", "a\0missing"),
        NameRow::query_with(&db).where_ends_with("name", "a\0missing"),
    ] {
        let error = query.clone().get().await.unwrap_err().to_string();
        assert!(error.contains("cannot represent a NUL"), "{error}");
        assert!(query.delete().await.is_err());
    }
    assert_eq!(
        NameRow::query_with(&db)
            .where_contains("name", "suffix")
            .order_asc("id")
            .pluck::<i64>("id")
            .await
            .unwrap(),
        vec![1, 5]
    );
    assert_eq!(
        NameRow::query_with(&db)
            .where_ends_with("name", "prefix")
            .pluck::<i64>("id")
            .await
            .unwrap(),
        vec![2]
    );
    assert_eq!(
        NameRow::query_with(&db)
            .where_starts_with("name", "PREFIX")
            .order_asc("id")
            .pluck::<i64>("id")
            .await
            .unwrap(),
        vec![1, 2]
    );
    assert_eq!(
        NameRow::query_with(&db)
            .where_contains("name", "_%!")
            .pluck::<i64>("id")
            .await
            .unwrap(),
        vec![3]
    );
    assert_eq!(
        NameRow::query_with(&db)
            .where_ends_with("name", "")
            .count()
            .await
            .unwrap(),
        4
    );
    db.exec_raw("PRAGMA case_sensitive_like=ON").await.unwrap();
    assert_eq!(
        NameRow::query_with(&db)
            .where_starts_with("name", "PREFIX")
            .count()
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        NameRow::query_with(&db)
            .where_contains("name", "suffix")
            .count()
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        NameRow::query_with(&db)
            .where_ends_with("name", "prefix")
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        NameRow::find_with(1, &db).await.unwrap().unwrap().name,
        Some("prefix\0suffix".into())
    );
}

#[tokio::test]
async fn sqlite_empty_contains_excludes_sql_null() {
    #[tideorm::model(table = "review_array_nulls")]
    struct ArrayNulls {
        #[tideorm(primary_key)]
        id: i64,
        items: Option<serde_json::Value>,
    }
    let db = memory_database(
        "CREATE TABLE review_array_nulls(id INTEGER PRIMARY KEY, items JSON); \
         INSERT INTO review_array_nulls VALUES(1,'[]'),(2,'[1]'),(3,NULL)",
    )
    .await;
    let rows = ArrayNulls::query_with(&db)
        .where_array_contains("items", Vec::<serde_json::Value>::new())
        .order_by("id", Order::Asc)
        .get()
        .await
        .unwrap();
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[tideorm::model(table = "review_bytes")]
struct BinaryRow {
    #[tideorm(primary_key)]
    id: i64,
    payload: Vec<u8>,
}

#[tokio::test]
async fn binary_equality_matches_stored_bytes() {
    let db = memory_database(
        "CREATE TABLE review_bytes(id INTEGER PRIMARY KEY, payload BLOB); \
         INSERT INTO review_bytes VALUES(1, X'0102'), (2, X'')",
    )
    .await;
    assert_eq!(
        BinaryRow::query_with(&db)
            .where_eq("payload", vec![1u8, 2])
            .first()
            .await
            .unwrap()
            .unwrap()
            .id,
        1
    );
    assert_eq!(
        BinaryRow::query_with(&db)
            .where_eq("payload", Vec::<u8>::new())
            .first()
            .await
            .unwrap()
            .unwrap()
            .id,
        2
    );
}

#[tokio::test]
async fn sqlite_array_filters_exclude_non_arrays_even_on_delete() {
    #[tideorm::model(table = "review_array_shapes")]
    struct Shapes {
        #[tideorm(primary_key)]
        id: i64,
        payload: Option<serde_json::Value>,
    }
    let db = memory_database(
        r#"CREATE TABLE review_array_shapes(id INTEGER PRIMARY KEY, payload JSON);
           INSERT INTO review_array_shapes
               VALUES(1,'[1]'),(2,'{"a":1}'),(3,'1'),(4,'[]'),(5,'true'),(6,'null'),(7,NULL)"#,
    )
    .await;
    let query = Shapes::query_with(&db);
    assert_eq!(
        query
            .clone()
            .where_array_contains("payload", vec![1])
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        query
            .clone()
            .where_array_overlaps("payload", vec![1])
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        query
            .clone()
            .where_array_contained_by("payload", vec![1])
            .count()
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        query
            .clone()
            .where_array_contains("payload", Vec::<i32>::new())
            .count()
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        query
            .clone()
            .where_array_contained_by("payload", Vec::<i32>::new())
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        query
            .clone()
            .where_array_contains("payload", vec![1])
            .delete()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_raw_json("SELECT id FROM review_array_shapes ORDER BY id")
            .await
            .unwrap()
            .iter()
            .map(|r| r["id"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5, 6, 7]
    );
}

/// A search for nothing, or a pattern of `%` alone, filters nothing, so it
/// cannot license a destructive mutation, and a group of empty groups is no
/// filter at all; a value no parameter can hold fails the query instead of
/// panicking.
#[tokio::test]
async fn vacuous_filters_neither_license_mutations_nor_empty_queries() {
    let db = memory_database(
        "CREATE TABLE audit_rows(id INTEGER PRIMARY KEY,amount INTEGER,aggregate_1 INTEGER); \
         INSERT INTO audit_rows VALUES(1,20,7),(2,30,8)",
    )
    .await;
    let count = || async { AuditRow::query_with(&db).count().await.unwrap() };
    for refused in [
        AuditRow::query_with(&db)
            .where_contains("amount", "")
            .delete()
            .await,
        AuditRow::query_with(&db)
            .where_starts_with("amount", "")
            .delete()
            .await,
        AuditRow::query_with(&db)
            .where_ends_with("amount", "")
            .delete()
            .await,
        AuditRow::query_with(&db)
            .where_like("amount", "%%")
            .delete()
            .await,
    ] {
        assert!(refused.is_err(), "{refused:?}");
    }
    assert_eq!(count().await, 2);
    assert_eq!(
        AuditRow::query_with(&db)
            .where_contains("amount", "2")
            .delete()
            .await
            .unwrap(),
        1
    );

    let rows = AuditRow::query_with(&db)
        .or_where(|group| group.nested_or(|inner| inner.nested_and(|deepest| deepest)))
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);

    let error = AuditRow::query_with(&db)
        .where_eq("amount", i128::MAX)
        .get()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot be bound"), "{error}");
}

/// Columns named like those of SQLite's `json_each`, whose rows the JSON
/// comparisons read.
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
    let db = memory_database(
        "CREATE TABLE audit_documents(pk INTEGER PRIMARY KEY,value JSON,id JSON,key JSON,\
         type JSON,atom JSON,json JSON,parent JSON,fullkey JSON,path JSON,payload JSON); \
         INSERT INTO audit_documents VALUES(1,'[1,2]','[1,2]','[1,2]','[1,2]','[1,2]',\
         '[1,2]','[1,2]','[1,2]','[1,2]','[1,2]')",
    )
    .await;
    for column in [
        "value", "id", "key", "type", "atom", "json", "parent", "fullkey", "path", "payload",
    ] {
        assert_eq!(
            Document::query_with(&db)
                .where_eq(column, json!([1, 2]))
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
            json!([1]),
            "{column}"
        );
    }
}
