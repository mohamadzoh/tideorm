use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::logging::{QueryStats, StatsCounters, slow_query_threshold_from_env};

/// Global flag controlling process-wide profiling collection.
static GLOBAL_PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Starts from the threshold `TIDE_SLOW_QUERY_MS` sets, as the logger does.
static GLOBAL_STATS: LazyLock<StatsCounters> =
    LazyLock::new(|| StatsCounters::new(slow_query_threshold_from_env()));

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

    /// Change the duration at or above which a query counts as slow; it
    /// starts at `TIDE_SLOW_QUERY_MS`, or 100 ms.
    pub fn set_slow_threshold(threshold: Duration) {
        GLOBAL_STATS.set_slow_threshold(threshold);
    }

    /// The duration at or above which a query counts as slow.
    pub fn slow_threshold() -> Duration {
        GLOBAL_STATS.slow_threshold()
    }
}
