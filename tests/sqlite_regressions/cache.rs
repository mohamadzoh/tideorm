//! Cached reads are invalidated by writes the ORM did not make itself: triggers,
//! pragmas, joined tables and failed statements.

use crate::fixtures::{Post, User, global_memory_database, memory_database, users_and_posts};
use std::time::Duration;
use tideorm::prelude::*;

#[cfg(feature = "dirty-tracking")]
#[tokio::test]
async fn cache_hits_do_not_replace_another_pools_baseline() {
    let tenant = users_and_posts("tenant", 10).await;
    let global = users_and_posts("global", 20).await;
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
async fn writable_pragmas_invalidate_cached_reads() {
    // A pragma changes one connection's settings; pin this test to one pool connection.
    let db = memory_database(
        "CREATE TABLE review_users(id INTEGER PRIMARY KEY,name TEXT,active BOOLEAN); \
         INSERT INTO review_users VALUES(1,'Alice',1),(2,'alice',1)",
    )
    .await;
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
