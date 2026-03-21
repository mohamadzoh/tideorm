use super::{Connection, Database};

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
        assert!(super::__current_connection().is_ok());
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

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn thread_override_is_reinstalled_when_future_is_polled_on_another_thread() {
    use std::task::{Context, Waker};

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed");
    let handle = super::DatabaseHandle::Connection(
        db.current_inner()
            .expect("database should expose internal connection"),
    );
    let polled_threads = Arc::new(Mutex::new(Vec::new()));
    let future = OverrideVisibleAcrossPolls {
        polled_threads: polled_threads.clone(),
        stage: 0,
    };
    let mut future = Box::pin(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);

    assert!(matches!(
        super::poll_with_thread_override(future.as_mut(), &mut context, &handle),
        Poll::Pending
    ));
    assert!(super::__current_connection().is_err());

    let join = std::thread::spawn(move || {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(
            super::poll_with_thread_override(future.as_mut(), &mut context, &handle),
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
async fn raw_json_preserves_decimal_and_datetime_column_types() {
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

    let expected_amount = serde_json::to_value(
        rust_decimal::Decimal::from_str_exact("12.34")
            .expect("decimal literal should parse for comparison"),
    )
    .expect("decimal should serialize to JSON");
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
