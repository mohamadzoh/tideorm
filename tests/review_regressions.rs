//! Reproductions retained from the October code review.
#![cfg(all(feature = "sqlite", feature = "runtime-tokio"))]

use std::time::Duration;
use tideorm::prelude::*;

#[tideorm::model(table = "review_chunk_projection")]
struct ChunkProjection {
    #[tideorm(primary_key, column = "stored_id")]
    id: i64,
    rank: i64,
}

async fn chunk_projection_database() -> Database {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_chunk_projection(stored_id INTEGER PRIMARY KEY, rank INTEGER); INSERT INTO review_chunk_projection VALUES(7,80),(9,90)").await.unwrap();
    db
}

#[tokio::test]
async fn review_batch_setters_reject_foreign_typed_columns() {
    let db = chunk_projection_database().await;
    for operation in 0..10 {
        let update = ChunkProjection::query_with(&db)
            .where_eq("id", 7)
            .update_all();
        let update = match operation {
            0 => update.set(ProjectionFlag::ID, 123),
            1 => update.increment(ProjectionFlag::ID, 1),
            2 => update.decrement(ProjectionFlag::ID, 1),
            3 => update.multiply(ProjectionFlag::ID, 2.0),
            4 => update.divide(ProjectionFlag::ID, 2.0),
            5 => update.coalesce(ProjectionFlag::ID, 123),
            6 => update.array_append(ProjectionFlag::ID, 123),
            7 => update.array_remove(ProjectionFlag::ID, 123),
            8 => update.json_set(ProjectionFlag::ID, "$.a", 123),
            _ => update.set_trusted_raw(ProjectionFlag::ID, "123"),
        };
        assert!(
            update
                .execute()
                .await
                .unwrap_err()
                .to_string()
                .contains("typed column"),
            "operation {operation}"
        );
    }
    assert_eq!(
        ChunkProjection::find_with(7, &db)
            .await
            .unwrap()
            .unwrap()
            .rank,
        80
    );
    for typed in [true, false] {
        let update = ChunkProjection::query_with(&db)
            .where_eq("id", 7)
            .update_all();
        let update = if typed {
            update.set(ChunkProjection::RANK, 81)
        } else {
            update.set("rank", 82)
        };
        assert_eq!(update.execute().await.unwrap(), 1);
    }
    assert_eq!(
        ChunkProjection::find_with(7, &db)
            .await
            .unwrap()
            .unwrap()
            .rank,
        82
    );
}

#[tokio::test]
async fn review_window_output_can_complete_a_model_projection() {
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

#[tokio::test]
async fn review_chunk_order_resolves_renamed_primary_keys() {
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

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn review_distinct_rejects_randomized_encrypted_values() {
    #[tideorm::model(table = "review_distinct_secret", encrypted = "secret")]
    struct Secret {
        #[tideorm(primary_key)]
        id: i64,
        #[tideorm(column = "stored_secret")]
        secret: String,
    }
    Database::reset_global();
    TideConfig::reset();
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .encryption_key("distinct-regression-key")
        .connect()
        .await
        .unwrap();
    Database::execute(
        "CREATE TABLE review_distinct_secret(id INTEGER PRIMARY KEY,stored_secret TEXT)",
    )
    .await
    .unwrap();
    for (i, value) in ["A", "A", "B", "B"].iter().enumerate() {
        Secret::create(Secret {
            id: i as i64 + 1,
            secret: (*value).into(),
        })
        .await
        .unwrap();
    }
    for name in ["secret", "stored_secret", "review_distinct_secret.secret"] {
        assert!(
            Secret::query()
                .count_distinct(name)
                .await
                .unwrap_err()
                .to_string()
                .contains("encrypted")
        );
        assert!(
            Secret::query()
                .aggregates::<serde_json::Value>(&[Aggregate::count_distinct(name)])
                .await
                .is_err()
        );
        assert!(
            Secret::query()
                .having(Aggregate::count_distinct(name).gt(1))
                .get_json()
                .await
                .is_err()
        );
    }
    for query in [
        Secret::query().distinct(),
        Secret::query().select(vec!["*"]).distinct(),
        Secret::query().select(vec!["secret AS renamed"]).distinct(),
        Secret::query()
            .select_raw("stored_secret AS renamed")
            .distinct(),
    ] {
        assert!(
            query
                .get_json()
                .await
                .unwrap_err()
                .to_string()
                .contains("encrypted")
        );
    }
    assert!(
        Secret::query()
            .distinct()
            .pluck::<String>("secret")
            .await
            .is_err()
    );
    assert!(
        Secret::query()
            .select(vec!["secret AS renamed"])
            .distinct()
            .count_distinct("renamed")
            .await
            .is_err()
    );
    assert_eq!(Secret::query().count_distinct("id").await.unwrap(), 4);
    assert_eq!(
        Secret::query()
            .distinct()
            .pluck::<i64>("id")
            .await
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        Secret::query()
            .order_asc("id")
            .pluck::<String>("secret")
            .await
            .unwrap(),
        vec!["A", "A", "B", "B"]
    );
    Database::reset_global();
    TideConfig::reset();
}

#[tideorm::model(table = "review_projection_people")]
struct ProjectionPerson {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "display_name")]
    name: Option<String>,
}

#[tideorm::model(table = "review_projection_flags")]
struct ProjectionFlag {
    #[tideorm(primary_key)]
    id: i64,
    enabled: bool,
}

