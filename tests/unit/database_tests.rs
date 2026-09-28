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
            Ok(super::ConnectionRef::Transaction(_))
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
fn hidden_accessors_return_errors_for_disconnected_database() {
    let db = Database::disconnected();

    assert!(db.__internal_connection().is_err());
    assert!(db.__internal_backend().is_err());
    assert!(db.__get_connection().is_err());
}

#[test]
fn backend_defaults_safely_for_disconnected_database() {
    let db = Database::disconnected();

    assert_eq!(db.backend(), crate::config::DatabaseType::Postgres);
}

#[test]
fn debug_reports_disconnected_database_state() {
    let db = Database::disconnected();

    assert_eq!(format!("{:?}", db), "Database { connected: false }");
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn global_database_round_trips_through_set_and_reset() {
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
    let handle = super::ConnectionRef::Transaction(Arc::new(transaction));
    let polled_threads = Arc::new(Mutex::new(Vec::new()));
    let mut future = Box::pin(super::state::with_connection_override(
        handle,
        None,
        None,
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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn raw_json_preserves_boolean_and_json_column_types() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");

    db.__execute_with_params(
        "CREATE TABLE raw_json_probe (enabled BOOLEAN NOT NULL, payload JSON NOT NULL)",
        vec![],
    )
    .await
    .expect("creating probe table should succeed");

    db.__execute_with_params(
        "INSERT INTO raw_json_probe (enabled, payload) VALUES (?, ?)",
        vec![
            crate::internal::Value::Bool(Some(true)),
            crate::internal::Value::Json(Some(Box::new(serde_json::json!({
                "kind": "probe",
                "count": 2
            })))),
        ],
    )
    .await
    .expect("inserting probe row should succeed");

    let rows = db
        .__raw_json_with_params("SELECT enabled, payload FROM raw_json_probe", vec![])
        .await
        .expect("querying raw JSON rows should succeed");

    assert_eq!(
        rows,
        vec![serde_json::json!({
            "enabled": true,
            "payload": {
                "kind": "probe",
                "count": 2
            }
        })]
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn raw_json_reads_decimal_and_datetime_columns_by_what_sqlite_stores() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");

    db.__execute_with_params(
        "CREATE TABLE raw_json_typed_probe (amount DECIMAL NOT NULL, created_at DATETIME NOT NULL)",
        vec![],
    )
    .await
    .expect("creating typed probe table should succeed");

    db.__execute_with_params(
        "INSERT INTO raw_json_typed_probe (amount, created_at) VALUES (?, ?)",
        vec![
            crate::internal::Value::String(Some("12.34".to_string())),
            crate::internal::Value::String(Some("2026-03-21 10:11:12".to_string())),
        ],
    )
    .await
    .expect("inserting typed probe row should succeed");

    let rows = db
        .__raw_json_with_params(
            "SELECT amount, created_at FROM raw_json_typed_probe",
            vec![],
        )
        .await
        .expect("querying typed raw JSON rows should succeed");

    // SQLite stores a `DECIMAL` as a REAL, and raw SQL has no model to say
    // the column is meant as a decimal.
    let expected_amount = serde_json::json!(12.34);
    let expected_created_at = serde_json::to_value(
        chrono::NaiveDateTime::parse_from_str("2026-03-21 10:11:12", "%Y-%m-%d %H:%M:%S")
            .expect("datetime literal should parse for comparison"),
    )
    .expect("datetime should serialize to JSON");

    assert_eq!(
        rows,
        vec![serde_json::json!({
            "amount": expected_amount,
            "created_at": expected_created_at,
        })]
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn raw_json_preserves_count_aggregates_as_numbers() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");

    db.__execute_with_params(
        "CREATE TABLE raw_json_count_probe (enabled BOOLEAN NOT NULL)",
        vec![],
    )
    .await
    .expect("creating count probe table should succeed");

    for enabled in [true, true, false] {
        db.__execute_with_params(
            "INSERT INTO raw_json_count_probe (enabled) VALUES (?)",
            vec![crate::internal::Value::Bool(Some(enabled))],
        )
        .await
        .expect("inserting count probe row should succeed");
    }

    let rows = db
        .__raw_json_with_params(
            "SELECT COUNT(*) AS count, SUM(enabled) AS enabled_total FROM raw_json_count_probe",
            vec![],
        )
        .await
        .expect("querying count aggregate JSON rows should succeed");

    assert_eq!(
        rows,
        vec![serde_json::json!({
            "count": 3,
            "enabled_total": 2,
        })]
    );
}

/// A float sum, a running total and an average are numbers: they used to come
/// back as decimal strings, which `as_f64()` reads as nothing.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn raw_json_reads_real_expressions_as_numbers() {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    for statement in [
        "CREATE TABLE raw_json_real_probe (id INTEGER PRIMARY KEY, amount REAL NOT NULL)",
        "INSERT INTO raw_json_real_probe (amount) VALUES (1.5), (2.25)",
    ] {
        db.__execute_with_params(statement, vec![])
            .await
            .expect("setting up the probe table should succeed");
    }

    let rows = db
        .__raw_json_with_params(
            "SELECT SUM(amount) OVER (ORDER BY id) AS running, AVG(id) OVER () AS average              FROM raw_json_real_probe ORDER BY id",
            vec![],
        )
        .await
        .expect("querying real expressions should succeed");

    assert_eq!(
        rows,
        vec![
            serde_json::json!({"running": 1.5, "average": 1.5}),
            serde_json::json!({"running": 3.75, "average": 1.5}),
        ]
    );
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
