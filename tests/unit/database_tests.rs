use super::Database;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::{Arc, Mutex};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::task::Poll;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
struct OverrideVisibleAcrossPolls {
    polled_threads: Arc<Mutex<Vec<std::thread::ThreadId>>>,
    stage: usize,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
impl std::future::Future for OverrideVisibleAcrossPolls {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        assert!(matches!(
            super::__current_connection(),
            Ok(super::ConnectionRef::Transaction(..))
        ));
        self.polled_threads
            .lock()
            .expect("thread list lock should not be poisoned")
            .push(std::thread::current().id());

        if self.stage == 0 {
            self.stage = 1;
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }

        std::task::Poll::Ready(())
    }
}

#[test]
fn a_disconnected_database_fails_its_accessors_and_reports_safe_defaults() {
    let db = Database::disconnected();

    assert!(db.__internal_connection().is_err());
    assert!(db.__internal_backend().is_err());
    assert!(db.__get_connection().is_err());
    assert_eq!(db.backend(), crate::config::DatabaseType::Postgres);
    assert_eq!(format!("{:?}", db), "Database { connected: false }");
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn global_database_round_trips_through_set_and_reset() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    Database::reset_global();
    assert!(!crate::database::has_global_db());
    assert!(super::require_db().is_err());

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");

    Database::set_global(db.clone()).expect("setting global database should succeed");

    assert!(crate::database::has_global_db());
    assert!(super::require_db().is_ok());
    assert!(format!("{:?}", super::db()).contains("connected: true"));

    Database::reset_global();

    assert!(!crate::database::has_global_db());
    assert!(super::require_db().is_err());
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn transaction_override_remains_visible_when_scoped_future_moves_threads() {
    use crate::internal::TransactionTrait;
    use std::task::{Context, Waker};

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    let transaction = db
        .current_inner()
        .expect("database should expose internal connection")
        .connection()
        .begin()
        .await
        .expect("transaction should begin successfully");
    let handle = super::ConnectionRef::Transaction(Arc::new(transaction), Default::default());
    let polled_threads = Arc::new(Mutex::new(Vec::new()));
    let mut future = Box::pin(super::state::with_connection_override(
        handle,
        OverrideVisibleAcrossPolls {
            polled_threads: polled_threads.clone(),
            stage: 0,
        },
    ));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let runtime_handle = tokio::runtime::Handle::current();

    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    assert!(super::__current_connection().is_err());

    let join = std::thread::spawn(move || {
        let _guard = runtime_handle.enter();
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(
            future.as_mut().poll(&mut context),
            Poll::Ready(())
        ));
        assert!(super::__current_connection().is_err());
    });

    join.join()
        .expect("cross-thread poll should complete successfully");

    let polled_threads = polled_threads
        .lock()
        .expect("thread list lock should not be poisoned");
    assert_eq!(polled_threads.len(), 2);
    assert_ne!(polled_threads[0], polled_threads[1]);
}

/// Raw JSON rows decode a column by the type it declares, and by what SQLite
/// stores where nothing declares one.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn raw_json_reads_sqlite_values_by_declared_type_or_storage_class() {
    use serde_json::json;

    for (setup, query, expected) in [
        (
            [
                "CREATE TABLE probe (enabled BOOLEAN NOT NULL, payload JSON NOT NULL)",
                r#"INSERT INTO probe VALUES (TRUE, '{"kind": "probe", "count": 2}')"#,
            ],
            "SELECT enabled, payload FROM probe",
            json!([{"enabled": true, "payload": {"kind": "probe", "count": 2}}]),
        ),
        // SQLite stores a `DECIMAL` as a REAL, and raw SQL has no model to say
        // the column is meant as a decimal.
        (
            [
                "CREATE TABLE probe (amount DECIMAL NOT NULL, created_at DATETIME NOT NULL)",
                "INSERT INTO probe VALUES ('12.34', '2026-03-21 10:11:12')",
            ],
            "SELECT amount, created_at FROM probe",
            json!([{"amount": 12.34, "created_at": "2026-03-21T10:11:12"}]),
        ),
        (
            [
                "CREATE TABLE probe (enabled BOOLEAN NOT NULL)",
                "INSERT INTO probe VALUES (TRUE), (TRUE), (FALSE)",
            ],
            "SELECT COUNT(*) AS count, SUM(enabled) AS enabled_total FROM probe",
            json!([{"count": 3, "enabled_total": 2}]),
        ),
        // A float sum, a running total and an average are numbers: they used
        // to come back as decimal strings, which `as_f64()` reads as nothing.
        (
            [
                "CREATE TABLE probe (id INTEGER PRIMARY KEY, amount REAL NOT NULL)",
                "INSERT INTO probe (amount) VALUES (1.5), (2.25)",
            ],
            "SELECT SUM(amount) OVER (ORDER BY id) AS running, AVG(id) OVER () AS average \
             FROM probe ORDER BY id",
            json!([
                {"running": 1.5, "average": 1.5},
                {"running": 3.75, "average": 1.5},
            ]),
        ),
    ] {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("sqlite in-memory connection should succeed");
        for statement in setup {
            db.__execute_with_params(statement, vec![])
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }

        let rows = db
            .__raw_json_with_params(query, vec![])
            .await
            .unwrap_or_else(|error| panic!("{query}: {error}"));
        assert_eq!(serde_json::Value::Array(rows), expected, "{query}");
    }
}

#[test]
fn database_builder_records_an_acquire_timeout_override() {
    let default_debug = format!("{:?}", Database::builder());
    assert!(
        default_debug.contains("acquire_timeout: None"),
        "{default_debug}"
    );

    let configured = Database::builder()
        .url("sqlite::memory:")
        .acquire_timeout(std::time::Duration::from_secs(23));
    let configured_debug = format!("{configured:?}");
    assert!(
        configured_debug.contains("acquire_timeout: Some("),
        "the acquire_timeout knob must reach the pool options: {configured_debug}"
    );
}

#[test]
fn database_builder_debug_masks_the_url_credentials() {
    let password = format!("pw{}", std::process::id());
    let builder = Database::builder().url(format!("postgres://app:{password}@db:5432/app"));
    let debug = format!("{builder:?}");

    assert!(!debug.contains(&password), "{debug}");
    assert!(debug.contains("postgres://***@db:5432/app"), "{debug}");
}

/// A transaction is joined by the handles of its own database only: inside
/// one on `first`, a statement through `second` runs on `second`.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn a_transaction_is_joined_only_by_its_own_database() {
    let first = Database::connect("sqlite::memory:").await.unwrap();
    let second = Database::connect("sqlite::memory:").await.unwrap();
    for (db, name) in [(&first, "first"), (&second, "second")] {
        db.__execute_with_params("CREATE TABLE which_database (name TEXT NOT NULL)", vec![])
            .await
            .unwrap();
        db.__execute_with_params(
            "INSERT INTO which_database (name) VALUES (?)",
            vec![crate::internal::Value::String(Some(name.to_string()))],
        )
        .await
        .unwrap();
    }

    let (own, other) = (first.clone(), second.clone());
    let (own_rows, other_rows) = first
        .transaction(move |_| {
            Box::pin(async move {
                let sql = "SELECT name FROM which_database";
                let own_rows = own.__raw_json_with_params(sql, vec![]).await?;
                let other_rows = other.__raw_json_with_params(sql, vec![]).await?;
                Ok((own_rows, other_rows))
            })
        })
        .await
        .unwrap();

    assert_eq!(own_rows, vec![serde_json::json!({"name": "first"})]);
    assert_eq!(other_rows, vec![serde_json::json!({"name": "second"})]);
}