async fn projection_database() -> Database {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    db.exec_raw(
        "CREATE TABLE review_projection_people(id INTEGER PRIMARY KEY, display_name TEXT);
        INSERT INTO review_projection_people VALUES(1,'Other'),(7,'Alice');
        CREATE TABLE review_projection_flags(id INTEGER PRIMARY KEY, enabled INTEGER);
        INSERT INTO review_projection_flags VALUES(7,1);
        CREATE TABLE review_projection_unknown(id INTEGER PRIMARY KEY, enabled INTEGER);
        INSERT INTO review_projection_unknown VALUES(7,42)",
    )
    .await
    .unwrap();
    db
}

#[tokio::test]
async fn review_window_alias_cannot_replace_a_model_primary_key() {
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
async fn review_model_projection_requires_literal_database_output_names() {
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
async fn review_joined_json_reads_use_the_registered_source_type() {
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

#[tideorm::model(table = "review_query_shapes")]
struct QueryShape {
    #[tideorm(primary_key)]
    id: i64,
    name: Option<String>,
}

async fn query_shapes_database() -> Database {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_query_shapes(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO review_query_shapes VALUES(1,'A'),(2,'A'),(3,'B')").await.unwrap();
    db
}

#[tokio::test]
async fn review_subquery_projections_cannot_load_incomplete_models() {
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
async fn review_grouped_chunking_is_rejected_before_callbacks() {
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
async fn review_exists_preserves_aggregate_cardinality_and_having_aliases() {
    let db = query_shapes_database().await;
    for expression in ["COUNT(*) AS total", "SUM(id) AS total", "MIN(id) AS total"] {
        for id in [-1, 1] {
            let query = QueryShape::query_with(&db)
                .where_eq("id", id)
                .select_raw(expression);
            assert_eq!(query.clone().get_json().await.unwrap().len(), 1);
            assert!(query.clone().exists().await.unwrap());
            assert!(query.limit(0).offset(100).exists().await.unwrap());
        }
    }
    assert!(
        !QueryShape::query_with(&db)
            .where_eq("id", -1)
            .exists()
            .await
            .unwrap()
    );
    assert!(
        !QueryShape::query_with(&db)
            .select(vec!["id"])
            .where_eq("id", -1)
            .exists()
            .await
            .unwrap()
    );
    for (id, expected) in [(-1, false), (1, true)] {
        assert_eq!(
            QueryShape::query_with(&db)
                .where_eq("id", id)
                .select_raw("COUNT(*) AS total")
                .having("total > 0")
                .exists()
                .await
                .unwrap(),
            expected
        );
    }
}

#[tideorm::model(table = "review_users")]
struct User {
    #[tideorm(primary_key)]
    id: i64,
    name: String,
    active: bool,
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    posts: HasMany<Post>,
}

#[tideorm::model(table = "review_posts")]
struct Post {
    #[tideorm(primary_key)]
    id: i64,
    user_id: i64,
    title: String,
}

async fn database(name: &str, post_id: i64) -> Database {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN)")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_posts(id INTEGER PRIMARY KEY, user_id INTEGER, title TEXT)")
        .await
        .unwrap();
    db.exec_raw_with_params(
        "INSERT INTO review_users VALUES(1, ?, 1)",
        vec![name.into()],
    )
    .await
    .unwrap();
    db.exec_raw_with_params(
        "INSERT INTO review_posts VALUES(?, 1, ?)",
        vec![post_id.into(), name.into()],
    )
    .await
    .unwrap();
    db
}

#[tokio::test]
async fn review_relations_keep_the_read_database_including_cache_and_transactions() {
    let tenant = database("tenant", 10).await;
    let global = database("global", 20).await;
    Database::set_global(global.clone()).unwrap();
    let cache = tideorm::QueryCache::global();
    cache.enable();
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
    cache.disable();
    cache.clear();
    Database::reset_global();
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn review_cache_hits_do_not_replace_another_pools_baseline() {
    let tenant = database("tenant", 10).await;
    let global = database("global", 20).await;
    Database::set_global(global.clone()).unwrap();
    let cache = tideorm::QueryCache::global();
    cache.enable();
    let query = User::query_with(&tenant).cache(Duration::from_secs(60));
    query.clone().get().await.unwrap();
    let user = User::query_with(&global).first_or_fail().await.unwrap();
    assert_eq!(user.original_value("name").unwrap(), None);
    query.get().await.unwrap();
    // Different values in two pools remain ambiguous; neither replaces the other.
    assert_eq!(user.original_value("name").unwrap(), None);
    cache.disable();
    cache.clear();
    Database::reset_global();
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn review_rollback_does_not_undo_an_independent_commit() {
    let outer = database("outer", 20).await;
    let inner = database("inner", 10).await;
    inner
        .exec_raw("INSERT INTO review_users VALUES(42, 'BEFORE', 1)")
        .await
        .unwrap();
    User::query_with(&inner)
        .where_eq("id", 42)
        .get()
        .await
        .unwrap();
    let handle = inner.clone();
    let outcome = outer
        .transaction(move |_| {
            Box::pin(async move {
                handle
                    .transaction(|_| {
                        Box::pin(async {
                            Database::execute(
                                "UPDATE review_users SET name='COMMITTED' WHERE id=42",
                            )
                            .await?;
                            User::query().where_eq("id", 42).get().await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err::<(), _>(tideorm::Error::query("roll back outer"))
            })
        })
        .await;
    assert!(outcome.is_err());
    let model = User {
        id: 42,
        name: "COMMITTED".into(),
        active: true,
        posts: Default::default(),
    };
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(json!("COMMITTED")))
    );
    assert_eq!(
        inner
            .query_raw_json("SELECT name FROM review_users WHERE id=42")
            .await
            .unwrap()[0]["name"],
        "COMMITTED"
    );
    // A savepoint on the same database must still restore its parent's baseline.
    let nested = inner.clone();
    let outcome = inner
        .transaction(move |_| {
            Box::pin(async move {
                nested
                    .transaction(|_| {
                        Box::pin(async {
                            Database::execute(
                                "UPDATE review_users SET name='ROLLBACK' WHERE id=42",
                            )
                            .await?;
                            User::query().where_eq("id", 42).get().await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err::<(), _>(tideorm::Error::query("roll back parent"))
            })
        })
        .await;
    assert!(outcome.is_err());
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(json!("COMMITTED")))
    );
}

#[tokio::test]
async fn review_window_alias_uses_the_expression_type() {
    let db = database("tenant", 10).await;
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

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn review_expression_alias_is_not_decrypted() {
    #[tideorm::model(table = "review_secrets", encrypted = "secret")]
    struct Secret {
        #[tideorm(primary_key)]
        id: i64,
        secret: String,
    }
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_secrets(id INTEGER PRIMARY KEY, secret TEXT)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_secrets VALUES(1, 'not a ciphertext')")
        .await
        .unwrap();
    let rows = Secret::query_with(&db)
        .select_raw("COUNT(*) AS secret")
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["secret"], 1);
    // Direct encrypted-column projections must still reject plaintext.
    assert!(
        Secret::query_with(&db)
            .select(vec!["secret"])
            .get_json()
            .await
            .is_err()
    );
}

#[cfg(feature = "entity-manager")]
#[tokio::test]
async fn review_rolled_back_reads_leave_no_entity_manager_phantoms() {
    let db = database("committed", 10).await;
    let manager = tideorm::entity_manager::EntityManager::new(std::sync::Arc::new(db.clone()));
    manager.find::<User>(1).await.unwrap().unwrap();
    let inside = manager.clone();
    let nested = db.clone();
    let result = db.transaction(move |_| Box::pin(async move {
        nested.transaction(move |_| Box::pin(async move {
            Database::execute("INSERT INTO review_users VALUES(99, 'uncommitted', 1), (100, 'managed', 1)").await?;
            assert!(inside.find::<User>(99).await?.is_some());
            assert!(inside.find_managed::<User>(100).await?.is_some());
            Ok(())
        })).await?;
        Err::<(), _>(tideorm::Error::query("rollback parent"))
    })).await;
    assert!(result.is_err());
    assert!(manager.find::<User>(99).await.unwrap().is_none());
    assert!(manager.find_managed::<User>(100).await.unwrap().is_none());
    assert_eq!(
        manager.find::<User>(1).await.unwrap().unwrap().name,
        "committed"
    );
    // A committed read stays attached and usable.
    let inside = manager.clone();
    db.transaction(move |_| {
        Box::pin(async move {
            Database::execute("INSERT INTO review_users VALUES(99, 'committed insert', 1)").await?;
            inside.find::<User>(99).await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(
        manager.find::<User>(99).await.unwrap().unwrap().name,
        "committed insert"
    );
    manager.flush().await.unwrap();
}

#[tokio::test]
async fn review_mixed_join_and_comma_dependencies_invalidate_cached_results() {
    #[tideorm::model(table = "review_flags")]
    struct Flag {
        #[tideorm(primary_key)]
        id: i64,
        enabled: bool,
    }
    let db = database("cached", 10).await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE review_flags(id INTEGER PRIMARY KEY, enabled BOOLEAN)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_flags VALUES(1, 1)")
        .await
        .unwrap();
    let cache = tideorm::QueryCache::global();
    cache.enable();
    for join in [
        "JOIN review_posts p2 ON p.id=p2.id",
        "LEFT JOIN review_posts p2 USING(id)",
    ] {
        let mut flag = Flag::find_with(1, &db).await.unwrap().unwrap();
        flag.enabled = true;
        flag = flag.update().await.unwrap();
        let query = User::query_with(&db)
            .where_raw(&format!(
                "EXISTS (SELECT 1 FROM review_posts p {join}, review_flags f WHERE f.enabled=1)"
            ))
            .cache(Duration::from_secs(60));
        assert_eq!(query.clone().get().await.unwrap().len(), 1);
        assert_eq!(query.clone().get().await.unwrap().len(), 1);
        flag.enabled = false;
        flag.update().await.unwrap();
        assert!(query.get().await.unwrap().is_empty());
    }
    cache.disable();
    cache.clear();
}

#[tokio::test]
async fn review_distinct_aggregate_does_not_substitute_another_tables_column() {
    let db = database("aggregate", 100).await;
    let query = User::query_with(&db)
        .inner_join("review_posts", "review_users.id", "review_posts.user_id")
        .select(vec!["review_users.id"])
        .distinct();
    assert!(query.clone().sum::<i64>("review_posts.id").await.is_err());
    assert_eq!(query.sum::<i64>("review_users.id").await.unwrap(), 1);
    let query = User::query_with(&db)
        .inner_join("review_posts", "review_users.id", "review_posts.user_id")
        .select(vec!["review_posts.id AS post_id"])
        .distinct();
    assert_eq!(
        query.clone().sum::<i64>("review_posts.id").await.unwrap(),
        100
    );
    assert_eq!(query.sum::<i64>("post_id").await.unwrap(), 100);
}

#[tokio::test]
async fn review_joined_column_keeps_its_type_when_a_model_field_is_renamed() {
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
async fn review_aggregate_rejects_unknown_operands_instead_of_sqlite_literals() {
    let db = database("aggregate", 10).await;
    let query = User::query_with(&db);
    assert!(query.clone().sum::<f64>("ammount").await.is_err());
    assert!(query.clone().min::<String>("ammount").await.is_err());
    assert!(query.clone().max::<String>("ammount").await.is_err());
    assert!(query.clone().avg::<f64>("ammount").await.is_err());
    assert!(query.clone().count_distinct("ammount").await.is_err());
    assert!(query.clone().limit(1).sum::<i64>("ammount").await.is_err());
    assert!(
        query
            .clone()
            .aggregates::<serde_json::Value>(&[Aggregate::count(), Aggregate::sum("ammount")])
            .await
            .is_err()
    );
    assert_eq!(query.clone().sum::<i64>("id").await.unwrap(), 1);
    assert_eq!(
        query
            .aggregates::<serde_json::Value>(&[Aggregate::count()])
            .await
            .unwrap(),
        json!([1])
    );
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn review_bulk_rollback_restores_dirty_tracking_baselines() {
    #[tideorm::model(table = "review_bulk_snapshots")]
    struct Snapshot {
        #[tideorm(primary_key)]
        id: i64,
        name: String,
    }
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_bulk_snapshots(id INTEGER PRIMARY KEY, name TEXT)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_bulk_snapshots VALUES(1, 'before')")
        .await
        .unwrap();
    let model = Snapshot::find_with(1, &db).await.unwrap().unwrap();
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(json!("before")))
    );
    let nested = db.clone();
    let result = db
        .transaction(move |_| {
            Box::pin(async move {
                nested
                    .transaction(|_| {
                        Box::pin(async {
                            Snapshot::query().where_eq("id", 1).delete().await?;
                            Ok(())
                        })
                    })
                    .await?;
                Err::<(), _>(tideorm::Error::query("rollback bulk delete"))
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(
        model.original_value("name").unwrap(),
        Some(Some(json!("before")))
    );
    assert!(
        Snapshot::query_with(&db)
            .where_eq("id", 1)
            .exists()
            .await
            .unwrap()
    );
    db.transaction(|_| {
        Box::pin(async {
            Snapshot::query().where_eq("id", 1).delete().await?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(model.original_value("name").unwrap(), None);
}

#[tideorm::model(table = "review_arithmetic")]
struct Arithmetic {
    #[tideorm(primary_key)]
    id: i64,
    amount: Option<i64>,
}

async fn arithmetic_database() -> Database {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_arithmetic(id INTEGER PRIMARY KEY, amount INTEGER)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_arithmetic VALUES(1,9007199254740993)")
        .await
        .unwrap();
    db
}

#[tokio::test]
async fn review_nonfinite_bulk_arithmetic_never_writes() {
    let db = arithmetic_database().await;
    for factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for divide in [false, true] {
            let update = Arithmetic::query_with(&db).where_eq("id", 1).update_all();
            let update = if divide {
                update.divide("amount", factor)
            } else {
                update.multiply("amount", factor)
            };
            assert!(update.execute().await.is_err());
            assert_eq!(
                Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
                Some(9007199254740993)
            );
        }
    }
    assert!(
        Arithmetic::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .divide("amount", 0.0)
            .execute()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn review_sqlite_integer_scale_refuses_inexact_fallback() {
    let db = arithmetic_database().await;
    for divide in [false, true] {
        let update = Arithmetic::query_with(&db).where_eq("id", 1).update_all();
        let update = if divide {
            update.divide("amount", 1.0000000001)
        } else {
            update.multiply("amount", 1.0000000001)
        };
        let error = update.execute().await.unwrap_err();
        assert!(error.to_string().contains("exactly"), "{error}");
        assert_eq!(
            Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(9007199254740993)
        );
    }
    Arithmetic::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .multiply("amount", 0.5)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(4503599627370497)
    );
}

#[tokio::test]
async fn review_aggregate_aliases_do_not_inherit_model_field_types() {
    #[tideorm::model(table = "review_aliases")]
    struct Aliases {
        #[tideorm(primary_key)]
        id: i64,
        count: bool,
        agg_result: bool,
        aggregate_0: bool,
        exists_result: String,
    }
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_aliases(id INTEGER PRIMARY KEY, count BOOLEAN, agg_result BOOLEAN, aggregate_0 BOOLEAN, exists_result TEXT)").await.unwrap();
    db.exec_raw("INSERT INTO review_aliases VALUES(1,1,1,1,'yes'),(2,0,0,0,'no')")
        .await
        .unwrap();
    let query = Aliases::query_with(&db);
    assert_eq!(query.clone().count().await.unwrap(), 2);
    assert_eq!(query.clone().sum::<i64>("id").await.unwrap(), 3);
    assert_eq!(
        query
            .clone()
            .aggregates::<serde_json::Value>(&[Aggregate::count(), Aggregate::sum("id")])
            .await
            .unwrap(),
        json!([2, 3])
    );
    assert!(query.clone().exists().await.unwrap());
    assert_eq!(query.min::<bool>("count").await.unwrap(), Some(false));
}

#[tokio::test]
async fn review_sqlite_empty_contains_excludes_sql_null() {
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

#[cfg(feature = "fulltext")]
#[tokio::test]
async fn review_ranked_search_relations_keep_the_read_database() {
    let tenant = database("tenant", 10).await;
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
    let global = database("global", 20).await;
    Database::set_global(global).unwrap();
    assert_eq!(ranked[0].record.posts.load().await.unwrap()[0].id, 10);
}

#[tideorm::model(table = "review_bytes")]
struct BinaryRow {
    #[tideorm(primary_key)]
    id: i64,
    payload: Vec<u8>,
}

#[tokio::test]
async fn review_binary_equality_matches_stored_bytes() {
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
async fn review_upsert_results_keep_the_write_database() {
    let tenant = database("tenant", 10).await;
    let global = database("global", 20).await;
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
async fn review_bulk_insert_results_keep_the_write_database() {
    let tenant = database("tenant", 10).await;
    let global = database("global", 20).await;
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
async fn review_convenience_reads_retain_their_origin() {
    let tenant = database("tenant", 10).await;
    let other = database("other", 20).await;
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
async fn review_sqlite_array_filters_exclude_non_arrays_even_on_delete() {
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

#[tokio::test]
async fn review_sqlite_scaling_overflow_aborts_the_whole_update() {
    let db = arithmetic_database().await;
    for (value, factor, divide) in [
        (4611686018427387904_i64, 2.0, false),
        (-4611686018427387905, 2.0, false),
        (i64::MIN, -1.0, false),
        (i64::MIN, -1.0, true),
        (4611686018427387904, 0.5, true),
    ] {
        db.exec_raw("DELETE FROM review_arithmetic").await.unwrap();
        db.exec_raw_with_params(
            "INSERT INTO review_arithmetic VALUES(1,2),(2,?)",
            vec![value.into()],
        )
        .await
        .unwrap();
        let update = Arithmetic::query_with(&db).where_gte("id", 1).update_all();
        let result = if divide {
            update.divide("amount", factor)
        } else {
            update.multiply("amount", factor)
        }
        .execute()
        .await;
        assert!(
            result.is_err(),
            "overflow unexpectedly succeeded: {value}, {factor}, {divide}"
        );
        assert_eq!(
            Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(2)
        );
        assert_eq!(
            Arithmetic::find_with(2, &db).await.unwrap().unwrap().amount,
            Some(value)
        );
    }
    db.exec_raw("UPDATE review_arithmetic SET amount=NULL WHERE id=2")
        .await
        .unwrap();
    Arithmetic::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .multiply("amount", 2.0)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(4)
    );
    assert_eq!(
        Arithmetic::find_with(2, &db).await.unwrap().unwrap().amount,
        None
    );
}

#[tokio::test]
async fn review_writable_pragmas_invalidate_cached_reads() {
    // A pragma changes one connection's settings; pin this test to one pool connection.
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_users(id INTEGER PRIMARY KEY,name TEXT,active BOOLEAN)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_users VALUES(1,'Alice',1),(2,'alice',1)")
        .await
        .unwrap();
    let cache = tideorm::QueryCache::global();
    cache.enable();
    cache.clear();
    let query = User::query_with(&db)
        .where_like("name", "alice")
        .cache(Duration::from_secs(60));
    assert_eq!(query.clone().get().await.unwrap().len(), 2);
    db.exec_raw("PRAGMA case_sensitive_like=ON").await.unwrap();
    assert_eq!(query.get().await.unwrap().len(), 1);
    cache.disable();
    cache.clear();
}

#[tokio::test]
async fn review_null_related_keys_never_change_the_pivot() {
    #[tideorm::model(table = "review_nullable_pivot")]
    struct Pivot {
        #[tideorm(primary_key)]
        id: i64,
        owner_id: i64,
        related_id: Option<i64>,
    }
    let db = database("owner", 10).await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE review_nullable_pivot(id INTEGER PRIMARY KEY,owner_id INTEGER,related_id INTEGER, UNIQUE(owner_id,related_id))").await.unwrap();
    db.exec_raw("INSERT INTO review_nullable_pivot VALUES(1,1,10)")
        .await
        .unwrap();
    let relation = HasManyThrough::<Post, Pivot>::new(
        "owner_id",
        "related_id",
        "id",
        "id",
        "review_nullable_pivot",
    )
    .with_parent_pk(json!(1));
    assert!(relation.sync(vec![Some(10_i64), None]).await.is_err());
    assert!(relation.attach(None::<i64>).await.is_err());
    assert!(relation.attach(None::<i64>).await.is_err());
    let rows = Pivot::all().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].related_id, Some(10));
    Database::reset_global();
}

#[tokio::test]
async fn review_integer_addition_and_subtraction_overflow_roll_back_all_rows() {
    let db = arithmetic_database().await;
    for (amount, increment, operand) in [
        (i64::MAX, true, 1),
        (i64::MIN, false, 1),
        (i64::MAX, false, -1),
        (i64::MIN, true, -1),
    ] {
        db.exec_raw("DELETE FROM review_arithmetic").await.unwrap();
        db.exec_raw_with_params(
            "INSERT INTO review_arithmetic(id,amount) VALUES(1,2),(2,?)",
            vec![amount.into()],
        )
        .await
        .unwrap();
        let query = Arithmetic::query_with(&db).where_gte("id", 1).update_all();
        assert!(
            if increment {
                query.increment("amount", operand)
            } else {
                query.decrement("amount", operand)
            }
            .execute()
            .await
            .is_err()
        );
        assert_eq!(
            Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(2)
        );
        assert_eq!(
            Arithmetic::find_with(2, &db).await.unwrap().unwrap().amount,
            Some(amount)
        );
    }
}

#[tokio::test]
async fn review_json_assignment_distinguishes_json_null_from_option_none() {
    let db = Database::builder()
        .url("sqlite::memory:")
        .build()
        .await
        .unwrap();
    #[tideorm::model(table = "review_json_null_assignment")]
    struct Document {
        #[tideorm(primary_key)]
        id: i64,
        payload: Option<serde_json::Value>,
    }
    db.exec_raw("CREATE TABLE review_json_null_assignment(id BIGINT PRIMARY KEY,payload JSON)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_json_null_assignment VALUES(1,'{}')")
        .await
        .unwrap();
    Document::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("payload", serde_json::Value::Null)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Document::find_with(1, &db).await.unwrap().unwrap().payload,
        Some(serde_json::Value::Null)
    );
    Document::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("payload", None::<serde_json::Value>)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Document::find_with(1, &db).await.unwrap().unwrap().payload,
        None
    );
    Document::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .coalesce("payload", Some(serde_json::Value::Null))
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Document::find_with(1, &db).await.unwrap().unwrap().payload,
        Some(serde_json::Value::Null)
    );
    db.exec_raw("DROP TABLE review_json_null_assignment")
        .await
        .unwrap();
}

#[tideorm::model(table = "review_narrow_arithmetic")]
struct NarrowArithmetic {
    #[tideorm(primary_key)]
    id: i64,
    signed: Option<i32>,
    unsigned: u32,
}

#[tokio::test]
async fn review_sqlite_arithmetic_respects_model_integer_widths() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_narrow_arithmetic(id INTEGER PRIMARY KEY, signed INTEGER, unsigned INTEGER)").await.unwrap();
    db.exec_raw("INSERT INTO review_narrow_arithmetic VALUES(1, 2, 2), (2, 2147483647, 0)")
        .await
        .unwrap();
    for operation in 0..4 {
        let query = NarrowArithmetic::query_with(&db)
            .where_gte("id", 1)
            .update_all();
        let query = match operation {
            0 => query.increment("signed", 1),
            1 => query.decrement("unsigned", 1),
            2 => query.multiply("signed", 2.0),
            _ => query.divide("signed", 0.5),
        };
        assert!(query.execute().await.is_err(), "operation {operation}");
        assert_eq!(
            NarrowArithmetic::find_with(1, &db)
                .await
                .unwrap()
                .unwrap()
                .signed,
            Some(2)
        );
        let row = NarrowArithmetic::find_with(2, &db).await.unwrap().unwrap();
        assert_eq!(row.signed, Some(i32::MAX));
        assert_eq!(row.unsigned, 0);
    }
    db.exec_raw("UPDATE review_narrow_arithmetic SET signed=NULL WHERE id=2")
        .await
        .unwrap();
    NarrowArithmetic::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .increment("signed", 1)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        NarrowArithmetic::find_with(2, &db)
            .await
            .unwrap()
            .unwrap()
            .signed,
        None
    );
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
async fn review_sqlite_large_json_integer_filters_and_deletes_are_exact() {
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
async fn review_sqlite_array_removal_keeps_original_json_numbers() {
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
async fn review_nonfinite_array_values_never_become_json_null() {
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
async fn review_aggregate_projection_alias_uses_source_type() {
    let db = database("Alice", 1).await;
    db.exec_raw("UPDATE review_users SET id=7").await.unwrap();
    let query = User::query_with(&db)
        .select(vec!["id AS active"])
        .distinct();
    assert_eq!(query.clone().min::<i64>("active").await.unwrap(), Some(7));
    assert_eq!(query.clone().max::<i64>("active").await.unwrap(), Some(7));
    assert_eq!(
        query
            .aggregates::<(i64, i64)>(&[Aggregate::min("active"), Aggregate::max("active")])
            .await
            .unwrap(),
        (7, 7)
    );
    assert_eq!(
        User::query_with(&db)
            .select_raw("id+1 AS active")
            .distinct()
            .min::<i64>("active")
            .await
            .unwrap(),
        Some(8)
    );
}

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn review_aggregate_projection_alias_keeps_encryption_source() {
    #[tideorm::model(table = "review_aggregate_secret", encrypted = "secret")]
    struct Secret {
        #[tideorm(primary_key)]
        id: i64,
        secret: String,
    }
    Database::reset_global();
    TideConfig::reset();
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .encryption_key("aggregate-alias-regression-key")
        .connect()
        .await
        .unwrap();
    Database::execute("CREATE TABLE review_aggregate_secret(id INTEGER PRIMARY KEY, secret TEXT)")
        .await
        .unwrap();
    Secret::create(Secret {
        id: 7,
        secret: "plaintext".into(),
    })
    .await
    .unwrap();
    let selected = Secret::query().select(vec!["secret AS renamed"]);
    let query = selected.clone().union_all(selected);
    assert_eq!(
        query
            .clone()
            .min::<String>("renamed")
            .await
            .unwrap()
            .as_deref(),
        Some("plaintext")
    );
    assert_eq!(
        query
            .aggregates::<(String, String)>(&[Aggregate::min("renamed"), Aggregate::max("renamed")])
            .await
            .unwrap(),
        ("plaintext".into(), "plaintext".into())
    );
    Database::reset_global();
    TideConfig::reset();
}

#[tokio::test]
async fn review_json_set_rejects_nonfinite_values_and_keeps_intentional_null() {
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
async fn review_sqlite_export_keeps_names_similar_to_reserved_prefix() {
    let source = Database::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE sqlitex_items(id INTEGER PRIMARY KEY, name TEXT)",
        "CREATE TABLE normal(id INTEGER)",
        "CREATE INDEX sqlitex_name ON sqlitex_items(name)",
        "CREATE TRIGGER sqlitex_insert AFTER INSERT ON sqlitex_items BEGIN INSERT INTO normal VALUES(NEW.id); END",
    ] {
        source.exec_raw(sql).await.unwrap();
    }
    Database::set_global(source).unwrap();
    let path = std::env::temp_dir().join(format!("tideorm-prefix-{}.sql", uuid::Uuid::new_v4()));
    tideorm::schema::SchemaWriter::write_schema(&path)
        .await
        .unwrap();
    let script = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let restored = Database::connect("sqlite::memory:").await.unwrap();
    restored.exec_raw(&script).await.unwrap();
    restored
        .exec_raw("INSERT INTO sqlitex_items VALUES(7,'kept')")
        .await
        .unwrap();
    assert_eq!(
        restored
            .query_raw_json("SELECT id FROM normal")
            .await
            .unwrap()[0]["id"],
        7
    );
    assert_eq!(
        restored
            .query_raw_json(
                "SELECT count(*) AS n FROM sqlite_master WHERE type='index' AND name='sqlitex_name'"
            )
            .await
            .unwrap()[0]["n"],
        1
    );
    Database::reset_global();
}

#[tideorm::model(table = "review_cascade_parent")]
struct CascadeParent {
    #[tideorm(primary_key)]
    id: i64,
    code: i64,
}
#[tideorm::model(table = "review_cascade_child")]
struct CascadeChild {
    #[tideorm(primary_key)]
    id: i64,
    parent_code: i64,
}
#[tideorm::model(table = "review_cascade_audit")]
struct CascadeAudit {
    #[tideorm(primary_key)]
    id: i64,
    updates: i64,
}

#[tokio::test]
async fn review_failed_writes_invalidate_trigger_side_effects() {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE review_cascade_parent(id INTEGER PRIMARY KEY,code INTEGER); CREATE TABLE review_cascade_audit(id INTEGER PRIMARY KEY,updates INTEGER); INSERT INTO review_cascade_parent VALUES(1,10); INSERT INTO review_cascade_audit VALUES(1,0)")
        .await.unwrap();
    for event in ["INSERT", "UPDATE", "DELETE"] {
        db.exec_raw(&format!("CREATE TRIGGER fail_{event} BEFORE {event} ON review_cascade_parent BEGIN UPDATE review_cascade_audit SET updates=updates+1; SELECT RAISE(FAIL, 'write refused after side effect'); END"))
            .await.unwrap();
    }
    let cache = QueryCache::global();
    cache.enable();
    let audit = || CascadeAudit::query().cache(Duration::from_secs(60));
    for operation in 0..8 {
        let before = audit().first_or_fail().await.unwrap().updates;
        let result = match operation {
            0 => CascadeParent::query()
                .where_eq("id", 1)
                .update_all()
                .set("code", 20)
                .execute()
                .await
                .map(|_| ()),
            1 => CascadeParent::query()
                .where_eq("id", 1)
                .delete()
                .await
                .map(|_| ()),
            2 => CascadeParent { id: 1, code: 20 }.update().await.map(|_| ()),
            3 => CascadeParent::create(CascadeParent { id: 2, code: 20 })
                .await
                .map(|_| ()),
            4 => CascadeParent::on_conflict(vec!["id"])
                .insert(CascadeParent { id: 1, code: 20 })
                .await
                .map(|_| ()),
            5 => CascadeParent::insert_all(vec![CascadeParent { id: 2, code: 20 }])
                .await
                .map(|_| ()),
            6 => CascadeParent { id: 1, code: 20 }.delete().await.map(|_| ()),
            _ => CascadeParent::query()
                .where_eq("id", 1)
                .update_all()
                .set("code", 20)
                .execute_returning()
                .await
                .map(|_| ()),
        };
        assert!(result.is_err(), "operation {operation}: {result:?}");
        let stored = CascadeAudit::query().first_or_fail().await.unwrap().updates;
        assert_eq!(
            stored,
            before + 1,
            "SQLite keeps the trigger's write after RAISE(FAIL)"
        );
        assert_eq!(
            audit().first_or_fail().await.unwrap().updates,
            stored,
            "operation {operation} returned an error but changed the database"
        );
    }
    let before = audit().first_or_fail().await.unwrap().updates;
    db.transaction(|_| {
        Box::pin(async {
            assert!(
                CascadeParent::query()
                    .where_eq("id", 1)
                    .delete()
                    .await
                    .is_err()
            );
            // Another connection can cache pre-commit values after the first flush.
            QueryCache::global().set(
                "review_failed_write_commit",
                &1,
                None,
                "review_cascade_audit",
            )?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(cache.get::<i32>("review_failed_write_commit"), None);
    assert_eq!(audit().first_or_fail().await.unwrap().updates, before + 1);
    cache.disable();
    cache.clear();
    Database::reset_global();
}

#[tokio::test]
async fn review_empty_selection_loads_the_default_model_columns() {
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
async fn review_failed_pivot_insert_invalidates_trigger_side_effects() {
    #[tideorm::model(table = "review_failed_pivot")]
    struct Pivot {
        #[tideorm(primary_key)]
        id: i64,
        owner_id: i64,
        related_id: i64,
    }
    let db = database("owner", 10).await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE review_failed_pivot(id INTEGER PRIMARY KEY,owner_id INTEGER,related_id INTEGER, UNIQUE(owner_id,related_id)); CREATE TABLE review_cascade_audit(id INTEGER PRIMARY KEY,updates INTEGER); INSERT INTO review_cascade_audit VALUES(1,0);").await.unwrap();
    let relation = HasManyThrough::<Post, Pivot>::new(
        "owner_id",
        "related_id",
        "id",
        "id",
        "review_failed_pivot",
    )
    .with_parent_pk(json!(1));
    let cache = QueryCache::global();
    cache.enable();
    let audit = || CascadeAudit::query().cache(Duration::from_secs(60));
    for action in ["FAIL, 'refused'", "IGNORE"] {
        db.exec_raw(&format!("CREATE TRIGGER fail_pivot BEFORE INSERT ON review_failed_pivot BEGIN UPDATE review_cascade_audit SET updates=updates+1; SELECT RAISE({action}); END")).await.unwrap();
        let before = audit().first_or_fail().await.unwrap().updates;
        assert!(relation.attach(1).await.is_err());
        let actual = CascadeAudit::query().first_or_fail().await.unwrap().updates;
        assert_eq!(actual, before + 1);
        assert_eq!(audit().first_or_fail().await.unwrap().updates, actual);
        db.exec_raw("DROP TRIGGER fail_pivot").await.unwrap();
    }
    cache.disable();
    cache.clear();
    Database::reset_global();
}

#[tokio::test]
async fn review_database_side_writes_invalidate_all_cached_tables_and_replay_on_commit() {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    for sql in [
        "PRAGMA foreign_keys=ON",
        "CREATE TABLE review_cascade_parent(id INTEGER PRIMARY KEY,code INTEGER UNIQUE)",
        "CREATE TABLE review_cascade_child(id INTEGER PRIMARY KEY,parent_code INTEGER REFERENCES review_cascade_parent(code) ON UPDATE CASCADE ON DELETE CASCADE)",
        "CREATE TABLE review_cascade_audit(id INTEGER PRIMARY KEY,updates INTEGER)",
        "INSERT INTO review_cascade_parent VALUES(1,10)",
        "INSERT INTO review_cascade_child VALUES(1,10)",
        "INSERT INTO review_cascade_audit VALUES(1,0)",
        "CREATE TRIGGER review_parent_updated AFTER UPDATE ON review_cascade_parent BEGIN UPDATE review_cascade_audit SET updates=updates+1; END",
    ] {
        db.exec_raw(sql).await.unwrap();
    }
    let cache = QueryCache::global();
    cache.enable();
    cache.clear();
    let children = || CascadeChild::query().cache(Duration::from_secs(60));
    let audit = || CascadeAudit::query().cache(Duration::from_secs(60));
    assert_eq!(children().first_or_fail().await.unwrap().parent_code, 10);
    assert_eq!(audit().first_or_fail().await.unwrap().updates, 0);
    let mut parent = CascadeParent::find(1).await.unwrap().unwrap();
    parent.code = 20;
    parent.update().await.unwrap();
    assert_eq!(children().first_or_fail().await.unwrap().parent_code, 20);
    assert_eq!(audit().first_or_fail().await.unwrap().updates, 1);
    db.transaction(|_| {
        Box::pin(async {
            CascadeParent::query()
                .where_eq("id", 1)
                .update_all()
                .set("code", 30)
                .execute()
                .await?;
            // Simulate another connection caching pre-commit data after the first flush.
            QueryCache::global().set("review_before_commit", &1, None, "review_cascade_child")?;
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(cache.get::<i32>("review_before_commit"), None);
    assert_eq!(children().first_or_fail().await.unwrap().parent_code, 30);
    assert_eq!(audit().first_or_fail().await.unwrap().updates, 2);
    let rolled_back: tideorm::Result<()> = db
        .transaction(|_| {
            Box::pin(async {
                CascadeParent::query().where_eq("id", 1).delete().await?;
                Err(tideorm::Error::query("rollback"))
            })
        })
        .await;
    assert!(rolled_back.is_err());
    assert_eq!(children().get().await.unwrap().len(), 1);
    // Both model CRUD and query-builder deletes must flush child results.
    CascadeParent::find(1)
        .await
        .unwrap()
        .unwrap()
        .delete()
        .await
        .unwrap();
    assert!(children().get().await.unwrap().is_empty());
    CascadeParent::create(CascadeParent { id: 2, code: 40 })
        .await
        .unwrap();
    CascadeChild::create(CascadeChild {
        id: 2,
        parent_code: 40,
    })
    .await
    .unwrap();
    assert_eq!(children().get().await.unwrap().len(), 1);
    db.transaction(|_| Box::pin(async { CascadeParent::query().where_eq("id", 2).delete().await }))
        .await
        .unwrap();
    assert!(children().get().await.unwrap().is_empty());
    Database::reset_global();
}
