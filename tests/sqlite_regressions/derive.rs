//! The code the derive emits for a model, compiled under `deny(warnings)`:
//! the model's field names reach generated items, which must not warn for
//! them in the model's own crate.
#![deny(warnings)]

use crate::fixtures::global_memory_database;
use tideorm::prelude::*;

/// A legacy schema's names: a `_`-prefixed field and a camelCase one. The
/// struct allows its own names; the generated deserializer and columns
/// struct used to warn `non_snake_case` regardless.
#[tideorm::model(table = "legacy_accounts")]
#[allow(non_snake_case)]
struct LegacyAccount {
    #[tideorm(primary_key)]
    id: i64,
    #[tideorm(column = "private_note")]
    _private: String,
    displayName: String,
}

#[tokio::test]
async fn underscored_and_camel_case_fields_compile_quietly_and_round_trip() {
    let db = global_memory_database(
        "CREATE TABLE legacy_accounts(id INTEGER PRIMARY KEY, private_note TEXT, display_name TEXT)",
    )
    .await;
    LegacyAccount::create(LegacyAccount {
        id: 1,
        _private: "note".to_string(),
        displayName: "Ada".to_string(),
    })
    .await
    .unwrap();

    let loaded = LegacyAccount::query_with(&db)
        .where_eq(LegacyAccount::columns.displayName, "Ada".to_string())
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded._private, "note");

    let json = serde_json::to_value(&loaded).unwrap();
    assert_eq!(json["displayName"], "Ada");
    let back: LegacyAccount = serde_json::from_value(json).unwrap();
    assert_eq!(
        (back._private, back.displayName),
        ("note".into(), "Ada".into())
    );
}
