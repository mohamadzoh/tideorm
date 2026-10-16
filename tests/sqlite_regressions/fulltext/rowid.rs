//! Tables whose `rowid` is shadowed by a column.

use crate::fixtures::global_memory_database;
use tideorm::fulltext::{FullTextIndex, FullTextSearch};
use tideorm::prelude::*;

#[tideorm::model(table = "rowid_articles")]
struct Article {
    #[tideorm(primary_key)]
    id: i64,
    body: String,
}

#[tideorm::model(table = "rowid_articles", schema = "tenant")]
struct TenantArticle {
    #[tideorm(primary_key)]
    id: i64,
    body: String,
}

#[tideorm::model(table = "alias_articles")]
struct AliasArticle {
    #[tideorm(primary_key, column = "rowid")]
    id: i64,
    body: String,
}

async fn database() -> Database {
    global_memory_database("").await
}

fn shadow_error<T>(result: tideorm::Result<T>) {
    let error = match result {
        Ok(_) => panic!("shadowed rowid must be rejected"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("rowid is shadowed"), "{error}");
}

async fn install(db: &Database, index: &FullTextIndex) -> tideorm::Result<()> {
    for sql in index.to_sqlite_sql() {
        db.exec_raw(&sql).await?;
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_rowid_schemas_fail_before_any_index_is_created() {
    for definition in [
        "id INTEGER PRIMARY KEY,rowid INTEGER,body TEXT",
        "id INTEGER PRIMARY KEY,RoWiD TEXT,body TEXT",
        "rowid INTEGER PRIMARY KEY DESC,body TEXT",
        "rowid INTEGER,other INTEGER,body TEXT,PRIMARY KEY(rowid,other)",
        "id INTEGER PRIMARY KEY,rowid INTEGER GENERATED ALWAYS AS (7) VIRTUAL,body TEXT",
        "id INTEGER PRIMARY KEY,rowid INTEGER,_rowid_ INTEGER,oid INTEGER,body TEXT",
    ] {
        for schema in ["main", "tenant"] {
            let db = database().await;
            db.exec_raw("ATTACH DATABASE ':memory:' AS tenant")
                .await
                .unwrap();
            db.exec_raw(&format!(
                "CREATE TABLE {schema}.rowid_articles({definition})"
            ))
            .await
            .unwrap();
            let table = if schema == "main" {
                "rowid_articles".to_string()
            } else {
                format!("{schema}.rowid_articles")
            };
            let index = FullTextIndex::new("search", table, vec!["body".into()]);
            shadow_error(install(&db, &index).await);
            let objects = db
                .query_raw_json(&format!(
                    "SELECT name FROM {schema}.sqlite_schema WHERE name LIKE 'rowid_articles_%'"
                ))
                .await
                .unwrap();
            assert!(objects.is_empty(), "{objects:?}");
        }
    }
}

#[tokio::test]
async fn legacy_indexes_and_unmodeled_shadow_columns_cannot_return_wrong_matches() {
    for schema in ["main", "tenant"] {
        let db = database().await;
        db.exec_raw("ATTACH DATABASE ':memory:' AS tenant")
            .await
            .unwrap();
        db.exec_raw(&format!(
            "CREATE TABLE {schema}.rowid_articles(id INTEGER PRIMARY KEY,rowid INTEGER,body TEXT)"
        ))
        .await
        .unwrap();
        let index = FullTextIndex::new(
            "search",
            format!("{schema}.rowid_articles"),
            vec!["body".into()],
        );
        // Recreate the pre-fix installation, whose CREATE and trigger definitions
        // used the user column as FTS identity. Skip both guarded statements.
        let statements = index.to_sqlite_sql();
        for sql in &statements[1..6] {
            db.exec_raw(sql).await.unwrap();
        }
        db.exec_raw(&format!(
            "INSERT INTO {schema}.rowid_articles VALUES(1,7,'alpha'),(2,7,'beta')"
        ))
        .await
        .unwrap();
        for term in ["alpha", "beta", "missing", ""] {
            if schema == "main" {
                shadow_error(Article::search(&["body"], term).get().await);
                shadow_error(Article::search(&["body"], term).first().await);
                shadow_error(Article::search(&["body"], term).get_ranked().await);
                shadow_error(Article::search(&["body"], term).count().await);
            } else {
                shadow_error(TenantArticle::search(&["body"], term).get().await);
                shadow_error(TenantArticle::search(&["body"], term).get_ranked().await);
                shadow_error(TenantArticle::search(&["body"], term).count().await);
            }
        }
        shadow_error(db.exec_raw(&index.sqlite_rebuild_sql()).await);
        assert_eq!(
            db.query_raw_json(&format!("SELECT id FROM {schema}.rowid_articles"))
                .await
                .unwrap()
                .len(),
            2
        );
    }
}

#[tokio::test]
async fn real_rowid_primary_key_alias_supports_search_and_maintenance() {
    let db = database().await;
    db.exec_raw("CREATE TABLE alias_articles(rowid INTEGER PRIMARY KEY,body TEXT)")
        .await
        .unwrap();
    let index = FullTextIndex::new("search", "alias_articles", vec!["body".into()]);
    install(&db, &index).await.unwrap();
    AliasArticle {
        id: 1,
        body: "alpha".into(),
    }
    .save()
    .await
    .unwrap();
    AliasArticle {
        id: 2,
        body: "beta".into(),
    }
    .save()
    .await
    .unwrap();
    let rows = AliasArticle::search(&["body"], "beta")
        .get_ranked()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record.id, 2);
    AliasArticle {
        id: 1,
        body: "beta".into(),
    }
    .save()
    .await
    .unwrap();
    assert_eq!(
        AliasArticle::search(&["body"], "beta")
            .count()
            .await
            .unwrap(),
        2
    );
    AliasArticle::find(2)
        .await
        .unwrap()
        .unwrap()
        .delete()
        .await
        .unwrap();
    install(&db, &index).await.unwrap();
    db.exec_raw(&index.sqlite_rebuild_sql()).await.unwrap();
    let rows = AliasArticle::search(&["body"], "beta").get().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 1);
}

#[tokio::test]
async fn other_rowid_aliases_do_not_interfere_with_hidden_rowid() {
    let db = database().await;
    db.exec_raw("CREATE TABLE rowid_articles(id INTEGER PRIMARY KEY,_rowid_ INTEGER,oid INTEGER,body TEXT); INSERT INTO rowid_articles VALUES(1,7,7,'alpha'),(2,7,7,'beta')").await.unwrap();
    let index = FullTextIndex::new("search", "rowid_articles", vec!["body".into()]);
    install(&db, &index).await.unwrap();
    let rows = Article::search(&["body"], "beta").get().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, 2);
    assert_eq!(Article::search(&["body"], "beta").count().await.unwrap(), 1);
}
