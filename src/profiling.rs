//! Performance Profiling for TideORM
//!
//! Use this module to answer performance questions with actual timings instead
//! of guesses.
//!
//! `GlobalProfiler` records statistics for real query execution paths.
//! `Profiler` is for manually assembled reports in tests, benchmarks, or local
//! experiments.
//!
//! Practical split:
//! - use `GlobalProfiler` when you want process-wide timings for real query execution paths
//! - use `Profiler` when you want to build a focused report around one benchmark, test, or experiment
//! - use `QueryAnalyzer` only as a heuristic pass over rendered SQL, not as a substitute for backend query plans

use parking_lot::RwLock;
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Manual profiler for collecting query timings into one report.
pub struct Profiler {
    start_time: Instant,
    queries: Vec<ProfiledQuery>,
    is_active: bool,
}

/// One recorded query plus timing metadata.
#[derive(Debug, Clone)]
pub struct ProfiledQuery {
    /// Rendered SQL text.
    pub sql: String,
    /// Table name used for grouping, if known.
    pub table: Option<String>,
    /// Recorded duration.
    pub duration: Duration,
    /// Affected or returned rows, if known.
    pub rows: Option<u64>,
    /// Whether the data came from cache.
    pub cached: bool,
    /// Operation label such as `SELECT` or `UPDATE`.
    pub operation: String,
    /// Capture time.
    pub timestamp: SystemTime,
}

impl ProfiledQuery {
    /// Capture one SQL statement and its duration.
    pub fn new(sql: impl Into<String>, duration: Duration) -> Self {
        let sql = sql.into();
        let operation = detect_operation(&sql);
        Self {
            sql,
            table: None,
            duration,
            rows: None,
            cached: false,
            operation,
            timestamp: SystemTime::now(),
        }
    }

    /// Attach the table name so grouped reports can point to the hotspot.
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Store how many rows the query touched or returned.
    pub fn with_rows(mut self, rows: u64) -> Self {
        self.rows = Some(rows);
        self
    }

    /// Mark this entry as served from cache.
    pub fn cached(mut self) -> Self {
        self.cached = true;
        self
    }
}

impl Profiler {
    /// Begin collecting queries for a manual profiling run.
    pub fn start() -> Self {
        Self {
            start_time: Instant::now(),
            queries: Vec::new(),
            is_active: true,
        }
    }

    /// Append a SQL statement if the profiler is still active.
    pub fn record(&mut self, sql: impl Into<String>, duration: Duration) {
        if self.is_active {
            self.queries.push(ProfiledQuery::new(sql, duration));
        }
    }

    /// Append a fully populated query entry.
    pub fn record_full(&mut self, query: ProfiledQuery) {
        if self.is_active {
            self.queries.push(query);
        }
    }

    /// Freeze the session and build the final report.
    pub fn stop(mut self) -> ProfileReport {
        self.is_active = false;
        let total_duration = self.start_time.elapsed();
        ProfileReport::from_queries(self.queries, total_duration)
    }

    /// Return how many queries have been collected so far.
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Return wall-clock time since `start()`, including non-query work.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// Summary report built from a profiling session.
#[derive(Debug, Clone)]
pub struct ProfileReport {
    /// Total wall-clock duration for the profiled scope.
    pub total_duration: Duration,
    /// Sum of recorded query durations.
    pub query_duration: Duration,
    /// All recorded queries.
    pub queries: Vec<ProfiledQuery>,
    /// Query counts grouped by operation.
    pub operations: HashMap<String, u64>,
    /// Slowest recorded queries.
    pub slowest: Vec<ProfiledQuery>,
    /// Query counts grouped by table.
    pub tables: HashMap<String, u64>,
}

impl ProfileReport {
    /// Build a report from recorded queries and wall-clock duration.
    fn from_queries(queries: Vec<ProfiledQuery>, total_duration: Duration) -> Self {
        let query_duration: Duration = queries.iter().map(|q| q.duration).sum();

        let mut operations: HashMap<String, u64> = HashMap::new();
        let mut tables: HashMap<String, u64> = HashMap::new();

        for query in &queries {
            *operations.entry(query.operation.clone()).or_insert(0) += 1;
            if let Some(ref table) = query.table {
                *tables.entry(table.clone()).or_insert(0) += 1;
            }
        }

        let mut slowest: Vec<ProfiledQuery> = queries.clone();
        slowest.sort_by(|a, b| b.duration.cmp(&a.duration));
        slowest.truncate(10);

        Self {
            total_duration,
            query_duration,
            queries,
            operations,
            slowest,
            tables,
        }
    }

