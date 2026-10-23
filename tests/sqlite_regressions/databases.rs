//! Results and relations keep the database they were read from or written to, and
//! composed queries refuse operands bound to another one.

use crate::fixtures::{
    ChunkProjection, User, chunk_projection_database, memory_database, users_and_posts,
};
use serde_json::json;
use std::time::Duration;
use tideorm::prelude::*;

#[tideorm::model(table = "query_behavior_docs")]
struct Doc {
    #[tideorm(primary_key)]
    id: i64,
    doc: Json,
}

#[tideorm::model(table = "query_behavior_children")]
struct Child {
    #[tideorm(primary_key)]
    id: i64,
    doc_id: i64,
}

async fn documents() -> Database {
    memory_database("CREATE TABLE query_behavior_docs(id INTEGER PRIMARY KEY,doc TEXT); INSERT INTO query_behavior_docs VALUES(1,'{\"a\":1}'),(2,'{\"a\":2}'); CREATE TABLE query_behavior_children(id INTEGER PRIMARY KEY,doc_id INTEGER); INSERT INTO query_behavior_children VALUES(10,1),(20,2)").await
}

#[tokio::test]
async fn nested_self_relations_keep_the_explicit_database_without_a_global() {
    let db = documents().await;
    let matching = |q: QueryBuilder<Doc>| q.where_json_contains("doc", json!({"a":1}));
    let nested = |q: QueryBuilder<Doc>| q.where_has("id", "id", matching);
    let ids = Doc::query_with(&db)
        .where_has("id", "id", nested)
        .pluck::<i64>("id")
        .await
        .unwrap();
    assert_eq!(ids, vec![1]);
    let ids = Doc::query_with(&db)
        .where_doesnt_have("id", "id", nested)
        .pluck::<i64>("id")
        .await
        .unwrap();
    assert_eq!(ids, vec![2]);
    // Exercise three levels, including a nested NOT EXISTS and bound values
    // before and inside the subqueries.
    let ids = Doc::query_with(&db)
        .where_gt("id", 0)
        .where_has("id", "id", |q: QueryBuilder<Doc>| {
            q.where_has("id", "id", |q: QueryBuilder<Doc>| {
                q.where_doesnt_have("id", "id", matching)
            })
        })
        .pluck::<i64>("id")
        .await
        .unwrap();
    assert_eq!(ids, vec![2]);
}

