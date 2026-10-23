//! Cached reads are invalidated by writes the ORM did not make itself: triggers,
//! pragmas, joined tables and failed statements.

use crate::fixtures::{
    Post, User, file_database, global_memory_database, memory_database, users_and_posts,
};
use std::time::Duration;
use tideorm::prelude::*;

#[tideorm::model(table = "flattened_cache_rows")]
#[derive(serde::Serialize)]
struct FlattenedCacheRow {
    #[tideorm(primary_key)]
    id: i64,
    #[serde(flatten)]
    extra: Option<serde_json::Value>,
}

#[tokio::test]
async fn flattened_serde_models_never_lose_fields_on_cached_reads() {
    let db = memory_database(
        "CREATE TABLE flattened_cache_rows(id INTEGER PRIMARY KEY, extra JSON); \
         INSERT INTO flattened_cache_rows VALUES(1, '{\"title\":\"kept\"}')",
    )
    .await;
    let cache = crate::fixtures::global_cache();
    let hits = cache.stats().hits;
    let query = FlattenedCacheRow::query_with(&db).cache(Duration::from_secs(60));
    let first = query.clone().first().await.unwrap().unwrap();
    let second = query.first().await.unwrap().unwrap();
    assert_eq!(first.extra, Some(serde_json::json!({"title": "kept"})));
    assert_eq!(second.extra, first.extra);
    assert_eq!(cache.stats().hits, hits);
    assert_eq!(cache.stats().entries, 0);
}

/// External writers use table invalidation after committing. SQL spelling and
/// parenthesized table factors must not leave their old rows cached.
#[tokio::test]
async fn table_invalidation_reaches_parenthesized_and_mixed_case_dependencies() {
    use tideorm::orm::ConnectionTrait;
    let db = memory_database(
        "CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN); \
         CREATE TABLE posts(id INTEGER PRIMARY KEY, user_id INTEGER); \
         CREATE TABLE flags(id INTEGER PRIMARY KEY); \
         INSERT INTO review_users VALUES(1, 'Ada', 1); \
         INSERT INTO posts VALUES(1, 1); INSERT INTO flags VALUES(1)",
    )
    .await;
    let cache = crate::fixtures::global_cache();
    for source in [
        "(posts JOIN flags ON flags.id = posts.id)",
        "((posts JOIN flags ON flags.id = posts.id))",
        "(flags, posts)",
        "POSTS",
        "\"POSTS\"",
    ] {
        db.__internal_connection()
            .unwrap()
            .execute_unprepared("UPDATE posts SET user_id = 1")
            .await
            .unwrap();
        cache.clear();
        let query = User::query_with(&db)
            .where_raw(&format!(
                "EXISTS (SELECT 1 FROM {source} WHERE posts.user_id = review_users.id)"
            ))
            .cache(Duration::from_secs(60));
        assert_eq!(query.clone().get().await.unwrap().len(), 1, "{source}");
        db.__internal_connection()
            .unwrap()
            .execute_unprepared("UPDATE posts SET user_id = 99")
            .await
            .unwrap();
        cache.invalidate_model("posts");
        assert!(query.get().await.unwrap().is_empty(), "{source}");
    }
}

#[tokio::test]
async fn writes_and_commits_preserve_an_unrelated_databases_cache() {
    let (a, _file) = file_database("cache-scope").await;
    a.exec_raw(crate::fixtures::OLD_USER).await.unwrap();
    let b = users_and_posts("other", 20).await;
    let cache = crate::fixtures::global_cache();
    let qa = User::query_with(&a).cache(Duration::from_secs(60));
    let qb = User::query_with(&b).cache(Duration::from_secs(60));
    qa.clone().get().await.unwrap();
    qb.clone().get().await.unwrap();
    let writer = a.clone();
    let reader = a.clone();
    a.transaction(move |_| {
        Box::pin(async move {
            writer
                .exec_raw("UPDATE review_users SET name = 'new' WHERE id = 1")
                .await?;
            // Another connection can cache the pre-commit row. Commit must evict
            // it again without evicting B.
            let before = tokio::spawn(async move {
                User::query_with(&reader)
                    .cache(Duration::from_secs(60))
                    .get()
                    .await
            })
            .await
            .unwrap()?;
            assert_eq!(before[0].name, "old");
            Ok(())
        })
    })
    .await
    .unwrap();
    assert_eq!(cache.stats().entries, 1);
    let hits = cache.stats().hits;
    assert_eq!(qb.get().await.unwrap()[0].name, "other");
    assert_eq!(cache.stats().hits, hits + 1);
    assert_eq!(qa.get().await.unwrap()[0].name, "new");
    a.exec_raw("UPDATE missing_table SET name = 'failure'")
        .await
        .unwrap_err();
    assert_eq!(
        cache.stats().entries,
        1,
        "failed writes stay in their database too"
    );
}