    /// Total number of recorded queries.
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Average per-query duration, or zero when the report is empty.
    pub fn avg_query_time(&self) -> Duration {
        if self.queries.is_empty() {
            Duration::ZERO
        } else {
            self.query_duration / self.queries.len() as u32
        }
    }

    /// Share of total wall-clock time spent inside recorded queries.
    pub fn query_time_percentage(&self) -> f64 {
        if self.total_duration.as_nanos() == 0 {
            0.0
        } else {
            (self.query_duration.as_nanos() as f64 / self.total_duration.as_nanos() as f64) * 100.0
        }
    }

    /// Return queries at or above the supplied threshold.
    pub fn queries_slower_than(&self, threshold: Duration) -> Vec<&ProfiledQuery> {
        self.queries
            .iter()
            .filter(|q| q.duration >= threshold)
            .collect()
    }

    /// Return simple heuristics that highlight likely hotspots in the report.
    pub fn suggestions(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Check for N+1 query patterns
        let mut table_counts: HashMap<&str, usize> = HashMap::new();
        for query in &self.queries {
            if let Some(ref table) = query.table {
                *table_counts.entry(table.as_str()).or_insert(0) += 1;
            }
        }

        for (table, count) in table_counts {
            if count > 10 {
                suggestions.push(format!(
                    "Potential N+1 query detected: {} queries on '{}' table. Consider using eager loading with `.with(\"{}\")` or batch queries.",
                    count, table, table
                ));
            }
        }

        // Check for slow queries
        let slow_count = self
            .queries
            .iter()
            .filter(|q| q.duration > Duration::from_millis(100))
            .count();

        if slow_count > 0 {
            suggestions.push(format!(
                "{} slow queries detected (>100ms). Review these queries and consider adding indexes.",
                slow_count
            ));
        }

        // Check for SELECT * usage
        let select_star = self
            .queries
            .iter()
            .filter(|q| q.sql.contains("SELECT *") || q.sql.contains("select *"))
            .count();

        if select_star > 5 {
            suggestions.push(
                "Multiple SELECT * queries detected. Use `.select([\"col1\", \"col2\"])` to fetch only needed columns.".to_string()
            );
        }

        // Check query time percentage
        if self.query_time_percentage() > 50.0 {
            suggestions.push(
                "More than 50% of time spent in database queries. Consider caching frequently accessed data.".to_string()
            );
        }

        suggestions
    }
}

impl fmt::Display for ProfileReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "╔═══════════════════════════════════════════════════════════╗"
        )?;
        writeln!(
            f,
            "║           TIDEORM PERFORMANCE PROFILE REPORT              ║"
        )?;
        writeln!(
            f,
            "╠═══════════════════════════════════════════════════════════╣"
        )?;
        writeln!(
            f,
            "║ Total Duration:     {:>10}ms                          ║",
            self.total_duration.as_millis()
        )?;
        writeln!(
            f,
            "║ Query Duration:     {:>10}ms ({:.1}% of total)        ║",
            self.query_duration.as_millis(),
            self.query_time_percentage()
        )?;
        writeln!(
            f,
            "║ Total Queries:      {:>10}                            ║",
            self.query_count()
        )?;
        writeln!(
            f,
            "║ Avg Query Time:     {:>10.2}ms                         ║",
            self.avg_query_time().as_secs_f64() * 1000.0
        )?;
        writeln!(
            f,
            "╠═══════════════════════════════════════════════════════════╣"
        )?;

        // Operations breakdown
        writeln!(
            f,
            "║ Operations:                                               ║"
        )?;
        for (op, count) in &self.operations {
            writeln!(
                f,
                "║   {:10}: {:>6}                                       ║",
                op, count
            )?;
        }

        // Slowest queries
        if !self.slowest.is_empty() {
            writeln!(
                f,
                "╠═══════════════════════════════════════════════════════════╣"
            )?;
            writeln!(
                f,
                "║ Slowest Queries:                                          ║"
            )?;
            for (i, query) in self.slowest.iter().take(5).enumerate() {
                let sql_preview: String = query.sql.chars().take(40).collect();
                writeln!(
                    f,
                    "║ {}. {:>6}ms  {}...                                         ║",
                    i + 1,
                    query.duration.as_millis(),
                    sql_preview.replace('\n', " ")
                )?;
            }
        }

        // Suggestions
        let suggestions = self.suggestions();
        if !suggestions.is_empty() {
            writeln!(
                f,
                "╠═══════════════════════════════════════════════════════════╣"
            )?;
            writeln!(
                f,
                "║ 💡 Optimization Suggestions:                              ║"
            )?;
            for suggestion in suggestions.iter().take(3) {
                let wrapped = textwrap_simple(suggestion, 55);
                for line in wrapped {
                    writeln!(f, "║   {}{}║", line, " ".repeat(55 - line.len().min(55)))?;
                }
            }
        }

        writeln!(
            f,
            "╚═══════════════════════════════════════════════════════════╝"
        )
    }
}

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
        if self.total_queries == 0 {
            Duration::ZERO
        } else {
            Duration::from_nanos(self.total_time_ns / self.total_queries)
        }
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

