use crate::fixtures::memory_database;
use tideorm::prelude::*;

#[tideorm::model(table = "chunked_batch_rows")]
struct BatchRow {
    #[tideorm(primary_key)]
    id: i64,
    label: String,
}

#[tokio::test]
async fn failure_in_a_later_chunk_rolls_back_the_whole_batch() {
    let db = memory_database(
        "CREATE TABLE chunked_batch_rows(id INTEGER PRIMARY KEY, label TEXT NOT NULL);
         CREATE TRIGGER reject_last BEFORE INSERT ON chunked_batch_rows
         WHEN NEW.label = 'reject' BEGIN SELECT RAISE(FAIL, 'rejected last row'); END;",
    )
    .await;
    // Two columns a row: one row past half the limit needs a second statement.
    let limit = tideorm::orm::DbBackend::SQLite
        .max_bind_parameters()
        .unwrap();
    let count = limit / 2 + 1;
    let mut models = (0..count)
        .map(|i| BatchRow {
            id: i as i64 + 1,
            label: format!("row {i}"),
        })
        .collect::<Vec<_>>();
    models.last_mut().unwrap().label = "reject".into();
    db.scope(async {
        assert!(BatchRow::insert_all(models).await.is_err());
        assert_eq!(BatchRow::count().await?, 0);
        Ok(())
    })
    .await
    .unwrap();
}
