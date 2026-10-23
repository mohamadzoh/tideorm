//! Full-text searches over encrypted columns.

use crate::fixtures::{encrypted_error, global_memory_database};
use tideorm::fulltext::{FullTextIndex, FullTextSearch};
use tideorm::prelude::*;

#[tideorm::model(table = "encrypted_notes", encrypted = "secret")]
struct Note {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
    #[tideorm(column = "body")]
    secret: String,
}

/// The index held a fresh ciphertext per row, so a search over the column
/// silently matched nothing instead of failing.
#[tokio::test]
async fn searching_an_encrypted_column_is_refused() {
    tideorm::tokenization::TokenConfig::set_encryption_key("fulltext-regression-key");
    let db = global_memory_database(
        "CREATE TABLE encrypted_notes(id INTEGER PRIMARY KEY, title TEXT, body TEXT)",
    )
    .await;
    let index = FullTextIndex::new(
        "encrypted_notes_search",
        "encrypted_notes",
        vec!["title".into(), "body".into()],
    );
    for sql in index.to_sqlite_sql() {
        db.exec_raw(&sql).await.unwrap();
    }
    Note::create(Note {
        id: 1,
        title: "alpha".into(),
        secret: "alpha".into(),
    })
    .await
    .unwrap();

    // By field name and by column name, in every kind of search.
    for column in ["secret", "body"] {
        encrypted_error(Note::search(&[column], "alpha").get().await);
        encrypted_error(Note::search(&["title", column], "alpha").get_ranked().await);
        encrypted_error(Note::search(&[column], "alpha").count().await);
        encrypted_error(Note::search(&[column], "alpha").first().await);
    }
    assert_eq!(Note::search(&["title"], "alpha").count().await.unwrap(), 1);
}
