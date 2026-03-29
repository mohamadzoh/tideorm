use arc_swap::ArcSwapOption;
use std::future::Future;
#[cfg(not(feature = "runtime-tokio"))]
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
#[cfg(not(feature = "runtime-tokio"))]
use std::task::{Context, Poll};

#[cfg(not(feature = "runtime-tokio"))]
use std::cell::RefCell;

use crate::error::{Error, Result};
use crate::internal::{Backend, InternalConnection};

use super::{ConnectionRef, Database};

static GLOBAL_DB: OnceLock<Database> = OnceLock::new();
static GLOBAL_CONNECTION: OnceLock<ArcSwapOption<InternalConnection>> = OnceLock::new();

#[cfg(feature = "runtime-tokio")]
tokio::task_local! {
    pub(super) static TASK_DB_OVERRIDE: DatabaseHandle;
}

#[cfg(not(feature = "runtime-tokio"))]
thread_local! {
    pub(super) static THREAD_DB_OVERRIDE: RefCell<Option<DatabaseHandle>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub(crate) enum DatabaseHandle {
    Connection(Arc<InternalConnection>),
    Transaction(Arc<crate::internal::OrmTransaction>),
    #[cfg(test)]
    #[allow(dead_code)]
    TestScope,
}

pub(super) fn global_connection_slot() -> &'static ArcSwapOption<InternalConnection> {
    GLOBAL_CONNECTION.get_or_init(|| ArcSwapOption::new(None))
}

pub(super) fn global_db_handle() -> &'static Database {
    GLOBAL_DB.get_or_init(Database::global_handle)
}

pub(super) fn panic_missing_global_db(message: &str) -> ! {
    panic!("{}", message)
}

/// Get a reference to the global database connection.
///
/// Panics if the global connection has not been initialized.
pub fn db() -> &'static Database {
    let db = global_db_handle();
    if db.is_connected() {
        db
    } else {
        panic_missing_global_db(
            "Global database connection not initialized. \
             Call Database::init() or Database::set_global() before using models. \
             Use try_db() for a non-panicking alternative.",
        )
    }
}

/// Get the global database handle, returning an error if not initialized.
pub fn require_db() -> Result<Database> {
    global_connection_slot()
        .load_full()
        .map(|inner| Database::from_handle(DatabaseHandle::Connection(inner)))
        .ok_or_else(|| {
            Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )
        })
}

/// Try to get the global database handle.
pub fn try_db() -> Option<Database> {
    global_connection_slot()
        .load_full()
        .map(|inner| Database::from_handle(DatabaseHandle::Connection(inner)))
}

/// Check whether a global database connection has been initialized.
pub fn has_global_db() -> bool {
    global_connection_slot().load_full().is_some()
}

#[cfg(feature = "runtime-tokio")]
fn current_override_handle() -> Option<DatabaseHandle> {
    TASK_DB_OVERRIDE.try_with(Clone::clone).ok()
}

#[cfg(not(feature = "runtime-tokio"))]
fn current_override_handle() -> Option<DatabaseHandle> {
    THREAD_DB_OVERRIDE.with(|slot| slot.borrow().clone())
}

#[cfg(not(feature = "runtime-tokio"))]
struct ResetThreadOverride(Option<DatabaseHandle>);