/// Heuristic analyzer for rendered SQL strings.
pub struct QueryAnalyzer;

impl QueryAnalyzer {
    /// Run simple SQL heuristics against a rendered query string.
    pub fn analyze(sql: &str) -> Vec<QuerySuggestion> {
        let mut suggestions = Vec::new();
        let sql_upper = sql.to_uppercase();

        // Check for SELECT *
        if sql_upper.contains("SELECT *") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Avoid SELECT *",
                "Specify columns explicitly to reduce data transfer and improve performance.",
                "Change to: .select([\"id\", \"name\", \"email\"])",
            ));
        }

        // Check for missing WHERE on UPDATE/DELETE
        if (sql_upper.starts_with("UPDATE") || sql_upper.starts_with("DELETE"))
            && !sql_upper.contains("WHERE")
        {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Critical,
                "Missing WHERE clause",
                "UPDATE/DELETE without WHERE will affect all rows!",
                "Add a WHERE condition: .where_eq(\"id\", value)",
            ));
        }

        // Check for LIKE with leading wildcard
        if sql_upper.contains("LIKE '%") || sql_upper.contains("LIKE '%") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Leading wildcard in LIKE",
                "LIKE '%pattern' cannot use indexes and will be slow on large tables.",
                "Consider using full-text search or restructure the query.",
            ));
        }

        // Check for OR conditions
        if sql_upper.contains(" OR ") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "OR conditions detected",
                "OR conditions may prevent index usage. Consider using UNION or restructuring.",
                "Use .where_in(\"column\", values) instead of multiple OR conditions.",
            ));
        }

        // Check for ORDER BY without LIMIT
        if sql_upper.contains("ORDER BY") && !sql_upper.contains("LIMIT") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "ORDER BY without LIMIT",
                "Ordering all rows can be expensive. Consider adding a LIMIT.",
                "Add .limit(100) to restrict result set.",
            ));
        }

        // Check for NOT IN
        if sql_upper.contains("NOT IN") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "NOT IN detected",
                "NOT IN may have unexpected NULL handling. Consider using NOT EXISTS.",
                "Use .where_not_exists(subquery) for more predictable behavior.",
            ));
        }

        // Check for functions in WHERE
        let function_patterns = ["LOWER(", "UPPER(", "DATE(", "YEAR(", "MONTH("];
        for pattern in function_patterns {
            if sql_upper.contains(pattern) {
                suggestions.push(QuerySuggestion::new(
                    SuggestionLevel::Warning,
                    "Function in WHERE clause",
                    "Functions in WHERE prevent index usage. Store computed values or use expression indexes.",
                    "Create a computed column or expression index."
                ));
                break;
            }
        }

        // Check for implicit type conversion
        if sql_upper.contains("= '") && (sql_upper.contains("_id =") || sql_upper.contains("id ="))
        {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "Possible type mismatch",
                "Comparing numeric ID with string may cause implicit conversion.",
                "Ensure parameter types match column types.",
            ));
        }

        suggestions
    }

    /// Classify query shape using a rough score for joins, subqueries, and aggregations.
    pub fn estimate_complexity(sql: &str) -> QueryComplexity {
        let sql_upper = sql.to_uppercase();
        let mut score = 0;

        // Base complexity by operation
        if sql_upper.starts_with("SELECT") {
            score += 1;
        } else if sql_upper.starts_with("INSERT") {
            score += 2;
        } else if sql_upper.starts_with("UPDATE") || sql_upper.starts_with("DELETE") {
            score += 3;
        }

        // Add complexity for joins
        score += sql_upper.matches("JOIN").count() * 2;

        // Add complexity for subqueries
        score += sql_upper.matches("SELECT").count().saturating_sub(1) * 3;

        // Add complexity for aggregations
        let agg_functions = ["COUNT(", "SUM(", "AVG(", "MAX(", "MIN(", "GROUP BY"];
        for func in agg_functions {
            if sql_upper.contains(func) {
                score += 1;
            }
        }

        // Add complexity for ORDER BY
        if sql_upper.contains("ORDER BY") {
            score += 1;
        }

        // Add complexity for DISTINCT
        if sql_upper.contains("DISTINCT") {
            score += 1;
        }

        QueryComplexity::from_score(score)
    }
}

