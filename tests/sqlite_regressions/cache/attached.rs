//! Writes through an attached SQLite database drop the attached file's
//! cached reads, whether the attachment is made on a pool or in a transaction.

use crate::fixtures::{User, file_database};
use std::time::Duration;
use tideorm::prelude::*;

use super::OLD_USER;

#[tokio::test]
async fn attached_databases_keep_conservative_invalidation() {
    for attach in [
        "ATTACH DATABASE ? AS other",
        "\u{feff}ATTACH DATABASE ? AS other",
        "/* outer /* inner */ ATTACH DATABASE ? AS other",
    ] {
        check_attachment_invalidation(attach).await;
    }
}

async fn check_attachment_invalidation(attach: &str) {
    let (file_db, file) = file_database("attached-cache-scope").await;
    file_db.exec_raw(OLD_USER).await.unwrap();
    // SQLITE_OPEN_MEMORY also affects ATTACH unless its URI overrides the
    // mode. Use a file-backed, single-connection pool for this file attachment.
    let attaching_file = crate::fixtures::TempFile::new("attaching-cache-scope", "sqlite");
    let attaching = crate::fixtures::single_connection(&format!(
        "sqlite://{}?mode=rwc",
        attaching_file.display()
    ))
    .await;
    let cache = crate::fixtures::global_cache();
    let query = User::query_with(&file_db).cache(Duration::from_secs(60));
    query.clone().get().await.unwrap();
    attaching
        .exec_raw_with_params(attach, vec![file.to_string_lossy().to_string().into()])
        .await
        .unwrap();
    query.clone().get().await.unwrap();
    attaching
        .exec_raw("UPDATE other.review_users SET name = 'new' WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(query.get().await.unwrap()[0].name, "new");
    assert_eq!(cache.stats().entries, 1);
    crate::fixtures::close(&attaching).await;
    crate::fixtures::close(&file_db).await;
}

/// SQLite attaches inside a transaction too, so a write through such an
/// attachment drops the attached file's cached reads when it commits.
#[tokio::test]
async fn databases_attached_inside_a_transaction_keep_conservative_invalidation() {
    let (file_db, file) = file_database("attached-in-transaction").await;
    file_db.exec_raw(OLD_USER).await.unwrap();
    let attaching_file = crate::fixtures::TempFile::new("attaching-in-transaction", "sqlite");
    let attaching = crate::fixtures::single_connection(&format!(
        "sqlite://{}?mode=rwc",
        attaching_file.display()
    ))
    .await;
    let _cache = crate::fixtures::global_cache();
    let query = User::query_with(&file_db).cache(Duration::from_secs(60));
    assert_eq!(query.clone().get().await.unwrap()[0].name, "old");
    let (handle, path) = (attaching.clone(), file.to_string_lossy().to_string());
    attaching
        .transaction(async move |_tx| {
            handle
                .exec_raw_with_params("ATTACH DATABASE ? AS other", vec![path.into()])
                .await?;
            handle
                .exec_raw("UPDATE other.review_users SET name = 'new' WHERE id = 1")
                .await?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(query.get().await.unwrap()[0].name, "new");
    crate::fixtures::close(&attaching).await;
    crate::fixtures::close(&file_db).await;
}