#[tokio::test]
async fn nested_relations_between_models_keep_the_explicit_database() {
    let db = documents().await;
    let nested = |q: QueryBuilder<Child>| {
        q.where_has("id", "doc_id", |q: QueryBuilder<Doc>| {
            q.where_json_contains("doc", json!({"a":1}))
        })
    };
    let ids = Doc::query_with(&db)
        .where_has("doc_id", "id", nested)
        .pluck::<i64>("id")
        .await
        .unwrap();
    assert_eq!(ids, vec![1]);
    let ids = Doc::query_with(&db)
        .where_doesnt_have("doc_id", "id", nested)
        .pluck::<i64>("id")
        .await
        .unwrap();
    assert_eq!(ids, vec![2]);
    // Unpinned queries inside a database transaction still use that scope.
    db.transaction(|_| {
        Box::pin(async {
            let ids = Doc::query()
                .where_has("doc_id", "id", |q: QueryBuilder<Child>| {
                    q.where_has("id", "doc_id", |q: QueryBuilder<Doc>| {
                        q.where_json_contains("doc", json!({"a":1}))
                    })
                })
                .pluck::<i64>("id")
                .await?;
            assert_eq!(ids, vec![1]);
            Ok(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn relations_keep_the_read_database_including_cache_and_transactions() {
    let tenant = users_and_posts("tenant", 10).await;
    let global = users_and_posts("global", 20).await;
    Database::set_global(global.clone()).unwrap();
    let _cache = crate::fixtures::global_cache();
    let query = User::query_with(&tenant).cache(Duration::from_secs(60));
    for _ in 0..2 {
        let user = query.clone().first_or_fail().await.unwrap();
        assert_eq!(user.posts.load().await.unwrap()[0].id, 10);
    }
    let user = User::find_with(1, &tenant).await.unwrap().unwrap();
    assert_eq!(user.posts.load().await.unwrap()[0].id, 10);
    let user = User::query_with(&tenant)
        .with("posts")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.posts.get_cached().unwrap()[0].id, 10);
    assert_eq!(user.posts.load().await.unwrap()[0].id, 10);
    // A different pool's ambient transaction cannot redirect the relation.
    global
        .transaction(move |_| {
            Box::pin(async move {
                assert_eq!(user.posts.load().await?[0].id, 10);
                Ok(())
            })
        })
        .await
        .unwrap();
    // Returned relations must retain a pool, not keep the transaction alive.
    let user = tenant
        .transaction(|_| Box::pin(async { User::query().first_or_fail().await }))
        .await
        .unwrap();
    assert_eq!(user.posts.load().await.unwrap()[0].id, 10);
    Database::reset_global();
}

#[cfg(feature = "fulltext")]
#[tokio::test]
async fn ranked_search_relations_keep_the_read_database() {
    let tenant = users_and_posts("tenant", 10).await;
    tenant
        .exec_raw("CREATE VIRTUAL TABLE review_users_fts USING fts5(name)")
        .await
        .unwrap();
    tenant
        .exec_raw("INSERT INTO review_users_fts(rowid, name) VALUES(1, 'tenant')")
        .await
        .unwrap();
    Database::set_global(tenant.clone()).unwrap();
    let ranked = User::search(&["name"], "tenant")
        .get_ranked()
        .await
        .unwrap();
    assert_eq!(ranked.len(), 1);
    let global = users_and_posts("global", 20).await;
    Database::set_global(global).unwrap();
    assert_eq!(ranked[0].record.posts.load().await.unwrap()[0].id, 10);
}

#[tokio::test]
async fn upsert_results_keep_the_write_database() {
    let tenant = users_and_posts("tenant", 10).await;
    let global = users_and_posts("global", 20).await;
    let incoming = || User {
        id: 1,
        name: "updated".into(),
        active: true,
        posts: Default::default(),
    };
    Database::set_global(tenant.clone()).unwrap();
    let updated = User::on_conflict(vec!["id"])
        .insert(incoming())
        .await
        .unwrap();
    let unchanged = User::on_conflict(vec!["id"])
        .update_columns(vec![])
        .insert(incoming())
        .await
        .unwrap();
    let transaction_row = tenant
        .transaction(move |_| {
            Box::pin(async move { User::on_conflict(vec!["id"]).insert(incoming()).await })
        })
        .await
        .unwrap();
    Database::set_global(global).unwrap();
    for row in [updated, unchanged, transaction_row] {
        assert_eq!(row.posts.load().await.unwrap()[0].id, 10);
    }
}

#[tokio::test]
async fn bulk_insert_results_keep_the_write_database() {
    let tenant = users_and_posts("tenant", 10).await;
    let global = users_and_posts("global", 20).await;
    tenant.exec_raw("DELETE FROM review_users").await.unwrap();
    Database::set_global(tenant.clone()).unwrap();
    let incoming = |id| User {
        id,
        name: "inserted".into(),
        active: true,
        posts: Default::default(),
    };
    // Multi-row RETURNING and the one-row fallback must both capture origin.
    let batch = User::insert_all(vec![incoming(1), incoming(2)])
        .await
        .unwrap();
    tenant.exec_raw("DELETE FROM review_users").await.unwrap();
    let single = User::insert_all(vec![incoming(1)]).await.unwrap();
    tenant.exec_raw("DELETE FROM review_users").await.unwrap();
    let transaction_rows = tenant
        .transaction(move |_| {
            Box::pin(async move { User::insert_all(vec![incoming(1), incoming(2)]).await })
        })
        .await
        .unwrap();
    Database::set_global(global).unwrap();
    for rows in [batch, single, transaction_rows] {
        assert_eq!(rows[0].posts.load().await.unwrap()[0].id, 10);
    }
}

#[tokio::test]
async fn convenience_reads_retain_their_origin() {
    let tenant = users_and_posts("tenant", 10).await;
    let other = users_and_posts("other", 20).await;
    Database::set_global(tenant.clone()).unwrap();
    let mut rows = User::all().await.unwrap();
    rows.push(User::first().await.unwrap().unwrap());
    rows.push(User::last().await.unwrap().unwrap());
    rows.extend(User::paginate(1, 10).await.unwrap());
    let transactional = tenant
        .transaction(|_| Box::pin(async { User::paginate(1, 10).await }))
        .await
        .unwrap();
    rows.extend(transactional);
    Database::set_global(other).unwrap();
    for row in rows {
        assert_eq!(row.posts.load().await.unwrap()[0].id, 10);
    }
    Database::reset_global();
}

#[tokio::test]
async fn composed_queries_reject_other_databases_before_mutation() {
    let a = chunk_projection_database().await;
    let b = chunk_projection_database().await;
    b.exec_raw("UPDATE review_chunk_projection SET rank=999")
        .await
        .unwrap();
    let operand = || {
        ChunkProjection::query_with(&b)
            .where_eq("rank", 80)
            .select(vec!["id"])
    };
    assert_eq!(operand().count().await.unwrap(), 0);
    let error = ChunkProjection::query_with(&a)
        .where_in_subquery("id", operand())
        .delete()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("different database"));
    assert_eq!(ChunkProjection::query_with(&a).count().await.unwrap(), 2);
    assert!(
        ChunkProjection::query_with(&a)
            .union_all(ChunkProjection::query_with(&b))
            .get_json()
            .await
            .is_err()
    );
    assert!(
        ChunkProjection::query_with(&a)
            .select_subquery(operand(), "other")
            .get_json()
            .await
            .is_err()
    );
    assert!(
        ChunkProjection::query_with(&a)
            .with_query("other", operand())
            .get_json()
            .await
            .is_err()
    );
    // The requirement survives fragments and is checked against the final DB.
    let fragment = ChunkProjection::query_with(&b)
        .with_query("other", operand())
        .consolidate();
    assert!(
        ChunkProjection::query_with(&a)
            .apply(&fragment)
            .get_json()
            .await
            .is_err()
    );
    let inside = a.clone();
    a.transaction(|_| {
        Box::pin(async move {
            assert_eq!(
                ChunkProjection::query_with(&inside)
                    .where_in_subquery(
                        "id",
                        ChunkProjection::query_with(&inside).select(vec!["id"])
                    )
                    .count()
                    .await?,
                2
            );
            Ok(())
        })
    })
    .await
    .unwrap();
}

/// An in-memory database lives only while a connection to it is open, so its
/// pool closes none for being old or idle, and holds one connection when the
/// database is private to each; a pool size it cannot have is a configuration
/// error rather than a panic.
#[tokio::test]
async fn an_in_memory_pool_keeps_its_database_and_pool_sizes_are_checked() {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .max_lifetime(std::time::Duration::from_millis(20))
        .idle_timeout(std::time::Duration::from_millis(20))
        .build()
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE kept(id INTEGER PRIMARY KEY); INSERT INTO kept VALUES (1)")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let rows = db.query_raw_json("SELECT id FROM kept").await.unwrap();
    assert_eq!(rows.len(), 1);

    // With a private cache each connection opens its own database, so the pool
    // holds one: concurrent reads wait for it instead of opening an empty one.
    let private = Database::builder()
        .url("sqlite::memory:?cache=private")
        .build()
        .await
        .unwrap();
    private
        .exec_raw("CREATE TABLE kept(id INTEGER PRIMARY KEY); INSERT INTO kept VALUES (1)")
        .await
        .unwrap();
    let read = || private.query_raw_json("SELECT id FROM kept");
    let (first, second, third) = tokio::join!(read(), read(), read());
    for rows in [first, second, third] {
        assert_eq!(rows.unwrap().len(), 1);
    }

    for builder in [
        Database::builder()
            .url("sqlite::memory:")
            .max_connections(0),
        Database::builder()
            .url("sqlite::memory:")
            .min_connections(3)
            .max_connections(2),
        Database::builder()
            .url("sqlite::memory:?cache=private")
            .max_connections(2),
    ] {
        let error = builder.build().await.unwrap_err();
        assert!(
            matches!(error, tideorm::Error::Configuration { .. }),
            "{error:?}"
        );
    }
}
