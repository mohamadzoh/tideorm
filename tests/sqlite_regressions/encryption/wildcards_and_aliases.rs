//! Joined wildcards and output aliases cannot hide an encrypted source column.

use crate::fixtures::{encrypted_error, global_memory_database};
use tideorm::prelude::*;

#[tideorm::model(table = "boundary_hosts")]
struct Host {
    #[tideorm(primary_key)]
    id: i64,
    secret_id: i64,
    secret: String,
}

#[tideorm::model(table = "boundary_secrets", encrypted = "secret")]
struct Secret {
    #[tideorm(primary_key)]
    id: i64,
    group_id: i64,
    secret: String,
    plain: String,
}

#[tideorm::model(table = "boundary_links")]
struct Link {
    #[tideorm(primary_key)]
    id: i64,
    secret_id: i64,
}

async fn database() -> Database {
    tideorm::tokenization::TokenConfig::set_encryption_key("boundary-audit-key");
    let db = global_memory_database("CREATE TABLE boundary_links(id INTEGER PRIMARY KEY,secret_id INTEGER); INSERT INTO boundary_links VALUES(1,1),(2,2); CREATE TABLE boundary_hosts(id INTEGER PRIMARY KEY,secret_id INTEGER,secret TEXT); CREATE TABLE boundary_secrets(id INTEGER PRIMARY KEY,group_id INTEGER,secret TEXT,plain TEXT); INSERT INTO boundary_hosts VALUES(1,1,'plain'),(2,2,'plain')").await;
    for id in [1, 2] {
        Secret::create(Secret {
            id,
            group_id: 1,
            secret: "same".into(),
            plain: "same".into(),
        })
        .await
        .unwrap();
    }
    db
}

#[tokio::test]
async fn joined_wildcards_decrypt_and_reject_distinct() {
    let db = database().await;
    for aliased in [false, true] {
        let (query, qualifier) = if aliased {
            (
                Host::query_with(&db).inner_join_as(
                    "boundary_secrets",
                    "s",
                    "boundary_hosts.secret_id",
                    "s.id",
                ),
                "s",
            )
        } else {
            (
                Host::query_with(&db).inner_join(
                    "boundary_secrets",
                    "boundary_hosts.secret_id",
                    "boundary_secrets.id",
                ),
                "boundary_secrets",
            )
        };
        let columns = ["id", "group_id", "secret", "plain"].map(|c| format!("{qualifier}.{c}"));
        let explicit = query
            .clone()
            .select(columns.iter().map(String::as_str).collect());
        let expected = explicit.clone().get_json().await.unwrap();
        encrypted_error(explicit.distinct().get_json().await);
        for wildcard in [
            query.clone().select(vec![&format!("{qualifier}.*")]),
            query.clone().select_raw(&format!("{qualifier}.*")),
        ] {
            assert_eq!(wildcard.clone().get_json().await.unwrap(), expected);
            assert_eq!(
                wildcard.clone().get_as::<Secret>().await.unwrap()[0].secret,
                "same"
            );
            encrypted_error(wildcard.clone().distinct().get_json().await);
            encrypted_error(wildcard.distinct().count().await);
        }
    }
}

#[tokio::test]
async fn self_join_and_plain_wildcards_keep_their_sources() {
    let db = database().await;
    let encrypted = Secret::query_with(&db)
        .inner_join_as("boundary_secrets", "s", "boundary_secrets.id", "s.id")
        .select(vec!["s.*"]);
    assert_eq!(
        encrypted.clone().get_json().await.unwrap()[0]["secret"],
        "same"
    );
    encrypted_error(encrypted.distinct().get_json().await);
    // An encrypted outer model must not mark a plain joined wildcard encrypted.
    let plain = Secret::query_with(&db)
        .inner_join_as("boundary_hosts", "h", "boundary_secrets.id", "h.secret_id")
        .select(vec!["h.*"])
        .distinct();
    let rows = plain.get_json().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["secret"], "plain");
    // Own-table wildcards still work without being counted twice in metadata.
    assert_eq!(
        Secret::query_with(&db)
            .select(vec!["boundary_secrets.*"])
            .get_json()
            .await
            .unwrap()[0]["secret"],
        "same"
    );
    encrypted_error(
        Secret::query_with(&db)
            .select(vec!["*"])
            .distinct()
            .get_json()
            .await,
    );
    // Wildcard expansion must not silently overwrite duplicate output names.
    let ambiguous = Host::query_with(&db)
        .inner_join_as("boundary_secrets", "s", "boundary_hosts.secret_id", "s.id")
        .select(vec!["*"])
        .get_json()
        .await
        .unwrap_err()
        .to_string();
    assert!(ambiguous.contains("twice"), "{ambiguous}");
}