/// Severity used by query-analysis suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionLevel {
    /// Informational observation.
    Info,
    /// Warning about likely performance cost.
    Warning,
    /// High-risk issue that should be addressed first.
    Critical,
}

impl SuggestionLevel {
    /// Display marker for formatted suggestions.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Info => "ℹ️",
            Self::Warning => "⚠️",
            Self::Critical => "🚨",
        }
    }

    /// Stable uppercase label for logs and reports.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Critical => "CRITICAL",
        }
    }
}

/// One query-analysis suggestion.
#[derive(Debug, Clone)]
pub struct QuerySuggestion {
    /// Severity bucket.
    pub level: SuggestionLevel,
    /// Short summary.
    pub title: String,
    /// Explanation of why the suggestion was emitted.
    pub explanation: String,
    /// Suggested next step.
    pub suggestion: String,
}

impl QuerySuggestion {
    /// Build one analyzer suggestion.
    pub fn new(
        level: SuggestionLevel,
        title: impl Into<String>,
        explanation: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            level,
            title: title.into(),
            explanation: explanation.into(),
            suggestion: suggestion.into(),
        }
    }
}

impl fmt::Display for QuerySuggestion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} [{}] {}",
            self.level.emoji(),
            self.level.label(),
            self.title
        )?;
        writeln!(f, "   {}", self.explanation)?;
        write!(f, "   💡 {}", self.suggestion)
    }
}

/// Rough complexity bucket for rendered SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryComplexity {
    /// Single-table or otherwise low-complexity query.
    Simple,
    /// Moderate complexity with some joins or conditions.
    Moderate,
    /// Complex query with joins, subqueries, or heavier aggregation.
    Complex,
    /// Very complex query shape.
    VeryComplex,
}

impl QueryComplexity {
    fn from_score(score: usize) -> Self {
        match score {
            0..=2 => Self::Simple,
            3..=5 => Self::Moderate,
            6..=10 => Self::Complex,
            _ => Self::VeryComplex,
        }
    }

    /// Human-readable summary of the complexity bucket.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Simple => "Simple query, should be fast",
            Self::Moderate => "Moderate complexity, ensure proper indexes",
            Self::Complex => "Complex query, may benefit from optimization",
            Self::VeryComplex => "Very complex query, review for N+1 issues and consider splitting",
        }
    }
}

impl fmt::Display for QueryComplexity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stars = match self {
            Self::Simple => "★☆☆☆",
            Self::Moderate => "★★☆☆",
            Self::Complex => "★★★☆",
            Self::VeryComplex => "★★★★",
        };
        write!(f, "{} {}", stars, self.description())
    }
}

// Helper functions
fn detect_operation(sql: &str) -> String {
    let sql_upper = sql.trim().to_uppercase();
    if sql_upper.starts_with("SELECT") {
        "SELECT".to_string()
    } else if sql_upper.starts_with("INSERT") {
        "INSERT".to_string()
    } else if sql_upper.starts_with("UPDATE") {
        "UPDATE".to_string()
    } else if sql_upper.starts_with("DELETE") {
        "DELETE".to_string()
    } else {
        "OTHER".to_string()
    }
}

fn textwrap_simple(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

#[cfg(test)]
#[path = "testing/profiling_tests.rs"]
mod tests;
