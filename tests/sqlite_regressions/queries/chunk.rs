//! `chunk()` refuses a cursor it cannot keep stable, before any callback runs.

use crate::fixtures::{
    ChunkProjection, Item, QueryShape, chunk_projection_database, items_database,
    query_shapes_database,
};
use tideorm::prelude::*;

#[tokio::test]
async fn chunk_rejects_replaced_keys_before_callbacks() {
    let db = items_database().await;
    let query = Item::query_with(&db).select(vec!["rank AS id", "id AS rank", "amount"]);
    let expected: Vec<_> = query
        .clone()
        .get()
        .await
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(expected, vec![10, 20, 30]);
    for query in [
        query,
        Item::query_with(&db)
            .select(vec!["rank", "amount"])
            .select_raw("id + 1 AS id"),
        Item::query_with(&db)
            .select(vec!["rank", "amount"])
            .select_subquery(
                Item::query_with(&db).select(vec!["rank"]).where_eq("id", 1),
                "id",
            ),
        Item::query_with(&db)
            .inner_join_as("behavior_items", "other", "behavior_items.id", "other.id")
            .select(vec!["other.id AS id", "rank", "amount"]),
    ] {
        let mut called = false;
        let error = query
            .chunk(1, |_| {
                called = true;
                async { Ok(()) }
            })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("original primary key"),
            "{error}"
        );
        assert!(!called);
    }
    // Same-source aliases, wildcards and simple raw key projections remain valid.
    for query in [
        Item::query_with(&db).select(vec!["id AS id", "rank", "amount"]),
        Item::query_with(&db).select(vec!["behavior_items.*"]),
        Item::query_with(&db)
            .select(vec!["rank", "amount"])
            .select_raw("id AS id"),
        Item::query_with(&db).select(vec![]),
    ] {
        let mut seen = Vec::new();
        query
            .chunk(1, |rows| {
                seen.extend(rows.into_iter().map(|row| row.id));
                async { Ok(()) }
            })
            .await
            .unwrap();
        assert_eq!(seen, vec![1, 2, 3]);
    }
}

#[tokio::test]
async fn chunk_order_resolves_renamed_primary_keys() {
    let db = chunk_projection_database().await;
    for column in [
        "id",
        "stored_id",
        "review_chunk_projection.id",
        "review_chunk_projection.stored_id",
    ] {
        for order in [Order::Asc, Order::Desc] {
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let output = seen.clone();
            ChunkProjection::query_with(&db)
                .order_by(column, order)
                .chunk(1, move |rows| {
                    let output = output.clone();
                    async move {
                        output
                            .lock()
                            .unwrap()
                            .extend(rows.into_iter().map(|row| row.id));
                        Ok(())
                    }
                })
                .await
                .unwrap();
            let expected = if order == Order::Asc {
                vec![7, 9]
            } else {
                vec![9, 7]
            };
            assert_eq!(*seen.lock().unwrap(), expected, "{column}");
        }
    }
    assert!(
        ChunkProjection::query_with(&db)
            .order_asc("rank")
            .chunk(1, |_| async { Ok(()) })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn grouped_chunking_is_rejected_before_callbacks() {
    let db = query_shapes_database().await;
    for size in [1, 2, 10] {
        for query in [
            QueryShape::query_with(&db)
                .select_raw("MIN(id) AS id,name")
                .group_by("name"),
            QueryShape::query_with(&db)
                .select_raw("MIN(id) AS id,MIN(name) AS name")
                .having("COUNT(*) > 0"),
        ] {
            let result = query
                .chunk(size, |_| async { panic!("grouped callback must not run") })
                .await;
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("GROUP BY or HAVING")
            );
        }
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = seen.clone();
        QueryShape::query_with(&db)
            .chunk(size, move |rows| {
                let output = output.clone();
                async move {
                    output
                        .lock()
                        .unwrap()
                        .extend(rows.into_iter().map(|row| row.id));
                    Ok(())
                }
            })
            .await
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![1, 2, 3]);
    }
}

#[tokio::test]
async fn chunk_refuses_windows_before_running_callbacks() {
    let db = chunk_projection_database().await;
    let query = ChunkProjection::query_with(&db)
        .select(vec!["id"])
        .row_number("rank", None, "id", Order::Asc)
        .order_asc("id");
    let rows = query.clone().get().await.unwrap();
    assert_eq!(rows.iter().map(|r| r.rank).collect::<Vec<_>>(), vec![1, 2]);
    let mut called = false;
    let error = query
        .chunk(1, |_| {
            called = true;
            async { Ok(()) }
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("window"));
    assert!(!called);
}
