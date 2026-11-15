//! A table name with a space and a column name with a `.` are the model's own
//! names: every clause writes them whole, never split at the `.` or refused
//! for the space.

use crate::fixtures::memory_database;
use tideorm::prelude::*;

#[tideorm::model(table = "spaced items")]
struct SpacedItem {
    #[tideorm(primary_key)]
    id: i64,
    grp: String,
    qty: i64,
}

#[tideorm::model(table = "dotted_items")]
struct DottedItem {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "a.b")]
    dotted: i64,
}

#[tokio::test]
async fn a_table_name_with_a_space_is_written_whole() {
    let db = memory_database(
        r#"CREATE TABLE "spaced items" (id INTEGER PRIMARY KEY, grp TEXT NOT NULL, qty INTEGER NOT NULL);
        INSERT INTO "spaced items" VALUES (1, 'a', 10), (2, 'a', 15), (3, 'b', 30), (4, 'b', 5);"#,
    )
    .await;
    let ids = |items: &[SpacedItem]| items.iter().map(|item| item.id).collect::<Vec<_>>();
    let groups = SpacedItem::query_with(&db)
        .select(vec!["grp"])
        .group_by("grp")
        .having(Aggregate::max("qty").gt(20))
        .get_json()
        .await
        .unwrap();
    assert_eq!(groups, [serde_json::json!({"grp": "b"})]);

    let qualified = SpacedItem::query_with(&db)
        .select(vec!["spaced items.*"])
        .where_eq("spaced items.grp", "b")
        .order_desc("spaced items.qty")
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&qualified), [3, 4]);

    let first = SpacedItem::query_with(&db)
        .cursor_paginate(3, None)
        .await
        .unwrap();
    let rest = SpacedItem::query_with(&db)
        .cursor_paginate(3, first.next_cursor.as_deref())
        .await
        .unwrap();
    assert_eq!(
        (ids(&first.items), ids(&rest.items)),
        (vec![1, 2, 3], vec![4])
    );

    let mut chunks = SpacedItem::query_with(&db).chunks(3).unwrap();
    let mut chunked = Vec::new();
    while let Some(chunk) = chunks.next().await.unwrap() {
        chunked.extend(ids(&chunk));
    }
    assert_eq!(chunked, [1, 2, 3, 4]);
}

#[tokio::test]
async fn a_column_name_with_a_dot_is_one_column() {
    let db = memory_database(
        r#"CREATE TABLE dotted_items (id INTEGER PRIMARY KEY, "a.b" INTEGER NOT NULL);
        INSERT INTO dotted_items VALUES (1, 10), (2, 30), (3, 20);"#,
    )
    .await;
    let ids = |items: Vec<DottedItem>| items.into_iter().map(|item| item.id).collect::<Vec<_>>();
    for column in ["dotted", "a.b"] {
        let found = DottedItem::query_with(&db)
            .where_gt(column, 15)
            .order_desc(column)
            .get()
            .await
            .unwrap();
        assert_eq!(ids(found), [2, 3]);
    }
    let typed = DottedItem::query_with(&db)
        .where_gt(DottedItem::DOTTED, 15)
        .order_asc(DottedItem::DOTTED)
        .get()
        .await
        .unwrap();
    assert_eq!(ids(typed), [3, 2]);
    assert_eq!(
        DottedItem::query_with(&db)
            .max::<i64>("dotted")
            .await
            .unwrap(),
        Some(30)
    );
    let ranked = DottedItem::query_with(&db)
        .select(vec!["id"])
        .window(WindowFunction::new(
            WindowFunctionType::Max("dotted".into()),
            "top",
        ))
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(ranked[0]["top"], 30);
    let plucked = DottedItem::query_with(&db)
        .order_asc("id")
        .pluck(DottedItem::DOTTED)
        .await
        .unwrap();
    assert_eq!(plucked, [10, 30, 20]);
    for column in ["a.b", "dotted_items.a.b"] {
        let selected = DottedItem::query_with(&db)
            .select(vec!["id", column])
            .where_eq(column, 30)
            .get()
            .await
            .unwrap();
        assert_eq!(
            selected.iter().map(|item| item.dotted).collect::<Vec<_>>(),
            [30]
        );
    }
    let groups = DottedItem::query_with(&db)
        .select(vec!["id"])
        .group_by("id")
        .having(Aggregate::max("dotted").gt(15))
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(
        groups,
        [serde_json::json!({"id": 2}), serde_json::json!({"id": 3})]
    );
}
