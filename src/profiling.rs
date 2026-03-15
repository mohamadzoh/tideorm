//! Performance Profiling for TideORM
//!
//! This module provides performance profiling, benchmarking, and optimization tools
//! for monitoring and improving query performance.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! use tideorm::profiling::{Profiler, ProfileReport};
//!
//! // Start profiling
//! let profiler = Profiler::start();
//!
//! // Execute queries
//! let users = User::all().await?;
//! let posts = Post::where_eq("published", true).get().await?;
//!
//! // Get profiling report
//! let report = profiler.stop();
//! println!("{}", report);
//! ```
//!
//! # Features
//!
//! - **Query Timing**: Measure individual query execution times
//! - **Slow Query Detection**: Automatically identify queries exceeding threshold
//! - **Query Analysis**: Get insights on query patterns and optimization opportunities
//! - **Memory Tracking**: Monitor memory usage during query execution (optional)
//! - **Connection Pool Stats**: Monitor pool utilization
//!
//! # Performance Tips
//!
//! The profiler can suggest optimizations:
//!
//! ```rust,ignore
//! let tips = Profiler::analyze_query("SELECT * FROM users WHERE email = 'test'");
//! for tip in tips {
//!     println!("💡 {}", tip);
//! }
//! // Output:
//! // 💡 Consider adding an index on 'email' column for faster lookups
//! // 💡 Use User::find_by_email() instead of raw WHERE for type safety
//! ```

use parking_lot::RwLock;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Performance profiler for tracking query execution
pub struct Profiler {
    start_time: Instant,
    queries: Vec<ProfiledQuery>,
    is_active: bool,
}

/// A profiled query with timing and metadata
#[derive(Debug, Clone)]
pub struct ProfiledQuery {
    /// The SQL query
    pub sql: String,
    /// Table involved
    pub table: Option<String>,
    /// Execution duration
    pub duration: Duration,
    /// Number of rows affected/returned
    pub rows: Option<u64>,
    /// Whether query was from cache
    pub cached: bool,
    /// Operation type
    pub operation: String,
    /// Timestamp
    pub timestamp: SystemTime,
}

impl ProfiledQuery {
    /// Create a new profiled query
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

    /// Set table name
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Set row count
    pub fn with_rows(mut self, rows: u64) -> Self {
        self.rows = Some(rows);
        self
    }

    /// Mark as cached
    pub fn cached(mut self) -> Self {
        self.cached = true;
        self
    }
}

impl Profiler {
    /// Start a new profiling session
    pub fn start() -> Self {
        Self {
            start_time: Instant::now(),
            queries: Vec::new(),
            is_active: true,
        }
    }

    /// Record a query execution
    pub fn record(&mut self, sql: impl Into<String>, duration: Duration) {
        if self.is_active {
            self.queries.push(ProfiledQuery::new(sql, duration));
        }
    }

    /// Record a query with full details
    pub fn record_full(&mut self, query: ProfiledQuery) {
        if self.is_active {
            self.queries.push(query);
        }
    }

    /// Stop profiling and generate report
    pub fn stop(mut self) -> ProfileReport {
        self.is_active = false;
        let total_duration = self.start_time.elapsed();
        ProfileReport::from_queries(self.queries, total_duration)
    }

    /// Get current query count
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Get elapsed time since start
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// Performance report generated from profiling session
#[derive(Debug, Clone)]
pub struct ProfileReport {
    /// Total profiling duration
    pub total_duration: Duration,
    /// Total time spent in queries
    pub query_duration: Duration,
    /// All profiled queries
    pub queries: Vec<ProfiledQuery>,
    /// Query count by operation type
    pub operations: HashMap<String, u64>,
    /// Slowest queries
    pub slowest: Vec<ProfiledQuery>,
    /// Query count by table
    pub tables: HashMap<String, u64>,
}

impl ProfileReport {
    /// Create report from queries
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

    /// Get total number of queries
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }

    /// Get average query time
    pub fn avg_query_time(&self) -> Duration {
        if self.queries.is_empty() {
            Duration::ZERO
        } else {
            self.query_duration / self.queries.len() as u32
        }
    }

    /// Get percentage of time spent in queries
    pub fn query_time_percentage(&self) -> f64 {
        if self.total_duration.as_nanos() == 0 {
            0.0
        } else {
            (self.query_duration.as_nanos() as f64 / self.total_duration.as_nanos() as f64) * 100.0
        }
    }

    /// Get queries slower than threshold
    pub fn queries_slower_than(&self, threshold: Duration) -> Vec<&ProfiledQuery> {
        self.queries
            .iter()
            .filter(|q| q.duration >= threshold)
            .collect()
    }

    /// Get optimization suggestions
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

/// Global profiling statistics
static GLOBAL_QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static GLOBAL_TOTAL_TIME_NS: AtomicU64 = AtomicU64::new(0);
static GLOBAL_SLOW_COUNT: AtomicU64 = AtomicU64::new(0);
static GLOBAL_PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

static GLOBAL_SLOW_THRESHOLD_MS: RwLock<u64> = RwLock::new(100);

/// Global profiling utilities
pub struct GlobalProfiler;

impl GlobalProfiler {
    /// Enable global profiling
    pub fn enable() {
        GLOBAL_PROFILING_ENABLED.store(true, Ordering::SeqCst);
    }

    /// Disable global profiling
    pub fn disable() {
        GLOBAL_PROFILING_ENABLED.store(false, Ordering::SeqCst);
    }

    /// Check if global profiling is enabled
    pub fn is_enabled() -> bool {
        GLOBAL_PROFILING_ENABLED.load(Ordering::SeqCst)
    }

