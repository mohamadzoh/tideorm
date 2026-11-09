//! Results and relations keep the database they were read from or written to, and
//! composed queries refuse operands bound to another one.

use crate::fixtures::{
    ChunkProjection, TempFile, User, chunk_projection_database, memory_database, users_and_posts,
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
    db.transaction(async |_| {
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
        .transaction(async move |_| {
            assert_eq!(user.posts.load().await?[0].id, 10);
            Ok(())
        })
        .await
        .unwrap();
    // Returned relations must retain a pool, not keep the transaction alive.
    let user = tenant
        .transaction(async |_| User::query().first_or_fail().await)
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
    let ranked = User::search(["name"], "tenant").get_ranked().await.unwrap();
    assert_eq!(ranked.len(), 1);
    let global = users_and_posts("global", 20).await;
    Database::set_global(global).unwrap();
    assert_eq!(ranked[0].record.posts.load().await.unwrap()[0].id, 10);
}

/// Rows read or written through the global database keep it: once another
/// database is the global one, their relations still load from the first,
/// whether a read, an upsert or a batch insert returned them, in a transaction
/// or not.
#[tokio::test]
async fn read_and_written_rows_keep_their_database() {
    let tenant = users_and_posts("tenant", 10).await;
    let other = users_and_posts("other", 20).await;
    Database::set_global(tenant.clone()).unwrap();
    let user = |id| User {
        id,
        name: "written".into(),
        active: true,
        posts: Default::default(),
    };
    let mut rows = vec![
        ("all", User::all().await.unwrap().remove(0)),
        ("first", User::first().await.unwrap().unwrap()),
        ("last", User::last().await.unwrap().unwrap()),
        (
            "paginate",
            User::paginate(1, 10).await.unwrap().items.remove(0),
        ),
        (
            "paginate in a transaction",
            tenant
                .transaction(async |_| User::paginate(1, 10).await)
                .await
                .unwrap()
                .items
                .remove(0),
        ),
        (
            "upsert",
            User::on_conflict(vec!["id"]).insert(user(1)).await.unwrap(),
        ),
        (
            "upsert updating nothing",
            User::on_conflict(vec!["id"])
                .do_nothing()
                .insert(user(1))
                .await
                .unwrap(),
        ),
        (
            "upsert in a transaction",
            tenant
                .transaction(async move |_| User::on_conflict(vec!["id"]).insert(user(1)).await)
                .await
                .unwrap(),
        ),
    ];
    // Multi-row RETURNING and the one-row path alike.
    for (case, users) in [
        ("batch insert", vec![user(1), user(2)]),
        ("one-row batch insert", vec![user(1)]),
    ] {
        tenant.exec_raw("DELETE FROM review_users").await.unwrap();
        rows.push((case, User::insert_all(users).await.unwrap().remove(0)));
    }
    tenant.exec_raw("DELETE FROM review_users").await.unwrap();
    let mut inserted = tenant
        .transaction(async move |_| User::insert_all(vec![user(1), user(2)]).await)
        .await
        .unwrap();
    rows.push(("batch insert in a transaction", inserted.remove(0)));
    Database::set_global(other).unwrap();
    for (case, row) in rows {
        assert_eq!(row.posts.load().await.unwrap()[0].id, 10, "{case}");
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
    a.transaction(async move |_| {
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
    ] {
        let error = builder.build().await.unwrap_err();
        assert!(
            matches!(error, tideorm::Error::Configuration { .. }),
            "{error:?}"
        );
    }
    // The driver refuses a pool of several connections to a database each
    // connection would open privately: a temporary one has no name to share.
    for builder in [
        Database::builder()
            .url("sqlite::memory:?cache=private")
            .max_connections(2),
        Database::builder()
            .url("sqlite://?mode=memory")
            .min_connections(2),
        Database::builder().url("sqlite://").max_connections(2),
    ] {
        let error = builder.build().await.unwrap_err();
        assert!(
            matches!(error, tideorm::Error::Connection { .. }),
            "{error:?}"
        );
        assert_eq!(
            error.failure_kind(),
            tideorm::error::DbFailureKind::InvalidConfiguration,
            "{error:?}"
        );
    }
}

/// The builder takes the driver's own options, which name settings a URL
/// does not; the pool is sized over them as over a URL's.
#[tokio::test]
async fn the_builder_connects_with_the_drivers_options() {
    let file = TempFile::new("typed-options", "sqlite");
    let db = Database::builder()
        .sqlite_options(
            SqliteConnectOptions::new()
                .filename(&*file)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_millis(1234))
                .statement_cache_capacity(7),
        )
        .max_connections(2)
        .build()
        .await
        .unwrap();
    let rows = db
        .query_raw_json("SELECT * FROM pragma_journal_mode, pragma_busy_timeout")
        .await
        .unwrap();
    assert_eq!(rows, [json!({"journal_mode": "wal", "timeout": 1234})]);

    let error = Database::builder()
        .sqlite_options(SqliteConnectOptions::new())
        .max_connections(2)
        .build()
        .await
        .unwrap_err();
    assert_eq!(
        error.failure_kind(),
        tideorm::error::DbFailureKind::InvalidConfiguration,
        "a temporary database is private to its connection: {error:?}"
    );
    db.close().await.unwrap();
}

/// `raw_as` reads rows into a `FromQueryResult` struct, `try_backend` names
/// the backend without panicking, and a closed pool refuses statements.
#[tokio::test]
async fn raw_rows_read_as_any_type_and_a_closed_pool_refuses_statements() {
    #[derive(FromQueryResult, Debug, PartialEq)]
    struct PostCount {
        name: String,
        posts: i64,
    }

    #[derive(FromQueryResult, Debug, PartialEq)]
    struct Title {
        title: String,
        r#type: Option<String>,
    }

    let db = users_and_posts("tenant", 10).await;
    let counts: Vec<PostCount> = db
        .query_raw_as(
            "SELECT u.name, COUNT(p.id) AS posts FROM review_users u \
             LEFT JOIN review_posts p ON p.user_id = u.id GROUP BY u.name",
        )
        .await
        .unwrap();
    assert_eq!(
        counts,
        vec![PostCount {
            name: "tenant".into(),
            posts: 1
        }]
    );
    // Digits in text stay text, and NULL reads as `None`.
    let titles: Vec<Title> = db
        .query_raw_as_with_params(
            "SELECT '00123' AS title, NULL AS type FROM review_posts WHERE id = ?",
            [10_i64.into()],
        )
        .await
        .unwrap();
    assert_eq!(
        titles,
        vec![Title {
            title: "00123".into(),
            r#type: None
        }]
    );
    let error = db
        .query_raw_as::<PostCount>("SELECT name FROM review_users")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("raw_as()"), "{error}");
    // The associated form reads the ambient database.
    let scoped: Vec<PostCount> = db
        .scope(Database::raw_as(
            "SELECT name, 0 AS posts FROM review_users",
        ))
        .await
        .unwrap();
    assert_eq!(scoped.len(), 1);

    assert_eq!(db.try_backend().unwrap(), DatabaseType::SQLite);
    db.close().await.unwrap();
    assert!(db.exec_raw("SELECT 1").await.is_err());
}

/// A pinned session keeps what its statements set for its connection, runs
/// its transactions there, and is a database of its own: a handle on the
/// pool does not join its transaction.
#[tokio::test]
async fn a_pinned_session_keeps_its_connection_settings() {
    let (db, _file) = crate::fixtures::file_database("pinned-session").await;
    let session = db.pin_session().await.unwrap();
    session.exec_raw("PRAGMA foreign_keys = OFF").await.unwrap();
    let enforced = |db: Database| async move {
        db.query_raw_json("PRAGMA foreign_keys").await.unwrap()[0]["foreign_keys"].as_i64()
    };
    assert_eq!(enforced(session.clone()).await, Some(0));
    assert_eq!(enforced(db.clone()).await, Some(1));

    let pool = db.clone();
    let (inside, beside) = session
        .transaction(async move |_| {
            let inside = Database::raw_json("PRAGMA foreign_keys").await?;
            let beside = pool.query_raw_json("PRAGMA foreign_keys").await?;
            Ok((
                inside[0]["foreign_keys"].as_i64(),
                beside[0]["foreign_keys"].as_i64(),
            ))
        })
        .await
        .unwrap();
    assert_eq!((inside, beside), (Some(0), Some(1)));

    session.close().await.unwrap();
    assert!(session.exec_raw("SELECT 1").await.is_err());
    assert_eq!(enforced(db).await, Some(1));
}

#[tokio::test]
async fn the_driver_keeps_a_parameterized_statement_prepared_for_reuse() {
    let db = memory_database("").await;
    let select = |id: i64| db.query_raw_json_with_params("SELECT ? AS id", [DbValue::from(id)]);
    let before = db.cached_statements().await.unwrap();
    select(1).await.unwrap();
    let prepared = db.cached_statements().await.unwrap();
    assert!(prepared > before, "{before} -> {prepared}");
    select(2).await.unwrap();
    select(3).await.unwrap();
    assert_eq!(db.cached_statements().await.unwrap(), prepared);
}

/// A connection keeps at most `statement_cache_capacity` statements prepared,
/// and none with 0.
#[tokio::test]
async fn the_statement_cache_keeps_at_most_its_capacity() {
    for capacity in [0, 1] {
        let db = Database::builder()
            .url("sqlite::memory:")
            .max_connections(1)
            .statement_cache_capacity(capacity)
            .build()
            .await
            .unwrap();
        for sql in ["SELECT ? AS a", "SELECT ? AS b"] {
            db.query_raw_json_with_params(sql, [DbValue::from(1_i64)])
                .await
                .unwrap();
        }
        assert_eq!(db.cached_statements().await.unwrap(), capacity);
    }
}
