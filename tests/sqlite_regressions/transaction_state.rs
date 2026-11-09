//! Direct engine handles must retain SQLite's automatic rollback signal.
use crate::fixtures::memory_database;
use tideorm::orm::ConnectionTrait;

#[tokio::test]
async fn automatic_rollback_blocks_direct_handle_statements_and_commit() {
    let db = memory_database("CREATE TABLE tx_state(id INTEGER PRIMARY KEY)").await;
    let outcome = db
        .transaction(async move |tx| {
            let tx = tx.connection();
            tx.execute_unprepared("INSERT INTO tx_state VALUES (1)")
                .await
                .unwrap();
            assert!(
                tx.execute_unprepared("INSERT OR ROLLBACK INTO tx_state VALUES (1)")
                    .await
                    .is_err()
            );
            assert!(tx.has_server_transaction_ended().await.unwrap());
            assert!(
                tx.execute_unprepared("INSERT INTO tx_state VALUES (2)")
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("SQLite roll the whole transaction back")
            );
            Ok(())
        })
        .await;
    assert!(outcome.is_err());
    assert_eq!(
        db.query_raw_json("SELECT count(*) AS n FROM tx_state")
            .await
            .unwrap()[0]["n"],
        0
    );
}
