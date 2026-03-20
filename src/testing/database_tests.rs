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