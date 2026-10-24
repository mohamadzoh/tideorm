//! Aliases and aggregates over encrypted columns.

use crate::fixtures::{Number, encrypted_error, global_memory_database, memory_database};
use tideorm::prelude::*;

#[tideorm::model(table = "audit_secrets", encrypted = "value")]
struct Secret {
    #[tideorm(primary_key)]
    id: i64,
    value: String,
}

#[tokio::test]
async fn encrypted_arithmetic_rejects_scalar_multi_having_and_window_operations() {
    let db=global_memory_database("CREATE TABLE audit_secrets(id INTEGER PRIMARY KEY,value TEXT); CREATE TABLE audit_numbers(id INTEGER PRIMARY KEY,value INTEGER); INSERT INTO audit_numbers VALUES(1,10),(2,20)").await;
    tideorm::tokenization::TokenConfig::set_encryption_key("audit-next-key");
    for (id, value) in [(1, "10"), (2, "20")] {
        Secret::create(Secret {
            id,
            value: value.into(),
        })
        .await
        .unwrap();
    }
    assert_eq!(
        Secret::query_with(&db)
            .order_asc("id")
            .pluck::<String>("value")
            .await
            .unwrap(),
        vec!["10", "20"]
    );
    encrypted_error(Secret::query_with(&db).sum::<f64>("value").await);
    encrypted_error(Secret::query_with(&db).avg::<f64>("value").await);
    encrypted_error(
        Secret::query_with(&db)
            .aggregates::<serde_json::Value>(&[Aggregate::sum("value"), Aggregate::avg("value")])
            .await,
    );
    for aggregate in [Aggregate::sum("value"), Aggregate::avg("value")] {
        encrypted_error(
            Secret::query_with(&db)
                .select_raw("COUNT(*) AS n")
                .having(aggregate.gt(0))
                .get_json()
                .await,
        );
    }
    for function in [
        WindowFunctionType::Sum("value".into()),
        WindowFunctionType::Avg("value".into()),
    ] {
        encrypted_error(
            Secret::query_with(&db)
                .select(vec!["id"])
                .window(WindowFunction::new(function, "amount"))
                .get_json()
                .await,
        );
    }
    let joined =
        Number::query_with(&db).inner_join_as("audit_secrets", "s", "audit_numbers.id", "s.id");
    encrypted_error(joined.sum::<f64>("s.value").await);
    let fragment = Number::query_with(&db)
        .select_raw("COUNT(*) AS n")
        .having(Aggregate::sum("s.value").gt(0))
        .consolidate();
    encrypted_error(
        Number::query_with(&db)
            .apply(&fragment)
            .inner_join_as("audit_secrets", "s", "audit_numbers.id", "s.id")
            .get_json()
            .await,
    );
    assert_eq!(
        Number::query_with(&db).sum::<i64>("value").await.unwrap(),
        30
    );
    assert_eq!(
        Number::query_with(&db).avg::<f64>("value").await.unwrap(),
        Some(15.0)
    );
}

#[tokio::test]
async fn distinct_rejects_randomized_encrypted_values() {
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

#[tokio::test]
async fn expression_alias_is_not_decrypted() {
    #[tideorm::model(table = "review_secrets", encrypted = "secret")]
    struct Secret {
        #[tideorm(primary_key)]
        id: i64,
        secret: String,
    }
    tideorm::tokenization::TokenConfig::set_encryption_key("audit-next-key");
    let db = memory_database(
        "CREATE TABLE review_secrets(id INTEGER PRIMARY KEY, secret TEXT); \
         INSERT INTO review_secrets VALUES(1, 'not a ciphertext')",
    )
    .await;
    let rows = Secret::query_with(&db)
        .select_raw("COUNT(*) AS secret")
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows[0]["secret"], 1);
    // Direct encrypted-column projections must still reject plaintext.
    let error = Secret::query_with(&db)
        .select(vec!["secret"])
        .get_json()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("plaintext"), "{error}");
}

#[tokio::test]
async fn aggregate_projection_alias_keeps_encryption_source() {
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
    assert!(
        query
            .clone()
            .min::<String>("renamed")
            .await
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    assert!(
        query
            .aggregates::<(String, String)>(&[Aggregate::min("renamed"), Aggregate::max("renamed")])
            .await
            .is_err()
    );
    assert!(
        Secret::query()
            .select(vec!["secret AS renamed"])
            .group_by("renamed")
            .get_json()
            .await
            .is_err()
    );
    assert!(Secret::query().group_by("secret").count().await.is_err());
    assert!(
        Secret::query()
            .having(Aggregate::min("secret").gt("a"))
            .get_json()
            .await
            .is_err()
    );
    assert!(Secret::query().max::<String>("secret").await.is_err());
    // Simple reads and alias decryption still work.
    assert_eq!(
        Secret::query()
            .select(vec!["secret AS renamed"])
            .get_json()
            .await
            .unwrap()[0]["renamed"],
        "plaintext"
    );
    Database::reset_global();
    TideConfig::reset();
}

/// Sorting by an encrypted column would order ciphertext: refused however the
/// column is named, while a raw `ORDER BY` stays the caller's own.
#[tokio::test]
async fn encrypted_columns_cannot_order_rows() {
    let db = memory_database("CREATE TABLE audit_secrets(id INTEGER PRIMARY KEY,value TEXT)").await;
    encrypted_error(Secret::query_with(&db).order_asc("value").get().await);
    encrypted_error(
        Secret::query_with(&db)
            .order_desc(Secret::VALUE)
            .first()
            .await,
    );
    encrypted_error(
        Secret::query_with(&db)
            .order_by("value", Order::Desc)
            .pluck::<i64>("id")
            .await,
    );
    encrypted_error(Secret::query_with(&db).sort("-value").get().await);
    assert!(Secret::query_with(&db).order_asc("id").get().await.is_ok());
    assert!(
        Secret::query_with(&db)
            .order_by_raw("id", Order::Asc)
            .get()
            .await
            .is_ok()
    );
}
