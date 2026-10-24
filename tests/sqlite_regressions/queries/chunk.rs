//! `chunk()` refuses a cursor it cannot keep stable, before any callback runs.

use crate::fixtures::{
    ChunkProjection, Item, QueryShape, chunk_projection_database, items_database,
    query_shapes_database,
};
use tideorm::prelude::*;

/// Assert that `chunk(size)` refuses `query` for `reason` before running its
/// callback.
async fn assert_refused<M: Model>(query: QueryBuilder<M>, size: u64, reason: &str) {
    let mut called = false;
    let error = query
        .chunk(size, |_| {
            called = true;
            async { Ok(()) }
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains(reason), "{error}");
    assert!(!called, "{reason}");
}

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
        assert_refused(query, 1, "original primary key").await;
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
            let mut seen = Vec::new();
            ChunkProjection::query_with(&db)
                .order_by(column, order)
                .chunk(1, |rows| {
                    seen.extend(rows.into_iter().map(|row| row.id));
                    async { Ok(()) }
                })
                .await
                .unwrap();
            let expected = if order == Order::Asc {
                vec![7, 9]
            } else {
                vec![9, 7]
            };
            assert_eq!(seen, expected, "{column}");
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
            assert_refused(query, size, "GROUP BY or HAVING").await;
        }
        let mut seen = Vec::new();
        QueryShape::query_with(&db)
            .chunk(size, |rows| {
                seen.extend(rows.into_iter().map(|row| row.id));
                async { Ok(()) }
            })
            .await
            .unwrap();
        assert_eq!(seen, vec![1, 2, 3], "{size}");
    }
}

/// The key cursor would change each window's input rows.
#[tokio::test]
async fn chunk_refuses_windows_before_running_callbacks() {
    let db = chunk_projection_database().await;
    let query = ChunkProjection::query_with(&db)
        .select(vec!["id"])
        .row_number("rank", None, "id", Order::Asc)
        .order_asc("id");
    assert_refused(query, 1, "window").await;
}