#[tokio::test]
async fn distinct_aliases_cannot_hide_having_inputs() {
    let db = database().await;
    for distinct_first in [false, true] {
        for condition in [
            Aggregate::count_distinct("secret").gt(1),
            Aggregate::min("secret").gt("a"),
            Aggregate::max("secret").gt("a"),
        ] {
            let query = Secret::query_with(&db)
                .select(vec!["plain AS secret"])
                .select_raw("COUNT(*) AS n")
                .group_by("group_id");
            let query = if distinct_first {
                query.distinct().having(condition)
            } else {
                query.having(condition).distinct()
            };
            encrypted_error(query.get_json().await);
        }
    }
    let plain = Secret::query_with(&db)
        .select(vec!["plain AS secret"])
        .select_raw("COUNT(*) AS n")
        .distinct()
        .group_by("group_id")
        .having(Aggregate::count_distinct("plain").eq(1))
        .get_json()
        .await
        .unwrap();
    assert_eq!(plain[0]["n"], 2);
    // A terminal MIN of a derived DISTINCT result really does read its alias.
    assert_eq!(
        Secret::query_with(&db)
            .select(vec!["plain AS secret"])
            .distinct()
            .min::<String>("secret")
            .await
            .unwrap()
            .as_deref(),
        Some("same")
    );
}

#[tokio::test]
async fn distinct_aliases_cannot_hide_grouping_inputs() {
    let db = database().await;
    for column in ["secret", "boundary_secrets.secret"] {
        encrypted_error(
            Secret::query_with(&db)
                .select(vec!["plain AS secret"])
                .select_raw("COUNT(*) AS n")
                .distinct()
                .group_by(column)
                .get_json()
                .await,
        );
    }
    encrypted_error(
        Secret::query_with(&db)
            .select(vec!["secret AS copied"])
            .select_raw("COUNT(*) AS n")
            .group_by("copied")
            .get_json()
            .await,
    );
    let plain = Secret::query_with(&db)
        .select(vec!["plain AS secret"])
        .select_raw("COUNT(*) AS n")
        .distinct()
        .group_by("plain")
        .get_json()
        .await
        .unwrap();
    assert_eq!(plain[0]["n"], 2);
}

// Registered only for the join lookups below: same table, two schemas, one encrypted.
#[tideorm::model(table = "boundary_scoped", schema = "secure", encrypted = "secret")]
struct ScopedSecret {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

#[tideorm::model(table = "boundary_scoped", schema = "ordinary")]
struct ScopedPlain {
    #[tideorm(primary_key)]
    id: i64,
    secret: String,
}

#[tokio::test]
async fn wildcard_metadata_keeps_schema_boundaries() {
    let db = database().await;
    encrypted_error(
        Host::query_with(&db)
            .inner_join_as(
                "secure.boundary_scoped",
                "s",
                "boundary_hosts.secret_id",
                "s.id",
            )
            .select(vec!["s.*"])
            .distinct()
            .validate(),
    );
    Host::query_with(&db)
        .inner_join_as(
            "ordinary.boundary_scoped",
            "s",
            "boundary_hosts.secret_id",
            "s.id",
        )
        .select(vec!["s.*"])
        .distinct()
        .validate()
        .unwrap();
}

#[tokio::test]
async fn aliases_cannot_hide_unqualified_joined_inputs() {
    let db = database().await;
    let query = Link::query_with(&db)
        .inner_join_as("boundary_secrets", "s", "boundary_links.secret_id", "s.id")
        .select(vec!["boundary_links.id AS secret"])
        .select_raw("COUNT(*) AS n")
        .distinct();
    encrypted_error(query.clone().group_by("secret").get_json().await);
    encrypted_error(
        query
            .having(Aggregate::count_distinct("secret").gt(1))
            .get_json()
            .await,
    );
}

#[tideorm::model(table = "boundary_named", encrypted = "token_enc")]
struct Named {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "token_enc")]
    token: String,
    #[tideorm(column = "token")]
    note: String,
}

/// A batch update and an upsert name columns, so `token` is the plaintext
/// column of `note`, though it is the encrypted field's name too: the update
/// used to encrypt its value into `note`'s column, and the upsert refused it
/// as a conflict target.
#[tokio::test]
async fn a_column_named_like_an_encrypted_field_stays_plaintext() {
    tideorm::tokenization::TokenConfig::set_encryption_key("boundary-audit-key");
    let db = global_memory_database(
        "CREATE TABLE boundary_named(id INTEGER PRIMARY KEY, token_enc TEXT, token TEXT UNIQUE)",
    )
    .await;
    Named::create(Named {
        id: 1,
        token: "secret".into(),
        note: "plain".into(),
    })
    .await
    .unwrap();

    Named::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("token", "hello")
        .execute()
        .await
        .unwrap();
    let stored = db
        .__raw_json_with_params("SELECT token FROM boundary_named", Vec::new())
        .await
        .unwrap();
    assert_eq!(stored[0]["token"], "hello");
    let row = Named::find_with(1, &db).await.unwrap().unwrap();
    assert_eq!((row.token.as_str(), row.note.as_str()), ("secret", "hello"));

    let upserted = Named::insert_or_update(
        Named {
            id: 1,
            token: "rotated".into(),
            note: "hello".into(),
        },
        vec!["token"],
    )
    .await
    .unwrap();
    assert_eq!(upserted.token, "rotated");
}
