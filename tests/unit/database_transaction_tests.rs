use super::{Error, LEAKED_TRANSACTION_MESSAGE, LeakedRollback, leaked_transaction_error};

#[test]
fn the_leak_report_names_the_rollback_outcome_and_keeps_the_closure_error() {
    for (rollback, closure_error, fragments) in [
        (
            LeakedRollback::RolledBack,
            None,
            vec!["rolled back explicitly"],
        ),
        // A rejected rollback is never swallowed.
        (
            LeakedRollback::Failed(Error::connection("connection closed")),
            None,
            vec!["rolling it back failed", "connection closed"],
        ),
        (
            LeakedRollback::RolledBack,
            Some(Error::validation("email", "is required")),
            vec!["rolled back explicitly", "email"],
        ),
    ] {
        let message = leaked_transaction_error(rollback, closure_error).to_string();
        assert!(message.contains(LEAKED_TRANSACTION_MESSAGE), "{message}");
        for fragment in fragments {
            assert!(message.contains(fragment), "{fragment:?}: {message}");
        }
    }
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn an_aborted_transaction_refuses_every_executor_entry_point() {
    use super::ConnectionRef;
    use crate::Database;
    use crate::orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE guarded (id INTEGER)")
        .await
        .unwrap();
    let independent = Database::connect("sqlite::memory:").await.unwrap();
    let result = db
        .transaction(async move |_| {
            // What a deadlock reported by the driver does; no live MySQL
            // server is needed to verify the gate.
            let connection = crate::database::__current_connection()?;
            let ConnectionRef::Transaction(transaction, ..) = &connection else {
                unreachable!("the transaction installs itself");
            };
            transaction.abort("a statement inside it deadlocked");
            let executor = connection.executor();
            let statement =
                || Statement::from_string(DbBackend::SQLite, "INSERT INTO guarded VALUES (1)");
            assert!(executor.execute_raw(statement()).await.is_err());
            assert!(
                executor
                    .execute_unprepared("INSERT INTO guarded VALUES (2)")
                    .await
                    .is_err()
            );
            assert!(executor.query_one_raw(statement()).await.is_err());
            assert!(executor.query_all_raw(statement()).await.is_err());
            assert!(executor.begin().await.is_err());
            // A separate pooled database remains usable in the failed scope.
            independent
                .exec_raw("CREATE TABLE independent (id INTEGER)")
                .await?;
            Ok(())
        })
        .await;
    assert!(result.unwrap_err().to_string().contains("deadlocked"));
    let rows = db
        .query_raw_json("SELECT COUNT(*) AS n FROM guarded")
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 0);
    // The next transaction has no stale failure flag.
    db.transaction(async |_| {
        Database::execute("INSERT INTO guarded VALUES (3)").await?;
        Ok(())
    })
    .await
    .unwrap();
    let rows = db
        .query_raw_json("SELECT COUNT(*) AS n FROM guarded")
        .await
        .unwrap();
    assert_eq!(rows[0]["n"], 1);
}
