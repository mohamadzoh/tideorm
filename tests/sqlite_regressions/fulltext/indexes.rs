//! FTS5 index installation, trigger-name conflicts, rank aliases and attached schemas.

use super::install;
use crate::fixtures::{global_memory_database, memory_database};
use tideorm::fulltext::{FullTextIndex, FullTextSearch, FullTextSearchBuilder};
use tideorm::prelude::*;

#[tideorm::model(table = "audit_articles")]
struct Article {
    #[tideorm(primary_key)]
    id: i64,
    body: String,
    #[tideorm(column = "_fts_rank")]
    stored_rank: f64,
}

#[tideorm::model(table = "audit_articles", schema = "tenant")]
struct ScopedArticle {
    #[tideorm(primary_key)]
    id: i64,
    body: String,
}

async fn index(db: &Database) -> tideorm::Result<()> {
    let index = FullTextIndex::new(
        "audit_articles_search",
        "audit_articles",
        vec!["body".into()],
    );
    install(db, &index).await
}

#[tokio::test]
async fn fulltext_rank_does_not_shadow_model_column() {
    let db = global_memory_database(
        "CREATE TABLE audit_articles(id INTEGER PRIMARY KEY,body TEXT,_fts_rank REAL); \
         INSERT INTO audit_articles VALUES(1,'alpha alpha',123.5),(2,'alpha beta',456.5)",
    )
    .await;
    index(&db).await.unwrap();
    let ranked = Article::search(&["body"], "alpha")
        .get_ranked()
        .await
        .unwrap();
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].record.stored_rank, 123.5);
    assert_eq!(ranked[1].record.stored_rank, 456.5);
    assert!(ranked[0].rank > ranked[1].rank);
    assert!(ranked.iter().all(|r| r.rank > 0.0 && r.rank < 1.0));
    assert!(
        Article::search(&["body"], "alpha")
            .min_rank(1.0)
            .get_ranked()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn fulltext_conflicting_triggers_fail_without_replacing_user_definitions() {
    for (suffix, event) in [("ai", "INSERT"), ("ad", "DELETE"), ("au", "UPDATE")] {
        let db = global_memory_database(
            "CREATE TABLE audit_articles(id INTEGER PRIMARY KEY,body TEXT,_fts_rank REAL); \
             CREATE TABLE audit_log(article_id INTEGER)",
        )
        .await;
        let trigger = format!(
            "CREATE TRIGGER audit_articles_{suffix} AFTER {event} ON audit_articles \
             BEGIN INSERT INTO audit_log VALUES(100); END"
        );
        db.exec_raw(&trigger).await.unwrap();
        let error = index(&db).await.unwrap_err().to_string();
        assert!(error.contains("full-text trigger name conflict"), "{error}");
        // The caller's trigger remains installed and still executes.
        db.exec_raw(
            "INSERT INTO audit_articles VALUES(1,'alpha',123.5); \
             UPDATE audit_articles SET body='beta'; DELETE FROM audit_articles",
        )
        .await
        .unwrap();
        // Renaming is a caller decision; dropping this fixture trigger lets installation succeed.
        db.exec_raw(&format!("DROP TRIGGER audit_articles_{suffix}"))
            .await
            .unwrap();
        index(&db).await.unwrap();
        index(&db).await.unwrap();
        db.exec_raw("INSERT INTO audit_articles VALUES(2,'alpha',456.5)")
            .await
            .unwrap();
        assert_eq!(
            Article::search(&["body"], "alpha").count().await.unwrap(),
            1
        );
        db.exec_raw("UPDATE audit_articles SET body='beta' WHERE id=2")
            .await
            .unwrap();
        assert_eq!(
            Article::search(&["body"], "alpha").count().await.unwrap(),
            0
        );
        assert_eq!(Article::search(&["body"], "beta").count().await.unwrap(), 1);
        db.exec_raw("DELETE FROM audit_articles WHERE id=2")
            .await
            .unwrap();
        assert_eq!(Article::search(&["body"], "beta").count().await.unwrap(), 0);
    }
}

#[tokio::test]
async fn fulltext_trigger_checks_use_the_index_schema_and_are_idempotent() {
    let db = global_memory_database(
        "CREATE TABLE audit_articles(id INTEGER PRIMARY KEY,body TEXT); \
         CREATE TRIGGER audit_articles_ai AFTER INSERT ON audit_articles BEGIN SELECT 1; END; \
         ATTACH ':memory:' AS tenant; \
         CREATE TABLE tenant.audit_articles(id INTEGER PRIMARY KEY,body TEXT)",
    )
    .await;
    let index = FullTextIndex::new(
        "tenant_search",
        "tenant.audit_articles",
        vec!["body".into()],
    );
    for _ in 0..2 {
        install(&db, &index).await.unwrap();
    }
    db.exec_raw("INSERT INTO tenant.audit_articles VALUES(1,'alpha')")
        .await
        .unwrap();
    assert_eq!(
        ScopedArticle::search(&["body"], "alpha")
            .count()
            .await
            .unwrap(),
        1
    );
    db.exec_raw("UPDATE tenant.audit_articles SET body='beta'")
        .await
        .unwrap();
    assert_eq!(
        ScopedArticle::search(&["body"], "beta")
            .count()
            .await
            .unwrap(),
        1
    );
    db.exec_raw("DELETE FROM tenant.audit_articles")
        .await
        .unwrap();
    assert_eq!(
        ScopedArticle::search(&["body"], "beta")
            .count()
            .await
            .unwrap(),
        0
    );
}

#[tideorm::model(table = "query_behavior_articles", schema = "tenant")]
struct TenantArticle {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
}

#[tideorm::model(table = "query_behavior_articles")]
struct MainArticle {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
}

#[tokio::test]
async fn searches_use_the_models_index_when_schema_names_collide() {
    let db = memory_database(
        "ATTACH DATABASE ':memory:' AS tenant; \
         CREATE TABLE main.query_behavior_articles(id INTEGER PRIMARY KEY,title TEXT); \
         CREATE TABLE tenant.query_behavior_articles(id INTEGER PRIMARY KEY,title TEXT); \
         INSERT INTO main.query_behavior_articles VALUES(1,'apple'),(2,'pear'),(3,'apple'); \
         INSERT INTO tenant.query_behavior_articles VALUES(1,'pear'),(2,'apple'),(3,'pear')",
    )
    .await;
    for table in [
        "main.query_behavior_articles",
        "tenant.query_behavior_articles",
    ] {
        install(
            &db,
            &FullTextIndex::new("search", table, vec!["title".into()]),
        )
        .await
        .unwrap();
    }
    db.transaction(|_| {
        Box::pin(async {
            let search = || FullTextSearchBuilder::<TenantArticle>::new(&["title"], "apple");
            let result = search().get().await?;
            assert_eq!(result.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2]);
            assert_eq!(search().first().await?.unwrap().id, 2);
            let ranked = search().min_rank(0.0).get_ranked().await?;
            assert_eq!(
                ranked.iter().map(|r| r.record.id).collect::<Vec<_>>(),
                vec![2]
            );
            assert!(ranked[0].rank > 0.0);
            assert_eq!(search().count().await?, 1);
            let mut main_ids: Vec<_> =
                FullTextSearchBuilder::<MainArticle>::new(&["title"], "apple")
                    .get()
                    .await?
                    .iter()
                    .map(|r| r.id)
                    .collect();
            main_ids.sort_unstable();
            assert_eq!(main_ids, vec![1, 3]);
            assert_eq!(
                FullTextSearchBuilder::<MainArticle>::new(&["title"], "apple")
                    .count()
                    .await?,
                2
            );
            // A query with no search terms must not invoke bm25 without MATCH.
            assert!(
                FullTextSearchBuilder::<TenantArticle>::new(&["title"], "")
                    .get_ranked()
                    .await?
                    .is_empty()
            );
            Ok(())
        })
    })
    .await
    .unwrap();
}
