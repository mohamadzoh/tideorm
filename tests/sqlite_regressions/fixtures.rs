//! Databases and models shared by several regression modules.

use rust_decimal::Decimal;
use tideorm::prelude::*;

/// A one-connection in-memory SQLite database with `schema` applied: each
/// test owns its data, and one connection keeps every statement on it.
pub async fn memory_database(schema: &str) -> Database {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(1)
        .build()
        .await
        .unwrap();
    if !schema.is_empty() {
        db.exec_raw(schema).await.unwrap();
    }
    db
}

/// [`memory_database`], also installed as the global connection.
pub async fn global_memory_database(schema: &str) -> Database {
    let db = memory_database(schema).await;
    Database::set_global(db.clone()).unwrap();
    db
}

/// Assert that `result` was refused because it would compare ciphertext.
#[cfg(feature = "encrypted-fields")]
pub fn encrypted_error<T: std::fmt::Debug>(result: tideorm::Result<T>) {
    let error = result.unwrap_err().to_string();
    assert!(error.contains("encrypted"), "{error}");
}

#[tideorm::model(table = "review_users")]
pub struct User {
    #[tideorm(primary_key)]
    pub id: i64,
    pub name: String,
    pub active: bool,
    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    pub posts: HasMany<Post>,
}

#[tideorm::model(table = "review_posts")]
pub struct Post {
    #[tideorm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub title: String,
}

/// A database holding user 1, named `name`, and its post `post_id`. Tests
/// open two of these to tell which database a result came from.
pub async fn users_and_posts(name: &str, post_id: i64) -> Database {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_users(id INTEGER PRIMARY KEY, name TEXT, active BOOLEAN)")
        .await
        .unwrap();
    db.exec_raw("CREATE TABLE review_posts(id INTEGER PRIMARY KEY, user_id INTEGER, title TEXT)")
        .await
        .unwrap();
    db.exec_raw_with_params(
        "INSERT INTO review_users VALUES(1, ?, 1)",
        vec![name.into()],
    )
    .await
    .unwrap();
    db.exec_raw_with_params(
        "INSERT INTO review_posts VALUES(?, 1, ?)",
        vec![post_id.into(), name.into()],
    )
    .await
    .unwrap();
    db
}

#[tideorm::model(table = "review_chunk_projection")]
pub struct ChunkProjection {
    #[tideorm(primary_key, column = "stored_id")]
    pub id: i64,
    pub rank: i64,
}

pub async fn chunk_projection_database() -> Database {
    memory_database("CREATE TABLE review_chunk_projection(stored_id INTEGER PRIMARY KEY, rank INTEGER); INSERT INTO review_chunk_projection VALUES(7,80),(9,90)").await
}

#[tideorm::model(table = "review_projection_flags")]
pub struct ProjectionFlag {
    #[tideorm(primary_key)]
    pub id: i64,
    pub enabled: bool,
}

#[tideorm::model(table = "review_query_shapes")]
pub struct QueryShape {
    #[tideorm(primary_key)]
    pub id: i64,
    pub name: Option<String>,
}

pub async fn query_shapes_database() -> Database {
    memory_database("CREATE TABLE review_query_shapes(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO review_query_shapes VALUES(1,'A'),(2,'A'),(3,'B')").await
}

#[tideorm::model(table = "audit_rows")]
pub struct AuditRow {
    #[tideorm(primary_key)]
    pub id: i64,
    pub amount: i64,
    pub aggregate_1: i64,
}

#[tideorm::model(table = "audit_numbers")]
pub struct Number {
    #[tideorm(primary_key)]
    pub id: i64,
    pub value: i64,
}

#[tideorm::model(table = "behavior_items")]
pub struct Item {
    #[tideorm(primary_key)]
    pub id: i64,
    pub rank: i64,
    pub amount: f64,
}

pub async fn items_database() -> Database {
    memory_database("CREATE TABLE behavior_items(id INTEGER PRIMARY KEY,rank INTEGER,amount REAL); INSERT INTO behavior_items VALUES(1,10,1.5),(2,20,2.5),(3,30,3.5)").await
}

#[tideorm::model(table = "resume_decimals")]
pub struct MoneyRow {
    #[tideorm(primary_key)]
    pub id: i64,
    pub amount: Option<Decimal>,
    pub marker: i64,
}
