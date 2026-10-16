//! Self-join aliases and window functions over encrypted columns.

use crate::fixtures::items_database;
use tideorm::prelude::*;

#[cfg(feature = "encrypted-fields")]
#[tideorm::model(table = "behavior_secrets", encrypted = "secret")]
struct Secret {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn encrypted_windows_reject_ciphertext_comparisons() {
    let db = items_database().await;
    tideorm::tokenization::TokenConfig::set_encryption_key("behavior-audit-key");
    db.exec_raw("CREATE TABLE behavior_secrets(id INTEGER PRIMARY KEY,secret TEXT)")
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    for (id, secret) in [(1, "z"), (2, "a")] {
        Secret::create(Secret {
            id,
            secret: secret.into(),
        })
        .await
        .unwrap();
    }
    assert!(
        Secret::query_with(&db)
            .min::<String>("secret")
            .await
            .is_err()
    );
    for column in ["secret", "behavior_secrets.secret", "other.secret"] {
        for window in [
            WindowFunction::new(WindowFunctionType::Min(column.into()), "val"),
            WindowFunction::new(WindowFunctionType::Max(column.into()), "val"),
            WindowFunction::new(WindowFunctionType::RowNumber, "val").partition_by(column),
            WindowFunction::new(WindowFunctionType::RowNumber, "val").order_by(column, Order::Asc),
        ] {
            let error = Secret::query_with(&db)
                .inner_join_as(
                    "behavior_secrets",
                    "other",
                    "behavior_secrets.id",
                    "other.id",
                )
                .window(window)
                .get_json()
                .await
                .unwrap_err();
            assert!(error.to_string().contains("encrypted"), "{error}");
        }
    }
    // A SELECT alias must not hide the encrypted physical window input.
    assert!(
        Secret::query_with(&db)
            .select(vec!["id AS secret"])
            .distinct()
            .window(WindowFunction::new(
                WindowFunctionType::Min("secret".into()),
                "val"
            ))
            .get_json()
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
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

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn encrypted_self_join_alias_must_return_plaintext() {
    let db = items_database().await;
    tideorm::tokenization::TokenConfig::set_encryption_key("behavior-audit-key");
    db.exec_raw("CREATE TABLE behavior_secrets(id INTEGER PRIMARY KEY,secret TEXT)")
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    Secret::create(Secret {
        id: 1,
        secret: "hello".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        Secret::query_with(&db)
            .select(vec!["secret AS copied"])
            .get_json()
            .await
            .unwrap()[0]["copied"],
        "hello"
    );
    let query = Secret::query_with(&db)
        .inner_join_as(
            "behavior_secrets",
            "other",
            "behavior_secrets.id",
            "other.id",
        )
        .select(vec!["other.secret AS copied"]);
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
    let rows = Secret::query_with(&db)
        .inner_join_as(
            "behavior_secrets",
            "other",
            "behavior_secrets.id",
            "other.id",
        )
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

#[cfg(feature = "encrypted-fields")]
#[tokio::test]
async fn encrypted_self_join_alias_must_not_bypass_distinct_guard() {
    let db = items_database().await;
    tideorm::tokenization::TokenConfig::set_encryption_key("behavior-audit-key");
    db.exec_raw("CREATE TABLE behavior_secrets(id INTEGER PRIMARY KEY,secret TEXT)")
        .await
        .unwrap();
    Database::set_global(db.clone()).unwrap();
    for id in 1..=2 {
        Secret::create(Secret {
            id,
            secret: "same".into(),
        })
        .await
        .unwrap();
    }
    assert!(
        Secret::query_with(&db)
            .count_distinct("secret")
            .await
            .is_err()
    );
    let query = Secret::query_with(&db).inner_join_as(
        "behavior_secrets",
        "other",
        "behavior_secrets.id",
        "other.id",
    );
    assert!(
        query
            .clone()
            .count_distinct("other.secret")
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    assert!(
        query
            .clone()
            .min::<String>("other.secret")
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    assert!(
        query
            .clone()
            .max::<String>("other.secret")
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    assert!(
        query
            .clone()
            .select(vec!["other.secret AS copied"])
            .distinct()
            .get_json()
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    assert!(
        query
            .clone()
            .select(vec!["other.secret AS copied"])
            .group_by("other.secret")
            .get_json()
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    assert_eq!(query.count_distinct("other.id").await.unwrap(), 2);
}
