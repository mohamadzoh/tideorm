//! `UNION` and `UNION ALL` over encrypted projections.

use crate::fixtures::global_memory_database;
use tideorm::prelude::*;

#[tideorm::model(table = "union_plain")]
struct Plain {
    #[tideorm(primary_key)]
    id: i64,
    value: String,
}

#[tideorm::model(table = "union_first", encrypted = "secret,backup")]
struct First {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
    backup: String,
}

#[tideorm::model(table = "union_second", encrypted = "secret")]
struct Second {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

async fn database() -> Database {
    tideorm::tokenization::TokenConfig::set_encryption_key("union-regression-key");
    let db = global_memory_database("CREATE TABLE union_plain(id INTEGER PRIMARY KEY,value TEXT); CREATE TABLE union_first(id INTEGER PRIMARY KEY,secret TEXT,backup TEXT); CREATE TABLE union_second(id INTEGER PRIMARY KEY,secret TEXT); INSERT INTO union_plain VALUES(1,'plain')").await;
    for id in [1, 2] {
        First::create(First {
            id,
            secret: "same".into(),
            backup: "backup".into(),
        })
        .await
        .unwrap();
    }
    Second::create(Second {
        id: 1,
        secret: "second plaintext".into(),
    })
    .await
    .unwrap();
    db
}

fn encrypted_error<T: std::fmt::Debug>(result: tideorm::Result<T>, operation: &str) {
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains(operation) && error.contains("encrypted"),
        "{error}"
    );
}

#[tokio::test]
async fn union_rejects_ciphertext_deduplication_at_every_terminal() {
    let db = database().await;
    let left = First::query_with(&db)
        .select(vec!["secret"])
        .where_eq("id", 1);
    let right = First::query_with(&db)
        .select(vec!["secret"])
        .where_eq("id", 2);
    encrypted_error(left.clone().distinct().get_json().await, "DISTINCT");
    let query = left.union(right);
    encrypted_error(query.validate(), "UNION");
    encrypted_error(query.clone().get_json().await, "UNION");
    encrypted_error(query.clone().pluck::<String>("secret").await, "UNION");
    encrypted_error(query.clone().count().await, "UNION");
    encrypted_error(query.exists().await, "UNION");
    encrypted_error(
        First::query_with(&db)
            .union(First::query_with(&db))
            .get()
            .await,
        "UNION",
    );
    // Selecting only unencrypted columns from these models remains supported.
    assert_eq!(
        First::query_with(&db)
            .select(vec!["id"])
            .union(Second::query_with(&db).select(vec!["id"]))
            .count()
            .await
            .unwrap(),
        2,
    );
}

#[tokio::test]
async fn union_all_preserves_matching_scopes_and_uses_first_operand_aliases() {
    let db = database().await;
    let query = First::query_with(&db)
        .select(vec!["secret AS value"])
        .where_eq("id", 1)
        .union_all(
            First::query_with(&db)
                .select(vec!["secret AS other_name"])
                .where_eq("id", 2),
        );
    query.validate().unwrap();
    let rows = query.clone().get_json().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["value"] == "same"));
    assert_eq!(
        query.clone().pluck::<String>("value").await.unwrap(),
        vec!["same"; 2]
    );
    assert_eq!(query.clone().count().await.unwrap(), 2);
    #[derive(Debug, serde::Deserialize)]
    struct Output {
        value: String,
    }
    assert!(
        query
            .get_as::<Output>()
            .await
            .unwrap()
            .iter()
            .all(|row| row.value == "same")
    );

    let rows = First::query_with(&db)
        .where_eq("id", 1)
        .union_all(First::query_with(&db).where_eq("id", 2))
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.secret == "same" && row.backup == "backup")
    );
}

