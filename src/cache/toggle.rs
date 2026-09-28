//! The configuration and on/off switch both caches keep.

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::sync::atomic::{AtomicBool, Ordering};

/// A cache configuration with an `enabled` flag.
pub(super) trait Switchable {
    fn enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);
}

/// A cache's configuration, with its `enabled` flag mirrored in an atomic so
/// every query can check it without taking the lock.
#[derive(Debug)]
pub(super) struct Toggle<C> {
    config: RwLock<C>,
    enabled: AtomicBool,
}

impl<C: Switchable> Toggle<C> {
    pub(super) fn new(config: C) -> Self {
        Self {
            enabled: AtomicBool::new(config.enabled()),
            config: RwLock::new(config),
        }
    }

    pub(super) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(super) fn set_enabled(&self, enabled: bool) {
        self.config.write().set_enabled(enabled);
        self.enabled.store(enabled, Ordering::Release);
    }

    /// Replace the whole configuration, its `enabled` flag included.
    pub(super) fn apply(&self, config: C) {
        let enabled = config.enabled();
        *self.config.write() = config;
        self.enabled.store(enabled, Ordering::Release);
    }

    pub(super) fn read(&self) -> RwLockReadGuard<'_, C> {
        self.config.read()
    }

    /// The configuration to change a setting other than `enabled`, which goes
    /// through [`Toggle::set_enabled`] so the atomic follows it.
    pub(super) fn write(&self) -> RwLockWriteGuard<'_, C> {
        self.config.write()
    }
}

/// Hits over all lookups, or `0.0` before the first.
pub(super) fn hit_ratio(hits: u64, misses: u64) -> f64 {
    let total = hits + misses;
    if total == 0 {
        0.0
    } else {
        hits as f64 / total as f64
    }
}
