//! Encrypted columns read through joined models and scalar subqueries.

use crate::fixtures::{encrypted_error, global_memory_database};
use tideorm::prelude::*;

#[tideorm::model(table = "projection_hosts")]
struct Host {
    #[tideorm(primary_key)]
    id: i64,
    secret_id: i64,
    secret: String,
}

#[tideorm::model(table = "projection_secrets", encrypted = "private_value")]
struct Secret {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "secret")]
    private_value: String,
}

async fn database() -> Database {
    tideorm::tokenization::TokenConfig::set_encryption_key("projection-regression-key");
    let db = global_memory_database("CREATE TABLE projection_hosts(id INTEGER PRIMARY KEY,secret_id INTEGER,secret TEXT); CREATE TABLE projection_secrets(id INTEGER PRIMARY KEY,secret TEXT); INSERT INTO projection_hosts VALUES(1,1,'plain'),(2,2,'plain'),(3,3,'plain')").await;
    for id in [1, 2] {
        Secret::create(Secret {
            id,
            private_value: "same plaintext".into(),
        })
        .await
        .unwrap();
    }
    db
}

fn joined(db: &Database, aliased: bool) -> (QueryBuilder<Host>, &'static str) {
    if aliased {
        (
            Host::query_with(db).inner_join_as(
                "projection_secrets",
                "s",
                "projection_hosts.secret_id",
                "s.id",
            ),
            "s.secret",
        )
    } else {
        (
            Host::query_with(db).inner_join(
                "projection_secrets",
                "projection_hosts.secret_id",
                "projection_secrets.id",
            ),
            "projection_secrets.secret",
        )
    }
}

#[tokio::test]
async fn foreign_join_projections_decrypt_renamed_fields_and_nulls() {
    let db = database().await;
    for aliased in [false, true] {
        let (query, column) = joined(&db, aliased);
        assert_eq!(
            query.clone().pluck::<String>(column).await.unwrap(),
            vec!["same plaintext"; 2]
        );
        assert_eq!(
            query
                .clone()
                .value::<String>(column)
                .await
                .unwrap()
                .as_deref(),
            Some("same plaintext")
        );
        let selected = query.clone().select(vec![&format!("{column} AS copied")]);
        assert_eq!(
            selected.clone().get_json().await.unwrap()[0]["copied"],
            "same plaintext"
        );
        #[derive(serde::Deserialize)]
        struct Output {
            copied: String,
        }
        assert_eq!(
            selected.get_as::<Output>().await.unwrap()[0].copied,
            "same plaintext"
        );
        let rows = query
            .select(vec!["projection_hosts.id"])
            .window(
                WindowFunction::new(WindowFunctionType::FirstValue(column.into()), "copied")
                    .order_by("projection_hosts.id", Order::Asc),
            )
            .get_json()
            .await
            .unwrap();
        assert_eq!(rows[0]["copied"], "same plaintext");
    }
    let rows = Host::query_with(&db)
        .left_join_as(
            "projection_secrets",
            "s",
            "projection_hosts.secret_id",
            "s.id",
        )
        .select(vec!["s.secret AS copied"])
        .order_asc("projection_hosts.id")
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[2]["copied"], serde_json::Value::Null);
    // The identically named plain column must not inherit encryption metadata.
    let (query, _) = joined(&db, true);
    assert_eq!(
        query
            .select(vec!["projection_hosts.secret"])
            .distinct()
            .pluck::<String>("projection_hosts.secret")
            .await
            .unwrap(),
        vec!["plain"]
    );
}

