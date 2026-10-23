//! Migration mutexes shared by the holders and waiters of each memory database.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};

use futures_util::lock::{Mutex, OwnedMutexGuard};

type Locks = HashMap<PathBuf, Weak<MemoryMutex>>;

fn locks() -> &'static parking_lot::Mutex<Locks> {
    static LOCKS: OnceLock<parking_lot::Mutex<Locks>> = OnceLock::new();
    LOCKS.get_or_init(Default::default)
}

pub(super) struct MemoryLock {
    // Release the mutex before releasing its registry ownership.
    _guard: OwnedMutexGuard<()>,
    _entry: Arc<MemoryMutex>,
}

struct MemoryMutex {
    name: PathBuf,
    mutex: Arc<Mutex<()>>,
}

impl MemoryLock {
    pub(super) async fn acquire(name: PathBuf) -> Self {
        let entry = {
            let mut locks = locks().lock();
            match locks.get(&name).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(MemoryMutex {
                        name: name.clone(),
                        mutex: Arc::new(Mutex::new(())),
                    });
                    locks.insert(name.clone(), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        // The entry lives while waiting too, so cancellation releases registry
        // ownership and other waiters keep sharing the same mutex.
        let guard = entry.mutex.clone().lock_owned().await;
        Self {
            _guard: guard,
            _entry: entry,
        }
    }
}

impl Drop for MemoryMutex {
    fn drop(&mut self) {
        let mut locks = locks().lock();
        // Another acquisition may have replaced our expired Weak before this
        // destructor got the registry lock. Never remove that replacement.
        if locks
            .get(&self.name)
            .is_some_and(|entry| std::ptr::eq(entry.as_ptr(), self))
        {
            locks.remove(&self.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{FutureExt, task::noop_waker_ref};
    use std::future::Future;
    use std::task::Context;

    #[test]
    fn cancelled_waiters_and_finished_databases_release_registry_entries() {
        for index in 0..256 {
            let name = PathBuf::from(format!("memory-lock-cleanup-{index}"));
            let holder = MemoryLock::acquire(name.clone()).now_or_never().unwrap();
            let mut waiter = Box::pin(MemoryLock::acquire(name.clone()));
            let mut context = Context::from_waker(noop_waker_ref());
            assert!(waiter.as_mut().poll(&mut context).is_pending());
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
        let holder = MemoryLock::acquire(name.clone()).now_or_never().unwrap();
        let mut waiter = Box::pin(MemoryLock::acquire(name.clone()));
        let mut context = Context::from_waker(noop_waker_ref());
        assert!(waiter.as_mut().poll(&mut context).is_pending());
        drop(holder);
        let next = waiter.now_or_never().unwrap();
        assert!(MemoryLock::acquire(name.clone()).now_or_never().is_none());
        drop(next);
        assert!(!locks().lock().contains_key(&name));
    }
}