#[tokio::test]
async fn separate_pools_on_one_file_invalidate_each_other() {
    let (writer, file) = file_database("shared-cache-scope").await;
    writer.exec_raw(crate::fixtures::OLD_USER).await.unwrap();
    let reader =
        crate::fixtures::single_connection(&format!("sqlite://{}?mode=rw", file.display())).await;
    let cache = crate::fixtures::global_cache();
    let query = User::query_with(&reader).cache(Duration::from_secs(60));
    assert_eq!(query.clone().get().await.unwrap()[0].name, "old");
    writer
        .exec_raw("UPDATE review_users SET name = 'new' WHERE id = 1")
        .await
        .unwrap();
    assert!(cache.is_empty());
    assert_eq!(query.get().await.unwrap()[0].name, "new");
    crate::fixtures::close(&reader).await;
    crate::fixtures::close(&writer).await;
}

#[tokio::test]
async fn attached_databases_keep_conservative_invalidation() {
    for attach in [
        "ATTACH DATABASE ? AS other",
        "\u{feff}ATTACH DATABASE ? AS other",
        "/* outer /* inner */ ATTACH DATABASE ? AS other",
    ] {
        check_attachment_invalidation(attach).await;
    }
}

async fn check_attachment_invalidation(attach: &str) {
    let (file_db, file) = file_database("attached-cache-scope").await;
    file_db.exec_raw(crate::fixtures::OLD_USER).await.unwrap();
    // SQLITE_OPEN_MEMORY also affects ATTACH unless its URI overrides the
    // mode. Use a file-backed, single-connection pool for this file attachment.
    let attaching_file = crate::fixtures::TempFile::new("attaching-cache-scope", "sqlite");
    let attaching = crate::fixtures::single_connection(&format!(
        "sqlite://{}?mode=rwc",
        attaching_file.display()
    ))
    .await;
    let cache = crate::fixtures::global_cache();
    let query = User::query_with(&file_db).cache(Duration::from_secs(60));
    query.clone().get().await.unwrap();
    attaching
        .exec_raw_with_params(attach, vec![file.to_string_lossy().to_string().into()])
        .await
        .unwrap();
    query.clone().get().await.unwrap();
    attaching
        .exec_raw("UPDATE other.review_users SET name = 'new' WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(query.get().await.unwrap()[0].name, "new");
    assert_eq!(cache.stats().entries, 1);
    crate::fixtures::close(&attaching).await;
    crate::fixtures::close(&file_db).await;
}

#[tokio::test]
async fn named_memory_cache_scopes_follow_shared_and_private_storage() {
    for mode in ["shared", "private"] {
        let url = format!(
            "sqlite:file:cache-scope-{}?mode=memory&cache={mode}",
            Uuid::new_v4()
        );
        let a = crate::fixtures::single_connection(&url).await;
        let b = crate::fixtures::single_connection(&url).await;
        a.exec_raw(crate::fixtures::OLD_USER).await.unwrap();
        if mode == "private" {
            b.exec_raw(crate::fixtures::OLD_USER).await.unwrap();
        }
        let cache = crate::fixtures::global_cache();
        let query = User::query_with(&b).cache(Duration::from_secs(60));
        query.clone().get().await.unwrap();
        let hits = cache.stats().hits;
        a.exec_raw("UPDATE review_users SET name = 'new' WHERE id = 1")
            .await
            .unwrap();
        let actual = query.get().await.unwrap();
        if mode == "shared" {
            assert_eq!(actual[0].name, "new");
            assert_eq!(cache.stats().hits, hits);
        } else {
            assert_eq!(actual[0].name, "old");
            assert_eq!(cache.stats().hits, hits + 1);
        }
        crate::fixtures::close(&a).await;
        crate::fixtures::close(&b).await;
    }
}

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn cache_hits_do_not_replace_another_pools_baseline() {
    let tenant = users_and_posts("tenant", 10).await;
    let global = users_and_posts("global", 20).await;
    Database::set_global(global.clone()).unwrap();
    let _cache = crate::fixtures::global_cache();
    let query = User::query_with(&tenant).cache(Duration::from_secs(60));
    query.clone().get().await.unwrap();
    let user = User::query_with(&global).first_or_fail().await.unwrap();
    assert_eq!(user.original_value("name").unwrap(), None);
    query.get().await.unwrap();
    // Different values in two pools remain ambiguous; neither replaces the other.
    assert_eq!(user.original_value("name").unwrap(), None);
    Database::reset_global();
}

/// A write run on database A from inside B's transaction, itself inside A's,
/// is A's transaction's to replay when it commits: a read another connection
/// cached while A was still open is dropped then, not served stale.
#[tokio::test]
async fn a_write_is_replayed_by_the_transaction_it_ran_in() {
    let (a, _a_file) = file_database("replay-a").await;
    let (b, _b_file) = file_database("replay-b").await;
    a.exec_raw(crate::fixtures::OLD_USER).await.unwrap();
    let _cache = crate::fixtures::global_cache();
    let read = |db: Database| async move {
        User::query_with(&db)
            .cache(Duration::from_secs(60))
            .get()
            .await
            .map(|users| users[0].name.clone())
    };

    let (writer, reader) = (a.clone(), a.clone());
    let result: tideorm::Result<()> = a
        .transaction(move |_| {
            Box::pin(async move {
                b.transaction(move |_| {
                    Box::pin(async move {
                        writer
                            .exec_raw("UPDATE review_users SET name = 'new' WHERE id = 1")
                            .await?;
                        Ok(())
                    })
                })
                .await?;
                // Another connection still reads the row A has not committed.
                let seen = tokio::spawn(read(reader)).await.unwrap()?;
                assert_eq!(seen, "old");
                Ok(())
            })
        })
        .await;
    result.unwrap();
    let after = read(a.clone()).await;
    assert_eq!(after.unwrap(), "new");
    drop(a);
}

/// A commit that fails may have committed all the same (a connection lost
/// once the server had it), so what the transaction invalidated is dropped
/// again then too: a read another connection cached while it was open does
/// not outlive it.
#[tokio::test]
async fn a_failed_commit_replays_what_the_transaction_invalidated() {
    let (db, _file) = file_database("failed-commit").await;
    db.exec_raw(
        "CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN); \
         CREATE TABLE review_posts(id INTEGER PRIMARY KEY, title TEXT, \
             user_id INTEGER REFERENCES review_users(id) DEFERRABLE INITIALLY DEFERRED); \
         INSERT INTO review_users VALUES (1, 'old', 1)",
    )
    .await
    .unwrap();
    let cache = crate::fixtures::global_cache();

    let (writer, reader) = (db.clone(), db.clone());
    let result: tideorm::Result<()> = db
        .transaction(move |_| {
            Box::pin(async move {
                writer
                    .exec_raw("UPDATE review_users SET name = 'new' WHERE id = 1")
                    .await?;
                // Checked when the transaction commits, which then fails.
                writer
                    .exec_raw("INSERT INTO review_posts VALUES (1, 'orphan', 99)")
                    .await?;
                let cached = tokio::spawn(async move {
                    User::query_with(&reader)
                        .cache(Duration::from_secs(60))
                        .get()
                        .await
                })
                .await
                .unwrap()?;
                assert_eq!(cached[0].name, "old");
                Ok(())
            })
        })
        .await;
    let entries = cache.stats().entries;
    assert!(
        result.is_err(),
        "the commit should fail on the deferred key"
    );
    assert_eq!(entries, 0);
    drop(db);
}

/// A write cancelled once it was sent still invalidates the cached reads:
/// the database applies it all the same, and a read cached before must not
/// outlive it. Raw SQL and the query builder alike.
#[tokio::test]
async fn a_cancelled_write_still_invalidates_cached_reads() {
    let slow = crate::fixtures::slow_count(3_000_000);
    let db = memory_database(
        "CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN); \
         INSERT INTO review_users VALUES (1, 'old', 1), (2, 'kept', 1)",
    )
    .await;
    let _cache = crate::fixtures::global_cache();
    let names = || async {
        let mut names: Vec<String> = User::query_with(&db)
            .cache(Duration::from_secs(60))
            .get()
            .await
            .unwrap()
            .into_iter()
            .map(|user| user.name)
            .collect();
        names.sort();
        names
    };
    // Waits for the cancelled statement, which holds the only connection,
    // without touching the cache.
    let settled = || db.__raw_json_with_params("SELECT 1", Vec::new());

    assert_eq!(names().await, ["kept", "old"]);
    let sql = format!("UPDATE review_users SET name = 'new' WHERE id = 1 AND {slow} > 0");
    let update = db.exec_raw(&sql);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), update)
            .await
            .is_err()
    );
    settled().await.unwrap();
    let after_update = names().await;

    let delete = User::query_with(&db)
        .where_eq("id", 2)
        .where_raw(&format!("{slow} > 0"))
        .delete();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), delete)
            .await
            .is_err()
    );
    settled().await.unwrap();
    let after_delete = names().await;
    assert_eq!(after_update, ["kept", "new"]);
    assert_eq!(after_delete, ["new"]);
}

