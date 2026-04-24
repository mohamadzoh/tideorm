use parking_lot::RwLock;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Global flag controlling process-wide profiling collection.
static GLOBAL_PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Default)]
struct GlobalStatsState {
    total_queries: u64,
    total_time_ns: u64,
    slow_queries: u64,
}

static GLOBAL_STATS: RwLock<GlobalStatsState> = RwLock::new(GlobalStatsState {
    total_queries: 0,
    total_time_ns: 0,
    slow_queries: 0,
});

static GLOBAL_SLOW_THRESHOLD_MS: RwLock<u64> = RwLock::new(100);

/// Process-wide profiling controls.
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
            let threshold_ms = *GLOBAL_SLOW_THRESHOLD_MS.read();
            let mut stats = GLOBAL_STATS.write();

            stats.total_queries += 1;
            stats.total_time_ns += duration.as_nanos() as u64;

            if duration.as_millis() as u64 >= threshold_ms {
                stats.slow_queries += 1;
            }
        }
    }

    /// Snapshot the current global counters.
    pub fn stats() -> GlobalStats {
        let stats = *GLOBAL_STATS.read();

        GlobalStats {
            total_queries: stats.total_queries,
            total_time_ns: stats.total_time_ns,
            slow_queries: stats.slow_queries,
            slow_threshold_ms: *GLOBAL_SLOW_THRESHOLD_MS.read(),
        }
    }

    /// Clear all global counters.
    pub fn reset() {
        *GLOBAL_STATS.write() = GlobalStatsState::default();
    }

    /// Change the duration, in milliseconds, used to classify slow queries.
    pub fn set_slow_threshold(ms: u64) {
        *GLOBAL_SLOW_THRESHOLD_MS.write() = ms;
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

/// Aggregate counters collected by `GlobalProfiler`.
#[derive(Debug, Clone, Copy)]
pub struct GlobalStats {
    /// Number of recorded queries.
    pub total_queries: u64,
    /// Sum of recorded query time in nanoseconds.
    pub total_time_ns: u64,
    /// Number of queries at or above the slow threshold.
    pub slow_queries: u64,
    /// Slow-query threshold in milliseconds.
    pub slow_threshold_ms: u64,
}

impl GlobalStats {
    /// Convert the accumulated nanoseconds into a `Duration`.
    pub fn total_time(&self) -> Duration {
        Duration::from_nanos(self.total_time_ns)
    }

    /// Average duration per recorded query, or zero when nothing ran.
    pub fn avg_query_time(&self) -> Duration {
        self.total_time_ns
            .checked_div(self.total_queries)
            .map(Duration::from_nanos)
            .unwrap_or(Duration::ZERO)
    }

    /// Percentage of recorded queries that crossed the slow-query threshold.
    pub fn slow_percentage(&self) -> f64 {
        if self.total_queries == 0 {
            0.0
        } else {
            (self.slow_queries as f64 / self.total_queries as f64) * 100.0
        }
    }
}

impl fmt::Display for GlobalStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TideORM Global Statistics:")?;
        writeln!(f, "  Total Queries:    {}", self.total_queries)?;
        writeln!(
            f,
            "  Total Time:       {:.2}ms",
            self.total_time().as_secs_f64() * 1000.0
        )?;
        writeln!(
            f,
            "  Avg Query Time:   {:.2}ms",
            self.avg_query_time().as_secs_f64() * 1000.0
        )?;
        writeln!(
            f,
            "  Slow Queries:     {} ({:.1}%)",
            self.slow_queries,
            self.slow_percentage()
        )?;
        write!(f, "  Slow Threshold:   {}ms", self.slow_threshold_ms)
    }
}
