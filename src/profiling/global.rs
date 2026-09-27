use parking_lot::RwLock;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::logging::{DEFAULT_SLOW_QUERY_THRESHOLD_MS, QueryStats};

/// Global flag controlling process-wide profiling collection.
static GLOBAL_PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

static GLOBAL_STATS: RwLock<QueryStats> = RwLock::new(QueryStats {
    total_queries: 0,
    slow_queries: 0,
    total_time_ns: 0,
    slow_threshold_ms: DEFAULT_SLOW_QUERY_THRESHOLD_MS,
});

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
            let mut stats = GLOBAL_STATS.write();

            stats.total_queries += 1;
            stats.total_time_ns += duration.as_nanos() as u64;

            if duration.as_millis() as u64 >= stats.slow_threshold_ms {
                stats.slow_queries += 1;
            }
        }
    }

    /// Snapshot the current global counters.
    pub fn stats() -> GlobalStats {
        *GLOBAL_STATS.read()
    }

    /// Clear all global counters.
    pub fn reset() {
        let mut stats = GLOBAL_STATS.write();
        stats.total_queries = 0;
        stats.slow_queries = 0;
        stats.total_time_ns = 0;
    }

    /// Change the duration, in milliseconds, used to classify slow queries.
    pub fn set_slow_threshold(ms: u64) {
        GLOBAL_STATS.write().slow_threshold_ms = ms;
    }

    /// The duration at or above which a query counts as slow.
    pub(super) fn slow_threshold() -> Duration {
        Duration::from_millis(GLOBAL_STATS.read().slow_threshold_ms)
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
