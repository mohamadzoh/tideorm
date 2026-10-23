#![cfg(all(feature = "sqlite", feature = "runtime-tokio"))]

use super::Database;

#[tokio::test]
async fn metadata_answers_for_the_handle_it_was_called_on() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    // Stands in for a second handle whose connection is not the ambient
    // one: whatever it reports has to come from itself, not from the
    // transaction running around it.
    let other = Database::disconnected();

    db.transaction(move |_| {
        Box::pin(async move {
            assert!(
                other.__internal_backend().is_err(),
                "a handle must report its own backend, not the ambient transaction's"
            );
            assert!(
                other.ping().await.is_err(),
                "a handle must be pinged through its own connection"
            );

            // Execution still joins the ambient transaction: that is what
            // makes a nested `transaction` a SAVEPOINT instead of a second,
            // independent top-level one.
            let nested: crate::error::Result<()> =
                other.transaction(|_| Box::pin(async { Ok(()) })).await;
            nested.expect("a nested transaction should join the ambient one");

            Ok(())
        })
    })
    .await
    .expect("the outer transaction should commit");
}
