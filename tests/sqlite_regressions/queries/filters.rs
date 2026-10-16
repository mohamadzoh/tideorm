//! Filters on binary, JSON array and text values, and soft-delete scopes.

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

#[tokio::test]
async fn literal_text_filters_include_stored_text_after_nul() {
    let db = memory_database("CREATE TABLE resume_names(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO resume_names VALUES(1,'prefix'||char(0)||'suffix'),(2,'prefix'),(3,'A_%!tail'),(4,NULL),(5,'🙂suffix')").await;
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
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_array_nulls(id INTEGER PRIMARY KEY, items JSON)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_array_nulls VALUES(1,'[]'),(2,'[1]'),(3,NULL)")
        .await
        .unwrap();
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
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_bytes(id INTEGER PRIMARY KEY, payload BLOB)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_bytes VALUES(1, X'0102'), (2, X'')")
        .await
        .unwrap();
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
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_array_shapes(id INTEGER PRIMARY KEY, payload JSON)")
        .await
        .unwrap();
    db.exec_raw(r#"INSERT INTO review_array_shapes VALUES(1,'[1]'),(2,'{"a":1}'),(3,'1'),(4,'[]'),(5,'true'),(6,'null'),(7,NULL)"#).await.unwrap();
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
