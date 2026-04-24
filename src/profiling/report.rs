use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant, SystemTime};

use super::utils::{detect_operation, textwrap_simple};

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
        let query_duration: Duration = queries.iter().map(|query| query.duration).sum();

        let mut operations: HashMap<String, u64> = HashMap::new();
        let mut tables: HashMap<String, u64> = HashMap::new();

        for query in &queries {
            *operations.entry(query.operation.clone()).or_insert(0) += 1;
            if let Some(ref table) = query.table {
                *tables.entry(table.clone()).or_insert(0) += 1;
            }
        }

        let mut slowest: Vec<ProfiledQuery> = queries.clone();
        slowest.sort_by_key(|query| Reverse(query.duration));
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
            .filter(|query| query.duration >= threshold)
            .collect()
    }

    /// Return simple heuristics that highlight likely hotspots in the report.
    pub fn suggestions(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

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

        let slow_count = self
            .queries
            .iter()
            .filter(|query| query.duration > Duration::from_millis(100))
            .count();

        if slow_count > 0 {
            suggestions.push(format!(
                "{} slow queries detected (>100ms). Review these queries and consider adding indexes.",
                slow_count
            ));
        }

        let select_star = self
            .queries
            .iter()
            .filter(|query| query.sql.contains("SELECT *") || query.sql.contains("select *"))
            .count();

        if select_star > 5 {
            suggestions.push(
                "Multiple SELECT * queries detected. Use `.select([\"col1\", \"col2\"])` to fetch only needed columns.".to_string(),
            );
        }

        if self.query_time_percentage() > 50.0 {
            suggestions.push(
                "More than 50% of time spent in database queries. Consider caching frequently accessed data.".to_string(),
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

        writeln!(
            f,
            "║ Operations:                                               ║"
        )?;
        for (operation, count) in &self.operations {
            writeln!(
                f,
                "║   {:10}: {:>6}                                       ║",
                operation, count
            )?;
        }

        if !self.slowest.is_empty() {
            writeln!(
                f,
                "╠═══════════════════════════════════════════════════════════╣"
            )?;
            writeln!(
                f,
                "║ Slowest Queries:                                          ║"
            )?;
            for (index, query) in self.slowest.iter().take(5).enumerate() {
                let sql_preview: String = query.sql.chars().take(40).collect();
                writeln!(
                    f,
                    "║ {}. {:>6}ms  {}...                                         ║",
                    index + 1,
                    query.duration.as_millis(),
                    sql_preview.replace('\n', " ")
                )?;
            }
        }

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
