use super::*;

#[test]
fn only_failures_that_left_nothing_committed_are_retried() {
    use crate::error::DbFailure;

    let failing = |kind| Error::query("simulated").with_db_failure(Some(DbFailure::new(kind)));
    for kind in [
        DbFailureKind::Deadlock,
        DbFailureKind::SerializationFailure,
        DbFailureKind::LockNotAvailable,
    ] {
        assert!(worth_retrying(&failing(kind)), "{kind}");
    }
    for kind in [
        DbFailureKind::ConnectionClosed,
        DbFailureKind::StatementTimeout,
        DbFailureKind::UniqueViolation,
        DbFailureKind::Unclassified,
    ] {
        assert!(!worth_retrying(&failing(kind)), "{kind}");
    }
    assert!(!worth_retrying(&Error::validation("email", "is required")));
}

#[test]
fn a_wait_is_shortened_by_at_most_half() {
    for _ in 0..100 {
        let wait = jittered(Duration::from_millis(100));
        assert!(wait > Duration::from_millis(50) && wait <= Duration::from_millis(100));
    }
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
mod sqlite {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::error::DbFailure;

    async fn database() -> Database {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.exec_raw("CREATE TABLE runs (attempt INTEGER NOT NULL)")
            .await
            .unwrap();
        db
    }

    async fn written(db: &Database) -> Vec<i64> {
        db.query_raw_json("SELECT attempt FROM runs ORDER BY attempt")
            .await
            .unwrap()
            .iter()
            .map(|row| row["attempt"].as_i64().unwrap())
            .collect()
    }

    /// Runs `attempts` times at most, writing its attempt number and failing
    /// with `kind` until attempt `succeeds_on`.
    async fn run(
        db: &Database,
        attempts: u32,
        kind: DbFailureKind,
        succeeds_on: u32,
    ) -> (Result<u32>, u32) {
        let runs = AtomicU32::new(0);
        let outcome = db
            .transaction_retrying(TransactionOptions::default(), attempts, async |_| {
                let attempt = runs.fetch_add(1, Ordering::SeqCst) + 1;
                db.exec_raw(&format!("INSERT INTO runs VALUES ({attempt})"))
                    .await?;
                if attempt < succeeds_on {
                    return Err(
                        Error::query("simulated").with_db_failure(Some(DbFailure::new(kind)))
                    );
                }
                Ok(attempt)
            })
            .await;
        (outcome, runs.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn a_retryable_failure_runs_it_again_in_a_new_transaction() {
        let db = database().await;
        let (outcome, runs) = run(&db, 3, DbFailureKind::SerializationFailure, 3).await;
        assert_eq!((outcome.unwrap(), runs), (3, 3));
        // The failed attempts were rolled back.
        assert_eq!(written(&db).await, [3]);
    }

    #[tokio::test]
    async fn the_last_failure_is_returned_once_attempts_run_out() {
        let db = database().await;
        let (outcome, runs) = run(&db, 2, DbFailureKind::Deadlock, 5).await;
        assert_eq!(outcome.unwrap_err().failure_kind(), DbFailureKind::Deadlock);
        assert_eq!(runs, 2);
        assert!(written(&db).await.is_empty());
    }

    #[tokio::test]
    async fn another_failure_is_returned_at_once() {
        let db = database().await;
        let (outcome, runs) = run(&db, 5, DbFailureKind::UniqueViolation, 5).await;
        assert!(outcome.unwrap_err().is_unique_violation());
        assert_eq!(runs, 1);
    }

    #[tokio::test]
    async fn inside_a_transaction_it_runs_once() {
        let db = database().await;
        let (outcome, runs) = db
            .atomically(async { Ok::<_, Error>(run(&db, 5, DbFailureKind::Deadlock, 2).await) })
            .await
            .unwrap();
        assert_eq!(outcome.unwrap_err().failure_kind(), DbFailureKind::Deadlock);
        assert_eq!(runs, 1);

        // Options are the enclosing transaction's; a savepoint takes none.
        let serializable = TransactionOptions {
            isolation_level: Some(crate::database::IsolationLevel::Serializable),
            ..TransactionOptions::default()
        };
        let nested = db
            .test_transaction(async {
                db.transaction_retrying(serializable, 3, async |_| Ok(1))
                    .await
            })
            .await;
        assert_eq!(nested.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_test_transaction_rolls_back_what_it_wrote() {
        let db = database().await;
        let inside = db
            .test_transaction(async {
                db.exec_raw("INSERT INTO runs VALUES (1)").await?;
                // A nested transaction's commit is a savepoint's.
                db.atomically(async { db.exec_raw("INSERT INTO runs VALUES (2)").await })
                    .await?;
                Ok(written(&db).await)
            })
            .await
            .unwrap();
        assert_eq!(inside, [1, 2]);
        assert!(written(&db).await.is_empty());

        let failed = db
            .test_transaction(async {
                db.exec_raw("INSERT INTO runs VALUES (3)").await?;
                Err::<(), _>(Error::validation("attempt", "is wrong"))
            })
            .await;
        assert!(failed.unwrap_err().to_string().contains("is wrong"));
        assert!(written(&db).await.is_empty());
    }
}