#[tokio::test]
async fn union_all_rejects_different_tables_columns_and_positions_before_execution() {
    let db = database().await;
    assert_eq!(
        Second::query_with(&db)
            .pluck::<String>("secret")
            .await
            .unwrap(),
        vec!["second plaintext"]
    );
    let plain = Plain::query_with(&db).select(vec!["value"]);
    let encrypted = Second::query_with(&db).select(vec!["secret AS value"]);
    let plain_first = plain.clone().union_all(encrypted.clone());
    encrypted_error(plain_first.validate(), "UNION ALL");
    encrypted_error(plain_first.get_json().await, "UNION ALL");
    let encrypted_first = encrypted.union_all(plain);
    encrypted_error(encrypted_first.validate(), "UNION ALL");
    encrypted_error(encrypted_first.get_json().await, "UNION ALL");
    let cross_table = First::query_with(&db)
        .select(vec!["secret"])
        .union_all(Second::query_with(&db).select(vec!["secret"]));
    encrypted_error(cross_table.validate(), "UNION ALL");
    encrypted_error(cross_table.get_json().await, "UNION ALL");
    encrypted_error(
        First::query_with(&db)
            .select(vec!["secret"])
            .union_all(First::query_with(&db).select(vec!["backup AS secret"]))
            .validate(),
        "UNION ALL",
    );
    encrypted_error(
        First::query_with(&db)
            .select(vec!["id", "secret"])
            .union_all(First::query_with(&db).select(vec!["secret", "id"]))
            .validate(),
        "UNION ALL",
    );
}

#[tokio::test]
async fn union_encryption_checks_survive_projection_changes_fragments_and_nesting() {
    let db = database().await;
    // Validate the final outer selection, not the one present at union_all().
    let changed = First::query_with(&db)
        .select(vec!["id"])
        .union_all(First::query_with(&db).select(vec!["secret"]))
        .select(vec!["secret"]);
    assert_eq!(changed.clone().count().await.unwrap(), 4);
    encrypted_error(changed.select(vec!["backup"]).validate(), "UNION ALL");

    let fragment = First::query_with(&db)
        .select(vec!["secret"])
        .union_all(Second::query_with(&db).select(vec!["secret"]))
        .consolidate();
    encrypted_error(
        First::query_with(&db).apply(&fragment).get_json().await,
        "UNION ALL",
    );
    let selected = || First::query_with(&db).select(vec!["secret"]);
    assert_eq!(
        selected()
            .union_all(selected().union_all(selected()))
            .count()
            .await
            .unwrap(),
        6
    );
    encrypted_error(
        selected()
            .union_all(selected())
            .union(selected())
            .validate(),
        "UNION",
    );
    encrypted_error(
        selected()
            .union_all(selected().union(selected()))
            .validate(),
        "UNION",
    );
}

#[tokio::test]
async fn union_unknown_layouts_fail_only_when_encrypted_outputs_are_involved() {
    let db = database().await;
    encrypted_error(
        First::query_with(&db)
            .select(vec!["*"])
            .union_all(First::query_with(&db).select(vec!["*"]))
            .validate(),
        "UNION ALL",
    );
    encrypted_error(
        First::query_with(&db)
            .select(vec!["secret"])
            .union_all_raw("SELECT secret FROM union_first")
            .validate(),
        "UNION ALL",
    );
    encrypted_error(
        First::query_with(&db)
            .select(vec!["secret"])
            .select_raw("1 AS n")
            .union_all(
                First::query_with(&db)
                    .select(vec!["secret"])
                    .select_raw("1 AS n"),
            )
            .validate(),
        "UNION ALL",
    );
    assert_eq!(
        Plain::query_with(&db)
            .select(vec!["*"])
            .union_all(Plain::query_with(&db).select(vec!["*"]))
            .count()
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        Plain::query_with(&db)
            .select(vec!["value"])
            .union_raw("SELECT value FROM union_plain")
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Plain::query_with(&db)
            .select(vec!["value"])
            .union_all_raw("SELECT value FROM union_plain")
            .count()
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn union_checks_include_joined_and_scalar_subquery_encryption_scopes() {
    let db = database().await;
    let joined = Plain::query_with(&db)
        .inner_join_as("union_first", "s", "union_plain.id", "s.id")
        .select(vec!["s.secret AS value"]);
    let same = First::query_with(&db).select(vec!["secret"]);
    let rows = joined
        .clone()
        .union_all(same.clone())
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row["value"] == "same"));
    encrypted_error(joined.clone().union(same.clone()).validate(), "UNION");
    encrypted_error(
        joined
            .union_all(Second::query_with(&db).select(vec!["secret"]))
            .validate(),
        "UNION ALL",
    );
    let scalar = Plain::query_with(&db).select_subquery(same.clone().limit(1), "value");
    let rows = scalar
        .clone()
        .union_all(same.clone())
        .get_json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row["value"] == "same"));
    encrypted_error(scalar.union(same).validate(), "UNION");
}
