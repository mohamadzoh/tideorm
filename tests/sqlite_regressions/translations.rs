//! Translation payloads rendered by `to_json`.

use std::collections::HashMap;

use crate::fixtures::global_memory_database;
use tideorm::prelude::*;

#[tideorm::model(
    table = "translated_articles",
    translatable = "title,body",
    fallback_language = "en"
)]
struct Article {
    #[tideorm(primary_key)]
    id: i64,
    title: String,
    body: String,
    translations: Option<serde_json::Value>,
}

/// A payload another client wrote out of shape (no object, a field holding
/// `null` or a bare string) made `to_json` return `{}` for the whole model.
#[tokio::test]
async fn a_malformed_translations_payload_does_not_blank_to_json() {
    let db = global_memory_database(
        "CREATE TABLE translated_articles(id INTEGER PRIMARY KEY, title TEXT, body TEXT, translations TEXT)",
    )
    .await;
    db.exec_raw(
        r#"INSERT INTO translated_articles VALUES
            (1, 'Hello', 'Text', '[]'),
            (2, 'Hello', 'Text', '{"title": null, "body": {"fr": "Texte"}}'),
            (3, 'Hello', 'Text', '{"title": "Bonjour", "body": {"fr": "Texte"}}')"#,
    )
    .await
    .unwrap();

    let french = HashMap::from([("language".to_string(), "fr".to_string())]);
    let rendered: Vec<serde_json::Value> = Article::query()
        .order_asc(Article::columns.id)
        .get()
        .await
        .unwrap()
        .iter()
        .map(|article| article.to_json(Some(french.clone())))
        .collect();

    for (index, json) in rendered.iter().enumerate() {
        assert_eq!(json["id"], index as i64 + 1, "{json}");
        // The malformed field keeps the model's own value.
        assert_eq!(json["title"], "Hello", "{json}");
        assert!(json.get("translations").is_none(), "{json}");
    }
    assert_eq!(rendered[0]["body"], "Text");
    // A well-formed field beside a malformed one is still translated.
    assert_eq!(rendered[1]["body"], "Texte");
    assert_eq!(rendered[2]["body"], "Texte");
}