#[tokio::test]
async fn foreign_join_comparisons_reject_ciphertext() {
    let db = database().await;
    for aliased in [false, true] {
        let (query, column) = joined(&db, aliased);
        encrypted_error(query.clone().select(vec![column]).distinct().count().await);
        encrypted_error(
            query
                .clone()
                .select(vec![&format!("{column} AS copied")])
                .distinct()
                .get_json()
                .await,
        );
        encrypted_error(query.clone().count_distinct(column).await);
        encrypted_error(query.clone().min::<String>(column).await);
        encrypted_error(query.clone().max::<String>(column).await);
        encrypted_error(
            query
                .clone()
                .select_raw("COUNT(*) AS n")
                .group_by(column)
                .get_json()
                .await,
        );
        for window in [
            WindowFunction::new(WindowFunctionType::Min(column.into()), "val"),
            WindowFunction::new(WindowFunctionType::Max(column.into()), "val"),
            WindowFunction::new(WindowFunctionType::RowNumber, "val").partition_by(column),
            WindowFunction::new(WindowFunctionType::RowNumber, "val").order_by(column, Order::Asc),
        ] {
            encrypted_error(query.clone().window(window).get_json().await);
        }
    }
    let (query, _) = joined(&db, false);
    encrypted_error(query.min::<String>(Secret::PRIVATE_VALUE).await);
}

#[tokio::test]
async fn nested_scalar_subqueries_retain_encryption_scope() {
    let db = database().await;
    let direct = Host::query_with(&db).select_subquery(
        Secret::query_with(&db)
            .select(vec!["secret"])
            .where_eq("id", 1)
            .limit(1),
        "picked",
    );
    let (inner_joined, _) = joined(&db, true);
    let from_join = Host::query_with(&db)
        .select_subquery(inner_joined.select(vec!["s.secret"]).limit(1), "picked");
    let nested = Host::query_with(&db).select_subquery(from_join.clone().limit(1), "picked");
    for query in [direct, from_join, nested] {
        assert_eq!(
            query.clone().get_json().await.unwrap()[0]["picked"],
            "same plaintext"
        );
        #[derive(serde::Deserialize)]
        struct Output {
            picked: String,
        }
        let values = query.clone().get_as::<Output>().await.unwrap();
        assert_eq!(values.len(), 3);
        assert!(values.iter().all(|value| value.picked == "same plaintext"));
        encrypted_error(query.clone().distinct().count().await);
        encrypted_error(query.clone().distinct().min::<String>("picked").await);
        encrypted_error(query.clone().distinct().max::<String>("picked").await);
        encrypted_error(
            query
                .select_raw("COUNT(*) AS n")
                .group_by("picked")
                .get_json()
                .await,
        );
    }
    // A subquery's local alias may mean something different in the outer scope.
    let plain_subquery = Host::query_with(&db)
        .inner_join_as("projection_hosts", "s", "projection_hosts.id", "s.id")
        .select(vec!["s.secret"])
        .limit(1);
    let (outer, _) = joined(&db, true);
    let plain = outer
        .select_subquery(plain_subquery, "picked")
        .distinct()
        .get_json()
        .await
        .unwrap();
    assert_eq!(plain[0]["picked"], "plain");
}

#[tokio::test]
async fn typed_having_is_checked_after_joins_are_added() {
    let db = database().await;
    for condition in [
        Aggregate::min("s.secret").gt("a"),
        Aggregate::max("s.secret").gt("a"),
        Aggregate::count_distinct("s.secret").gt(1),
    ] {
        let query = Host::query_with(&db)
            .select_raw("COUNT(*) AS n")
            .having(condition)
            .inner_join_as(
                "projection_secrets",
                "s",
                "projection_hosts.secret_id",
                "s.id",
            );
        encrypted_error(query.get_json().await);
    }
}

// Registered only for the join lookups below: same table, two schemas, one encrypted.
#[tideorm::model(table = "projection_shared", schema = "secure", encrypted = "secret")]
struct ScopedSecret {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

#[tideorm::model(table = "projection_shared", schema = "ordinary")]
struct ScopedPlain {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

/// A joined table's schema picks the model whose encryption applies, to a
/// named column and to a wildcard alike.
#[tokio::test]
async fn joined_encryption_metadata_keeps_schema_boundaries() {
    let db = database().await;
    for selection in ["s.secret", "s.*"] {
        let distinct = |table: &str| {
            Host::query_with(&db)
                .inner_join_as(table, "s", "projection_hosts.secret_id", "s.id")
                .select(vec![selection])
                .distinct()
        };
        encrypted_error(distinct("secure.projection_shared").validate());
        distinct("ordinary.projection_shared").validate().unwrap();
    }
}
