use super::*;
use futures_util::{FutureExt, task::noop_waker_ref};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::task::Context;

type Waiter = Pin<Box<dyn Future<Output = MemoryLock>>>;

/// A held lock on `name` and a second acquisition already waiting for it.
fn held_with_waiter(name: &Path) -> (MemoryLock, Waiter) {
    let holder = MemoryLock::acquire(name.to_path_buf())
        .now_or_never()
        .unwrap();
    let mut waiter: Waiter = Box::pin(MemoryLock::acquire(name.to_path_buf()));
    let mut context = Context::from_waker(noop_waker_ref());
    assert!(waiter.as_mut().poll(&mut context).is_pending());
    (holder, waiter)
}

#[test]
fn cancelled_waiters_and_finished_databases_release_registry_entries() {
    for index in 0..256 {
        let name = PathBuf::from(format!("memory-lock-cleanup-{index}"));
        let (holder, waiter) = held_with_waiter(&name);
        if index % 2 == 0 {
            drop(waiter);
            assert!(locks().lock().contains_key(&name));
            drop(holder);
        } else {
            drop(holder);
            assert!(locks().lock().contains_key(&name));
            drop(waiter);
        }
        assert!(!locks().lock().contains_key(&name));
    }
}

#[test]
fn a_waiter_keeps_the_shared_mutex_after_its_holder_exits() {
    let name = PathBuf::from("memory-lock-handoff");
    let (holder, waiter) = held_with_waiter(&name);
    drop(holder);
    let next = waiter.now_or_never().unwrap();
    assert!(MemoryLock::acquire(name.clone()).now_or_never().is_none());
    drop(next);
    assert!(!locks().lock().contains_key(&name));
}