#[tokio::test]
async fn mixed_join_and_comma_dependencies_invalidate_cached_results() {
    #[tideorm::model(table = "review_flags")]
    struct Flag {
        #[tideorm(primary_key)]
        id: i64,
        enabled: bool,
    }
    let db = users_and_posts("cached", 10).await;
    Database::set_global(db.clone()).unwrap();
    db.exec_raw("CREATE TABLE review_flags(id INTEGER PRIMARY KEY, enabled BOOLEAN)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_flags VALUES(1, 1)")
        .await
        .unwrap();
    let _cache = crate::fixtures::global_cache();
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
}

#[tokio::test]
async fn writable_pragmas_invalidate_cached_reads() {
    // A pragma changes one connection's settings; pin this test to one pool connection.
    let db = memory_database(
        "CREATE TABLE review_users(id INTEGER PRIMARY KEY,name TEXT,active BOOLEAN); \
         INSERT INTO review_users VALUES(1,'Alice',1),(2,'alice',1)",
    )
    .await;
    let _cache = crate::fixtures::global_cache();
    let query = User::query_with(&db)
        .where_like("name", "alice")
        .cache(Duration::from_secs(60));
    assert_eq!(query.clone().get().await.unwrap().len(), 2);
    db.exec_raw("PRAGMA case_sensitive_like=ON").await.unwrap();
    assert_eq!(query.get().await.unwrap().len(), 1);
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
async fn failed_writes_invalidate_trigger_side_effects() {
    let db = global_memory_database("CREATE TABLE review_cascade_parent(id INTEGER PRIMARY KEY,code INTEGER); CREATE TABLE review_cascade_audit(id INTEGER PRIMARY KEY,updates INTEGER); INSERT INTO review_cascade_parent VALUES(1,10); INSERT INTO review_cascade_audit VALUES(1,0)").await;
    for event in ["INSERT", "UPDATE", "DELETE"] {
        db.exec_raw(&format!("CREATE TRIGGER fail_{event} BEFORE {event} ON review_cascade_parent BEGIN UPDATE review_cascade_audit SET updates=updates+1; SELECT RAISE(FAIL, 'write refused after side effect'); END"))
            .await.unwrap();
    }
    let cache = crate::fixtures::global_cache();
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
    Database::reset_global();
}

#[tokio::test]
async fn failed_pivot_insert_invalidates_trigger_side_effects() {
    #[tideorm::model(table = "review_failed_pivot")]
    struct Pivot {
        #[tideorm(primary_key)]
        id: i64,
        owner_id: i64,
        related_id: i64,
    }
    let db = users_and_posts("owner", 10).await;
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
    let _cache = crate::fixtures::global_cache();
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
    Database::reset_global();
}

#[tokio::test]
async fn database_side_writes_invalidate_all_cached_tables_and_replay_on_commit() {
    let db = global_memory_database("").await;
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
    let cache = crate::fixtures::global_cache();
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

#[tideorm::model(table = "review_cached_values")]
struct CachedValue {
    #[tideorm(primary_key)]
    id: i64,
    data: Option<serde_json::Value>,
    score: Option<f64>,
}

#[tokio::test]
async fn cached_reads_return_what_an_uncached_read_does() {
    let db = memory_database(
        "CREATE TABLE review_cached_values(id INTEGER PRIMARY KEY, data JSON, score REAL); \
         INSERT INTO review_cached_values VALUES(1, 'null', 9e999), (2, '{\"a\":1}', 1.5)",
    )
    .await;
    let cache = crate::fixtures::global_cache();
    let read = |id: i64| {
        CachedValue::query_with(&db)
            .where_eq(CachedValue::ID, id)
            .cache(Duration::from_secs(60))
    };
    // JSON writes a JSON `null` in an `Option` and an infinite float as a
    // `null` that reads back as `None`, so that row is never cached.
    for _ in 0..2 {
        let row = read(1).first_or_fail().await.unwrap();
        assert_eq!(row.data, Some(serde_json::Value::Null));
        assert_eq!(row.score, Some(f64::INFINITY));
    }
    let hits = cache.stats().hits;
    for _ in 0..2 {
        let row = read(2).first_or_fail().await.unwrap();
        assert_eq!(row.data, Some(json!({"a": 1})));
        assert_eq!(row.score, Some(1.5));
    }
    assert_eq!(cache.stats().hits, hits + 1, "an exact row is still cached");
}

#[tokio::test]
async fn an_explicit_cache_key_keeps_terminals_and_pages_apart() {
    let db = memory_database(
        "CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN); \
         CREATE TABLE review_posts(id INTEGER PRIMARY KEY, user_id INTEGER, title TEXT); \
         INSERT INTO review_users VALUES(1, 'a', 1), (2, 'b', 1), (3, 'c', 1); \
         INSERT INTO review_posts VALUES(10, 2, 'by b')",
    )
    .await;
    let _cache = crate::fixtures::global_cache();
    let query = || {
        User::query_with(&db)
            .order_asc(User::ID)
            .cache_with_key("review-users", Duration::from_secs(60))
    };
    let ids = |users: Vec<User>| users.into_iter().map(|user| user.id).collect::<Vec<_>>();
    // The second round reads every entry the first one filled.
    for _ in 0..2 {
        assert_eq!(query().first().await.unwrap().map(|user| user.id), Some(1));
        assert_eq!(ids(query().get().await.unwrap()), [1, 2, 3]);
        assert_eq!(query().find(2).await.unwrap().map(|user| user.id), Some(2));
        assert_eq!(query().find(3).await.unwrap().map(|user| user.id), Some(3));
        assert_eq!(ids(query().paginate(1, 2).await.unwrap().items), [1, 2]);
        assert_eq!(ids(query().paginate(2, 2).await.unwrap().items), [3]);
        let first = query().with(User::POSTS).first().await.unwrap().unwrap();
        assert_eq!(first.id, 1);
        let found = query().with(User::POSTS).find(2).await.unwrap().unwrap();
        assert_eq!(
            (found.id, found.posts.get_cached().map(<[_]>::len)),
            (2, Some(1))
        );
    }
}