#[cfg(not(feature = "runtime-tokio"))]
impl Drop for ResetThreadOverride {
    fn drop(&mut self) {
        THREAD_DB_OVERRIDE.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

#[cfg(not(feature = "runtime-tokio"))]
fn install_thread_override(handle: &DatabaseHandle) -> ResetThreadOverride {
    let previous = THREAD_DB_OVERRIDE.with(|slot| slot.replace(Some(handle.clone())));
    ResetThreadOverride(previous)
}

#[doc(hidden)]
pub fn __current_db() -> Result<Database> {
    if let Some(handle) = current_override_handle() {
        return Ok(Database::from_handle(handle));
    }

    require_db()
}

pub(super) fn current_scope_handle() -> Result<DatabaseHandle> {
    if let Some(handle) = current_override_handle() {
        return Ok(handle);
    }

    global_connection_slot()
        .load_full()
        .map(DatabaseHandle::Connection)
        .ok_or_else(|| {
            Error::connection(
                "Global database connection not initialized. \
                 Call Database::init() or Database::set_global() before using models."
                    .to_string(),
            )
        })
}

#[cfg(feature = "runtime-tokio")]
pub(super) async fn with_connection_override<F>(handle: DatabaseHandle, future: F) -> F::Output
where
    F: Future,
{
    TASK_DB_OVERRIDE.scope(handle, future).await
}

#[cfg(not(feature = "runtime-tokio"))]
pub(super) fn with_connection_override<F>(
    handle: DatabaseHandle,
    future: F,
) -> impl Future<Output = F::Output>
where
    F: Future,
{
    struct ScopedOverrideFuture<F> {
        handle: DatabaseHandle,
        future: Pin<Box<F>>,
    }

    impl<F> Future for ScopedOverrideFuture<F>
    where
        F: Future,
    {
        type Output = F::Output;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let guard = install_thread_override(&this.handle);
            let result = this.future.as_mut().poll(cx);
            drop(guard);
            result
        }
    }

    ScopedOverrideFuture {
        handle,
        future: Box::pin(future),
    }
}

#[doc(hidden)]
pub fn __current_connection() -> Result<ConnectionRef> {
    Ok(match current_scope_handle()? {
        DatabaseHandle::Connection(inner) => ConnectionRef::Database(inner),
        DatabaseHandle::Transaction(tx) => ConnectionRef::Transaction(tx),
        #[cfg(test)]
        DatabaseHandle::TestScope => unreachable!("test scope marker does not carry a connection"),
    })
}

#[doc(hidden)]
pub fn __current_backend() -> Result<Backend> {
    use crate::internal::ConnectionTrait;

    Ok(match current_scope_handle()? {
        DatabaseHandle::Connection(inner) => {
            Backend::from(inner.connection().get_database_backend())
        }
        DatabaseHandle::Transaction(tx) => Backend::from(tx.as_ref().get_database_backend()),
        #[cfg(test)]
        DatabaseHandle::TestScope => unreachable!("test scope marker does not carry a backend"),
    })
}

#[cfg(all(test, not(feature = "runtime-tokio")))]
mod tests {
    use super::{DatabaseHandle, current_override_handle, with_connection_override};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    struct OverrideVisibleAcrossPolls {
        polled_threads: Arc<Mutex<Vec<std::thread::ThreadId>>>,
        stage: usize,
    }

    impl Future for OverrideVisibleAcrossPolls {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            assert!(current_override_handle().is_some());
            self.polled_threads
                .lock()
                .expect("thread list lock should not be poisoned")
                .push(std::thread::current().id());

            if self.stage == 0 {
                self.stage = 1;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }

            Poll::Ready(())
        }
    }

    #[test]
    fn thread_override_is_poll_scoped_and_survives_cross_thread_polls() {
        let polled_threads = Arc::new(Mutex::new(Vec::new()));
        let mut future = Box::pin(with_connection_override(
            DatabaseHandle::TestScope,
            OverrideVisibleAcrossPolls {
                polled_threads: polled_threads.clone(),
                stage: 0,
            },
        ));
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        assert!(current_override_handle().is_none());

        let join = std::thread::spawn(move || {
            let waker = Waker::noop();
            let mut context = Context::from_waker(waker);
            assert!(matches!(
                future.as_mut().poll(&mut context),
                Poll::Ready(())
            ));
            assert!(current_override_handle().is_none());
        });

        join.join()
            .expect("cross-thread poll should complete successfully");

        let polled_threads = polled_threads
            .lock()
            .expect("thread list lock should not be poisoned");
        assert_eq!(polled_threads.len(), 2);
        assert_ne!(polled_threads[0], polled_threads[1]);
    }
}
