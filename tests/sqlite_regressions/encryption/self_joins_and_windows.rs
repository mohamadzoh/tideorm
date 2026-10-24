//! Self-join aliases and window functions over encrypted columns.

use crate::fixtures::{encrypted_error, global_memory_database};
use tideorm::prelude::*;

#[tideorm::model(table = "behavior_secrets", encrypted = "secret")]
struct Secret {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

/// `behavior_secrets` holding `rows`, encrypted, as the global database.
async fn database(rows: &[(i64, &str)]) -> Database {
    tideorm::tokenization::TokenConfig::set_encryption_key("behavior-audit-key");
    let db =
        global_memory_database("CREATE TABLE behavior_secrets(id INTEGER PRIMARY KEY,secret TEXT)")
            .await;
    for &(id, secret) in rows {
        Secret::create(Secret {
            id,
            secret: secret.into(),
        })
        .await
        .unwrap();
    }
    db
}

/// The table joined to itself as `other`.
fn self_joined(db: &Database) -> QueryBuilder<Secret> {
    Secret::query_with(db).inner_join_as(
        "behavior_secrets",
        "other",
        "behavior_secrets.id",
        "other.id",
    )
}

/// Comparing ciphertext is refused through a self-join alias as through the
/// table's own name: in windows, distinct counts, MIN and MAX, DISTINCT and
/// GROUP BY.
#[tokio::test]
async fn self_join_aliases_cannot_compare_ciphertext() {
    let db = database(&[(1, "z"), (2, "a")]).await;
    encrypted_error(Secret::query_with(&db).min::<String>("secret").await);
    for column in ["secret", "behavior_secrets.secret", "other.secret"] {
        for window in [
            WindowFunction::new(WindowFunctionType::Min(column.into()), "val"),
            WindowFunction::new(WindowFunctionType::Max(column.into()), "val"),
            WindowFunction::new(WindowFunctionType::RowNumber, "val").partition_by(column),
            WindowFunction::new(WindowFunctionType::RowNumber, "val").order_by(column, Order::Asc),
        ] {
            encrypted_error(self_joined(&db).window(window).get_json().await);
        }
    }
    // A SELECT alias must not hide the encrypted physical window input.
    encrypted_error(
        Secret::query_with(&db)
            .select(vec!["id AS secret"])
            .distinct()
            .window(WindowFunction::new(
                WindowFunctionType::Min("secret".into()),
                "val",
            ))
            .get_json()
            .await,
    );
    encrypted_error(self_joined(&db).count_distinct("other.secret").await);
    encrypted_error(self_joined(&db).min::<String>("other.secret").await);
    encrypted_error(self_joined(&db).max::<String>("other.secret").await);
    let copied = || self_joined(&db).select(vec!["other.secret AS copied"]);
    encrypted_error(copied().distinct().get_json().await);
    encrypted_error(copied().group_by("other.secret").get_json().await);

    assert_eq!(
        self_joined(&db).count_distinct("other.id").await.unwrap(),
        2
    );
    let rows = Secret::query_with(&db)
        .window(WindowFunction::new(
            WindowFunctionType::Min("id".into()),
            "val",
        ))
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["val"], 1);
}

#[tokio::test]
async fn encrypted_self_join_alias_must_return_plaintext() {
    let db = database(&[(1, "hello")]).await;
    assert_eq!(
        Secret::query_with(&db)
            .select(vec!["secret AS copied"])
            .get_json()
            .await
            .unwrap()[0]["copied"],
        "hello"
    );
    let query = self_joined(&db).select(vec!["other.secret AS copied"]);
    let rows = query.clone().get_json().await.unwrap();
    assert_eq!(rows[0]["copied"], "hello");
    assert_eq!(
        query.clone().pluck::<String>("other.secret").await.unwrap(),
        vec!["hello"]
    );
    assert_eq!(
        query
            .clone()
            .value::<String>("other.secret")
            .await
            .unwrap()
            .as_deref(),
        Some("hello")
    );
    #[derive(serde::Deserialize)]
    struct Output {
        copied: String,
    }
    assert_eq!(query.get_as::<Output>().await.unwrap()[0].copied, "hello");
    let rows = self_joined(&db)
        .select(vec!["id"])
        .window(
            WindowFunction::new(
                WindowFunctionType::FirstValue("other.secret".into()),
                "copied",
            )
            .order_by("other.id", Order::Asc),
        )
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["copied"], "hello");
    // A similarly named column from a different joined table is ordinary text.
    db.exec_raw("CREATE TABLE behavior_plain(id INTEGER PRIMARY KEY,secret TEXT); INSERT INTO behavior_plain VALUES(1,'plain')").await.unwrap();
    let rows = Secret::query_with(&db)
        .inner_join_as("behavior_plain", "other", "behavior_secrets.id", "other.id")
        .select(vec!["other.secret AS copied"])
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["copied"], "plain");
}
