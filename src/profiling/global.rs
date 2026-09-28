use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::logging::{DEFAULT_SLOW_QUERY_THRESHOLD_MS, QueryStats, StatsCounters};

/// Global flag controlling process-wide profiling collection.
static GLOBAL_PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

static GLOBAL_STATS: StatsCounters = StatsCounters::new(DEFAULT_SLOW_QUERY_THRESHOLD_MS);

/// Aggregate counters collected by [`GlobalProfiler`].
///
/// The same type [`QueryLogger::stats`](crate::logging::QueryLogger::stats)
/// returns; the profiler keeps its own counters and slow-query threshold.
pub type GlobalStats = QueryStats;

/// Process-wide profiling controls.
///
/// Once [`enable`](Self::enable)d, every statement TideORM executes is timed
/// into the global counters. [`Profiler`](super::Profiler) is the manual
/// collector, which records only what it is handed.
pub struct GlobalProfiler;

impl GlobalProfiler {
    /// Start collecting aggregate timings for profiled execution paths.
    pub fn enable() {
        GLOBAL_PROFILING_ENABLED.store(true, Ordering::SeqCst);
    }

    /// Stop collecting aggregate timings.
    pub fn disable() {
        GLOBAL_PROFILING_ENABLED.store(false, Ordering::SeqCst);
    }

    /// Return whether global profiling is currently active.
    pub fn is_enabled() -> bool {
        GLOBAL_PROFILING_ENABLED.load(Ordering::SeqCst)
    }

    /// Add one query duration to the global counters when profiling is enabled.
    pub fn record(duration: Duration) {
        if Self::is_enabled() {
            GLOBAL_STATS.record(Some(duration));
        }
    }

    /// Snapshot the current global counters.
    pub fn stats() -> GlobalStats {
        GLOBAL_STATS.snapshot()
    }

    /// Clear all global counters.
    pub fn reset() {
        GLOBAL_STATS.reset();
    }

    /// Change the duration, in milliseconds, used to classify slow queries.
    pub fn set_slow_threshold(ms: u64) {
        GLOBAL_STATS.set_slow_threshold_ms(ms);
    }

    /// The duration at or above which a query counts as slow.
    pub(super) fn slow_threshold() -> Duration {
        Duration::from_millis(GLOBAL_STATS.slow_threshold_ms())
    }
}

#[doc(hidden)]
pub async fn __profile_future<T, F>(future: F) -> T
where
    F: Future<Output = T>,
{
    if !GlobalProfiler::is_enabled() {
        return future.await;
    }

    let start = Instant::now();
    let output = future.await;
    GlobalProfiler::record(start.elapsed());
    output
}
