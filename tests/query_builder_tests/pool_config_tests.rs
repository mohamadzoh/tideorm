use std::time::Duration;
use tideorm::Database;

#[test]
fn test_database_builder_creation() {
    let builder = Database::builder()
        .url("sqlite::memory:")
        .max_connections(20)
        .min_connections(5)
        .connect_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .max_lifetime(Duration::from_secs(3600));

    drop(builder);
}

#[tokio::test]
#[ignore]
async fn test_pool_connection() {
    let db = Database::builder()
        .url("sqlite::memory:")
        .max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .await;

    assert!(db.is_ok(), "Should connect with pool settings");

    let db = db.unwrap();
    let ping = db.ping().await;
    assert!(ping.is_ok());
}