    /// Record a query execution globally
    pub fn record(duration: Duration) {
        if Self::is_enabled() {
            GLOBAL_QUERY_COUNT.fetch_add(1, Ordering::SeqCst);
            GLOBAL_TOTAL_TIME_NS.fetch_add(duration.as_nanos() as u64, Ordering::SeqCst);

            let threshold_ms = *GLOBAL_SLOW_THRESHOLD_MS.read();
            if duration.as_millis() as u64 >= threshold_ms {
                GLOBAL_SLOW_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Get global statistics
    pub fn stats() -> GlobalStats {
        GlobalStats {
            total_queries: GLOBAL_QUERY_COUNT.load(Ordering::SeqCst),
            total_time_ns: GLOBAL_TOTAL_TIME_NS.load(Ordering::SeqCst),
            slow_queries: GLOBAL_SLOW_COUNT.load(Ordering::SeqCst),
            slow_threshold_ms: *GLOBAL_SLOW_THRESHOLD_MS.read(),
        }
    }

    /// Reset global statistics
    pub fn reset() {
        GLOBAL_QUERY_COUNT.store(0, Ordering::SeqCst);
        GLOBAL_TOTAL_TIME_NS.store(0, Ordering::SeqCst);
        GLOBAL_SLOW_COUNT.store(0, Ordering::SeqCst);
    }

    /// Set slow query threshold
    pub fn set_slow_threshold(ms: u64) {
        *GLOBAL_SLOW_THRESHOLD_MS.write() = ms;
    }
}

/// Global profiling statistics
#[derive(Debug, Clone, Copy)]
pub struct GlobalStats {
    /// Total queries executed
    pub total_queries: u64,
    /// Total time in nanoseconds
    pub total_time_ns: u64,
    /// Number of slow queries
    pub slow_queries: u64,
    /// Slow query threshold in milliseconds
    pub slow_threshold_ms: u64,
}

impl GlobalStats {
    /// Get total time as Duration
    pub fn total_time(&self) -> Duration {
        Duration::from_nanos(self.total_time_ns)
    }

    /// Get average query time
    pub fn avg_query_time(&self) -> Duration {
        if self.total_queries == 0 {
            Duration::ZERO
        } else {
            Duration::from_nanos(self.total_time_ns / self.total_queries)
        }
    }

    /// Get slow query percentage
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

/// Query analyzer for optimization suggestions
pub struct QueryAnalyzer;

impl QueryAnalyzer {
    /// Analyze a query and return optimization suggestions
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

    /// Estimate query complexity
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

/// Suggestion severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionLevel {
    /// Informational suggestion
    Info,
    /// Warning that may impact performance
    Warning,
    /// Critical issue that should be addressed
    Critical,
}

impl SuggestionLevel {
    /// Get emoji for display
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Info => "ℹ️",
            Self::Warning => "⚠️",
            Self::Critical => "🚨",
        }
    }

    /// Get label
    pub fn label(&self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Critical => "CRITICAL",
        }
    }
}

/// A query optimization suggestion
#[derive(Debug, Clone)]
pub struct QuerySuggestion {
    /// Severity level
    pub level: SuggestionLevel,
    /// Short title
    pub title: String,
    /// Detailed explanation
    pub explanation: String,
    /// Suggested fix
    pub suggestion: String,
}

impl QuerySuggestion {
    /// Create a new suggestion
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

/// Query complexity estimate
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryComplexity {
    /// Simple query (single table, no joins)
    Simple,
    /// Moderate complexity (few joins or conditions)
    Moderate,
    /// Complex query (multiple joins, subqueries)
    Complex,
    /// Very complex query
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

    /// Get description
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
mod tests {
    use super::*;

    #[test]
    fn test_query_analyzer() {
        let suggestions = QueryAnalyzer::analyze("SELECT * FROM users");
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.title.contains("SELECT *")));
    }

    #[test]
    fn test_query_complexity() {
        let simple = QueryAnalyzer::estimate_complexity("SELECT id FROM users WHERE id = 1");
        assert_eq!(simple, QueryComplexity::Simple);

        let complex = QueryAnalyzer::estimate_complexity(
            "SELECT u.*, p.* FROM users u 
             JOIN posts p ON p.user_id = u.id 
             JOIN comments c ON c.post_id = p.id
             WHERE u.active = true
             ORDER BY p.created_at DESC",
        );
        assert!(matches!(
            complex,
            QueryComplexity::Complex | QueryComplexity::VeryComplex
        ));
    }

    #[test]
    fn test_profiler() {
        let mut profiler = Profiler::start();
        profiler.record("SELECT 1", Duration::from_millis(10));
        profiler.record("SELECT 2", Duration::from_millis(20));

        let report = profiler.stop();
        assert_eq!(report.query_count(), 2);
    }

    #[test]
    fn test_global_stats() {
        GlobalProfiler::enable();
        GlobalProfiler::reset();

        GlobalProfiler::record(Duration::from_millis(50));
        GlobalProfiler::record(Duration::from_millis(150)); // slow

        let stats = GlobalProfiler::stats();
        assert_eq!(stats.total_queries, 2);
        assert_eq!(stats.slow_queries, 1);

        GlobalProfiler::disable();
    }

    #[test]
    fn test_missing_where_detection() {
        let suggestions = QueryAnalyzer::analyze("DELETE FROM users");
        assert!(
            suggestions
                .iter()
                .any(|s| s.level == SuggestionLevel::Critical)
        );
    }
}
