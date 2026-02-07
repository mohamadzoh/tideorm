//! Query Logging and Debugging
//!
//! This module provides comprehensive logging and debugging capabilities for TideORM queries,
//! including query logging, timing information, slow query detection, and debug output.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use tideorm::prelude::*;
//! use tideorm::logging::{QueryLogger, LogLevel};
//!
//! // Enable query logging globally
//! QueryLogger::global()
//!     .set_level(LogLevel::Debug)
//!     .enable_timing(true)
//!     .set_slow_query_threshold_ms(100)
//!     .enable();
//!
//! // All queries are now logged
//! let users = User::query()
//!     .where_eq("active", true)
//!     .get()
//!     .await?;
//! // Output: [TIDE] SELECT * FROM users WHERE active = true (took 12ms)
//! ```
//!
//! # Log Levels
//!
//! | Level | Output |
//! |-------|--------|
//! | `Off` | No logging |
//! | `Error` | Only errors |
//! | `Warn` | Errors and slow queries |
//! | `Info` | Errors, slow queries, and query summaries |
//! | `Debug` | All queries with timing |
//! | `Trace` | All queries with parameters and execution plan hints |
//!
//! # Environment Variables
//!
//! - `TIDE_LOG_QUERIES=true` - Enable basic query logging
//! - `TIDE_LOG_LEVEL=debug` - Set log level (off, error, warn, info, debug, trace)
//! - `TIDE_SLOW_QUERY_MS=100` - Slow query threshold in milliseconds
//!
//! # Query Debugging
//!
//! ```rust,ignore
//! // Debug a specific query without executing
//! let debug_info = User::query()
//!     .where_eq("email", "test@example.com")
//!     .where_gt("age", 18)
//!     .debug();
//!
//! println!("{}", debug_info);
//! // Output:
//! // ═══════════════════════════════════════════════════
//! // TIDEORM QUERY DEBUG
//! // ═══════════════════════════════════════════════════
//! // Table: users
//! // Operation: SELECT
//! // Conditions:
//! //   - email = 'test@example.com'
//! //   - age > 18
//! // SQL: SELECT * FROM users WHERE email = $1 AND age > $2
//! // Params: ["test@example.com", 18]
//! // ═══════════════════════════════════════════════════
//! ```

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Log level for query logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Default)]
pub enum LogLevel {
    /// No logging
    #[default]
    Off = 0,
    /// Only errors
    Error = 1,
    /// Errors and slow queries
    Warn = 2,
    /// Errors, slow queries, and query summaries
    Info = 3,
    /// All queries with timing
    Debug = 4,
    /// All queries with parameters and execution plan hints
    Trace = 5,
}


impl LogLevel {
    /// Parse log level from string
    pub fn parse_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "off" | "none" | "0" => Self::Off,
            "error" | "1" => Self::Error,
            "warn" | "warning" | "2" => Self::Warn,
            "info" | "3" => Self::Info,
            "debug" | "4" => Self::Debug,
            "trace" | "all" | "5" => Self::Trace,
            _ => Self::Off,
        }
    }
    
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Query operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryOperation {
    /// SELECT query
    Select,
    /// INSERT query
    Insert,
    /// UPDATE query
    Update,
    /// DELETE query
    Delete,
    /// Raw SQL query
    Raw,
    /// Transaction operation
    Transaction,
    /// Unknown operation
    Unknown,
}

impl QueryOperation {
    /// Detect operation from SQL string
    pub fn from_sql(sql: &str) -> Self {
        let sql_upper = sql.trim().to_uppercase();
        if sql_upper.starts_with("SELECT") {
            Self::Select
        } else if sql_upper.starts_with("INSERT") {
            Self::Insert
        } else if sql_upper.starts_with("UPDATE") {
            Self::Update
        } else if sql_upper.starts_with("DELETE") {
            Self::Delete
        } else if sql_upper.starts_with("BEGIN") || sql_upper.starts_with("COMMIT") || sql_upper.starts_with("ROLLBACK") {
            Self::Transaction
        } else {
            Self::Unknown
        }
    }
    
    /// Get operation name
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Select => "SELECT",
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
            Self::Raw => "RAW",
            Self::Transaction => "TRANSACTION",
            Self::Unknown => "UNKNOWN",
        }
    }
}

impl fmt::Display for QueryOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Query log entry
#[derive(Debug, Clone)]
pub struct QueryLogEntry {
    /// The SQL query string
    pub sql: String,
    /// Query parameters (if available)
    pub params: Vec<String>,
    /// Operation type
    pub operation: QueryOperation,
    /// Table name (if known)
    pub table: Option<String>,
    /// Execution duration
    pub duration: Option<Duration>,
    /// Number of rows affected/returned
    pub rows: Option<u64>,
    /// Whether the query was successful
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Timestamp when query started
    pub timestamp: std::time::SystemTime,
}

impl QueryLogEntry {
    /// Create a new query log entry
    pub fn new(sql: impl Into<String>) -> Self {
        let sql = sql.into();
        let operation = QueryOperation::from_sql(&sql);
        Self {
            sql,
            params: Vec::new(),
            operation,
            table: None,
            duration: None,
            rows: None,
            success: true,
            error: None,
            timestamp: std::time::SystemTime::now(),
        }
    }
    
    /// Set query parameters
    pub fn with_params(mut self, params: Vec<String>) -> Self {
        self.params = params;
        self
    }
    
    /// Set table name
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }
    
    /// Set execution duration
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
    
    /// Set number of rows
    pub fn with_rows(mut self, rows: u64) -> Self {
        self.rows = Some(rows);
        self
    }
    
    /// Mark as failed with error
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self
    }
    
    /// Check if this is a slow query
    pub fn is_slow(&self, threshold_ms: u64) -> bool {
        self.duration
            .map(|d| d.as_millis() as u64 >= threshold_ms)
            .unwrap_or(false)
    }
    
    /// Format for console output
    pub fn format_console(&self) -> String {
        let mut output = format!("[TIDE][{}]", self.operation);
        
        if let Some(ref table) = self.table {
            output.push_str(&format!(" {}", table));
        }
        
        if let Some(duration) = self.duration {
            output.push_str(&format!(" ({}ms)", duration.as_millis()));
        }
        
        if let Some(rows) = self.rows {
            output.push_str(&format!(" [{} rows]", rows));
        }
        
        if !self.success {
            output.push_str(" FAILED");
            if let Some(ref err) = self.error {
                output.push_str(&format!(": {}", err));
            }
        }
        
        output.push_str(&format!("\n  SQL: {}", self.sql));
        
        if !self.params.is_empty() {
            output.push_str(&format!("\n  Params: {:?}", self.params));
        }
        
        output
    }
}

/// Query timer for measuring execution time
pub struct QueryTimer {
    start: Instant,
    sql: String,
    table: Option<String>,
}

impl QueryTimer {
    /// Start a new query timer
    pub fn start(sql: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            sql: sql.into(),
            table: None,
        }
    }
    
    /// Set table name for the query
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }
    
    /// Stop the timer and return duration
    pub fn stop(&self) -> Duration {
        self.start.elapsed()
    }
    
    /// Stop and create a log entry
    pub fn finish(self) -> QueryLogEntry {
        let duration = self.start.elapsed();
        let mut entry = QueryLogEntry::new(self.sql).with_duration(duration);
        if let Some(table) = self.table {
            entry = entry.with_table(table);
        }
        entry
    }
    
    /// Stop, create entry with row count
    pub fn finish_with_rows(self, rows: u64) -> QueryLogEntry {
        self.finish().with_rows(rows)
    }
    
    /// Stop, create entry with error
    pub fn finish_with_error(self, error: impl Into<String>) -> QueryLogEntry {
        self.finish().with_error(error)
    }
}

/// Global query logger configuration
static LOGGER_ENABLED: AtomicBool = AtomicBool::new(false);
static LOGGER_TIMING: AtomicBool = AtomicBool::new(true);
static SLOW_QUERY_THRESHOLD_MS: AtomicU64 = AtomicU64::new(100);
static QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static SLOW_QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static TOTAL_QUERY_TIME_MS: AtomicU64 = AtomicU64::new(0);

lazy_static::lazy_static! {
    static ref LOG_LEVEL: RwLock<LogLevel> = RwLock::new(LogLevel::Off);
    static ref QUERY_HISTORY: RwLock<Vec<QueryLogEntry>> = RwLock::new(Vec::new());
    static ref HISTORY_LIMIT: RwLock<usize> = RwLock::new(100);
}

/// Query logger for debugging and performance monitoring
pub struct QueryLogger;

impl QueryLogger {
    /// Get the global query logger instance
    pub fn global() -> QueryLoggerBuilder {
        QueryLoggerBuilder::new()
    }
    
    /// Enable logging (convenience method)
    pub fn enable() {
        LOGGER_ENABLED.store(true, Ordering::SeqCst);
    }
    
    /// Disable logging (convenience method)
    pub fn disable() {
        LOGGER_ENABLED.store(false, Ordering::SeqCst);
    }
    
    /// Check if logging is enabled
    pub fn is_enabled() -> bool {
        LOGGER_ENABLED.load(Ordering::SeqCst)
    }
    
    /// Get current log level
    pub fn level() -> LogLevel {
        *LOG_LEVEL.read().unwrap()
    }
    
    /// Set log level
    pub fn set_level(level: LogLevel) {
        *LOG_LEVEL.write().unwrap() = level;
    }
    
    /// Log a query entry
    pub fn log(entry: QueryLogEntry) {
        if !Self::is_enabled() {
            return;
        }
        
        let level = Self::level();
        if level == LogLevel::Off {
            return;
        }
        
        // Update statistics
        QUERY_COUNT.fetch_add(1, Ordering::SeqCst);
        if let Some(duration) = entry.duration {
            TOTAL_QUERY_TIME_MS.fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
        }
        
        let threshold = SLOW_QUERY_THRESHOLD_MS.load(Ordering::SeqCst);
        let is_slow = entry.is_slow(threshold);
        if is_slow {
            SLOW_QUERY_COUNT.fetch_add(1, Ordering::SeqCst);
        }
        
        // Store in history
        {
            let mut history = QUERY_HISTORY.write().unwrap();
            let limit = *HISTORY_LIMIT.read().unwrap();
            if history.len() >= limit {
                history.remove(0);
            }
            history.push(entry.clone());
        }
        
        // Determine if we should output
        let should_log = match level {
            LogLevel::Off => false,
            LogLevel::Error => !entry.success,
            LogLevel::Warn => !entry.success || is_slow,
            LogLevel::Info => !entry.success || is_slow,
            LogLevel::Debug => true,
            LogLevel::Trace => true,
        };
        
        if should_log {
            let output = if level == LogLevel::Trace {
                entry.format_console()
            } else if level >= LogLevel::Debug {
                format_debug(&entry)
            } else if is_slow {
                format_slow(&entry, threshold)
            } else {
                format_error(&entry)
            };
            
            eprintln!("{}", output);
        }
    }
    
    /// Log a query with timing
    pub fn log_timed(sql: impl Into<String>, duration: Duration) {
        if !Self::is_enabled() {
            return;
        }
        let entry = QueryLogEntry::new(sql).with_duration(duration);
        Self::log(entry);
    }
    
    /// Log a query error
    pub fn log_error(sql: impl Into<String>, error: impl Into<String>) {
        if !Self::is_enabled() {
            return;
        }
        let entry = QueryLogEntry::new(sql).with_error(error);
        Self::log(entry);
    }
    
    /// Get query statistics
    pub fn stats() -> QueryStats {
        QueryStats {
            total_queries: QUERY_COUNT.load(Ordering::SeqCst),
            slow_queries: SLOW_QUERY_COUNT.load(Ordering::SeqCst),
            total_time_ms: TOTAL_QUERY_TIME_MS.load(Ordering::SeqCst),
            threshold_ms: SLOW_QUERY_THRESHOLD_MS.load(Ordering::SeqCst),
        }
    }
    
    /// Reset query statistics
    pub fn reset_stats() {
        QUERY_COUNT.store(0, Ordering::SeqCst);
        SLOW_QUERY_COUNT.store(0, Ordering::SeqCst);
        TOTAL_QUERY_TIME_MS.store(0, Ordering::SeqCst);
    }
    
    /// Get query history
    pub fn history() -> Vec<QueryLogEntry> {
        QUERY_HISTORY.read().unwrap().clone()
    }
    
    /// Clear query history
    pub fn clear_history() {
        QUERY_HISTORY.write().unwrap().clear();
    }
    
    /// Get slow queries from history
    pub fn slow_queries() -> Vec<QueryLogEntry> {
        let threshold = SLOW_QUERY_THRESHOLD_MS.load(Ordering::SeqCst);
        QUERY_HISTORY
            .read()
            .unwrap()
            .iter()
            .filter(|e| e.is_slow(threshold))
            .cloned()
            .collect()
    }
    
    /// Initialize from environment variables
    pub fn init_from_env() {
        // TIDE_LOG_QUERIES
        if let Ok(val) = std::env::var("TIDE_LOG_QUERIES") {
            if val == "1" || val.to_lowercase() == "true" {
                LOGGER_ENABLED.store(true, Ordering::SeqCst);
            }
        }
        
        // TIDE_LOG_LEVEL
        if let Ok(val) = std::env::var("TIDE_LOG_LEVEL") {
            let level = LogLevel::parse_str(&val);
            *LOG_LEVEL.write().unwrap() = level;
            if level != LogLevel::Off {
                LOGGER_ENABLED.store(true, Ordering::SeqCst);
            }
        }
        
        // TIDE_SLOW_QUERY_MS
        if let Ok(val) = std::env::var("TIDE_SLOW_QUERY_MS") {
            if let Ok(ms) = val.parse::<u64>() {
                SLOW_QUERY_THRESHOLD_MS.store(ms, Ordering::SeqCst);
            }
        }
    }
}

/// Builder for configuring the query logger
pub struct QueryLoggerBuilder {
    level: Option<LogLevel>,
    timing: Option<bool>,
    threshold_ms: Option<u64>,
    history_limit: Option<usize>,
}

impl QueryLoggerBuilder {
    fn new() -> Self {
        Self {
            level: None,
            timing: None,
            threshold_ms: None,
            history_limit: None,
        }
    }
    
    /// Set the log level
    pub fn set_level(mut self, level: LogLevel) -> Self {
        self.level = Some(level);
        self
    }
    
    /// Enable or disable timing
    pub fn enable_timing(mut self, enable: bool) -> Self {
        self.timing = Some(enable);
        self
    }
    
    /// Set slow query threshold in milliseconds
    pub fn set_slow_query_threshold_ms(mut self, ms: u64) -> Self {
        self.threshold_ms = Some(ms);
        self
    }
    
    /// Set query history limit
    pub fn set_history_limit(mut self, limit: usize) -> Self {
        self.history_limit = Some(limit);
        self
    }
    
    /// Enable the logger with configured settings
    pub fn enable(self) {
        if let Some(level) = self.level {
            *LOG_LEVEL.write().unwrap() = level;
        }
        if let Some(timing) = self.timing {
            LOGGER_TIMING.store(timing, Ordering::SeqCst);
        }
        if let Some(ms) = self.threshold_ms {
            SLOW_QUERY_THRESHOLD_MS.store(ms, Ordering::SeqCst);
        }
        if let Some(limit) = self.history_limit {
            *HISTORY_LIMIT.write().unwrap() = limit;
        }
        LOGGER_ENABLED.store(true, Ordering::SeqCst);
    }
    
    /// Disable the logger
    pub fn disable(self) {
        LOGGER_ENABLED.store(false, Ordering::SeqCst);
    }
}

/// Query statistics
#[derive(Debug, Clone, Copy)]
pub struct QueryStats {
    /// Total number of queries executed
    pub total_queries: u64,
    /// Number of slow queries
    pub slow_queries: u64,
    /// Total time spent in queries (milliseconds)
    pub total_time_ms: u64,
    /// Slow query threshold (milliseconds)
    pub threshold_ms: u64,
}

impl QueryStats {
    /// Get average query time in milliseconds
    pub fn avg_query_time_ms(&self) -> f64 {
        if self.total_queries == 0 {
            0.0
        } else {
            self.total_time_ms as f64 / self.total_queries as f64
        }
    }
    
    /// Get percentage of slow queries
    pub fn slow_query_percentage(&self) -> f64 {
        if self.total_queries == 0 {
            0.0
        } else {
            (self.slow_queries as f64 / self.total_queries as f64) * 100.0
        }
    }
}

impl fmt::Display for QueryStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "═══════════════════════════════════════════════════")?;
        writeln!(f, "TIDEORM QUERY STATISTICS")?;
        writeln!(f, "═══════════════════════════════════════════════════")?;
        writeln!(f, "Total Queries:     {}", self.total_queries)?;
        writeln!(f, "Slow Queries:      {} ({:.1}%)", self.slow_queries, self.slow_query_percentage())?;
        writeln!(f, "Total Time:        {}ms", self.total_time_ms)?;
        writeln!(f, "Avg Query Time:    {:.2}ms", self.avg_query_time_ms())?;
        writeln!(f, "Slow Threshold:    {}ms", self.threshold_ms)?;
        write!(f, "═══════════════════════════════════════════════════")
    }
}

/// Debug information for a query builder
#[derive(Debug, Clone)]
pub struct QueryDebugInfo {
    /// Table name
    pub table: String,
    /// Operation type
    pub operation: QueryOperation,
    /// Conditions as strings
    pub conditions: Vec<String>,
    /// Order by clauses
    pub order_by: Vec<String>,
    /// Group by columns
    pub group_by: Vec<String>,
    /// Selected columns
    pub select: Vec<String>,
    /// Join clauses
    pub joins: Vec<String>,
    /// Limit value
    pub limit: Option<u64>,
    /// Offset value
    pub offset: Option<u64>,
    /// Generated SQL
    pub sql: String,
    /// Query parameters
    pub params: Vec<String>,
}

impl QueryDebugInfo {
    /// Create new debug info
    pub fn new(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            operation: QueryOperation::Select,
            conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            select: vec!["*".to_string()],
            joins: Vec::new(),
            limit: None,
            offset: None,
            sql: String::new(),
            params: Vec::new(),
        }
    }
    
    /// Set operation type
    pub fn with_operation(mut self, op: QueryOperation) -> Self {
        self.operation = op;
        self
    }
    
    /// Add a condition
    pub fn add_condition(&mut self, condition: impl Into<String>) {
        self.conditions.push(condition.into());
    }
    
    /// Add order by clause
    pub fn add_order_by(&mut self, order: impl Into<String>) {
        self.order_by.push(order.into());
    }
    
    /// Set SQL
    pub fn with_sql(mut self, sql: impl Into<String>) -> Self {
        self.sql = sql.into();
        self
    }
    
    /// Set params
    pub fn with_params(mut self, params: Vec<String>) -> Self {
        self.params = params;
        self
    }
}

impl fmt::Display for QueryDebugInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "═══════════════════════════════════════════════════")?;
        writeln!(f, "TIDEORM QUERY DEBUG")?;
        writeln!(f, "═══════════════════════════════════════════════════")?;
        writeln!(f, "Table:      {}", self.table)?;
        writeln!(f, "Operation:  {}", self.operation)?;
        
        if !self.select.is_empty() && self.select != vec!["*".to_string()] {
            writeln!(f, "Select:     {}", self.select.join(", "))?;
        }
        
        if !self.conditions.is_empty() {
            writeln!(f, "Conditions:")?;
            for cond in &self.conditions {
                writeln!(f, "  - {}", cond)?;
            }
        }
        
        if !self.joins.is_empty() {
            writeln!(f, "Joins:")?;
            for join in &self.joins {
                writeln!(f, "  - {}", join)?;
            }
        }
        
        if !self.order_by.is_empty() {
            writeln!(f, "Order By:   {}", self.order_by.join(", "))?;
        }
        
        if !self.group_by.is_empty() {
            writeln!(f, "Group By:   {}", self.group_by.join(", "))?;
        }
        
        if let Some(limit) = self.limit {
            write!(f, "Limit:      {}", limit)?;
            if let Some(offset) = self.offset {
                write!(f, " (offset: {})", offset)?;
            }
            writeln!(f)?;
        }
        
        if !self.sql.is_empty() {
            writeln!(f, "───────────────────────────────────────────────────")?;
            writeln!(f, "SQL: {}", self.sql)?;
        }
        
        if !self.params.is_empty() {
            writeln!(f, "Params: {:?}", self.params)?;
        }
        
        write!(f, "═══════════════════════════════════════════════════")
    }
}

// Helper formatting functions
fn format_debug(entry: &QueryLogEntry) -> String {
    let timing = entry.duration
        .map(|d| format!(" ({}ms)", d.as_millis()))
        .unwrap_or_default();
    
    format!("[TIDE][{}]{} {}", entry.operation, timing, entry.sql)
}

fn format_slow(entry: &QueryLogEntry, threshold: u64) -> String {
    let duration_ms = entry.duration
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    
    format!(
        "[TIDE][SLOW QUERY] {} ({}ms > {}ms threshold)\n  SQL: {}",
        entry.operation,
        duration_ms,
        threshold,
        entry.sql
    )
}

fn format_error(entry: &QueryLogEntry) -> String {
    let error = entry.error.as_deref().unwrap_or("Unknown error");
    format!("[TIDE][ERROR] {} failed: {}\n  SQL: {}", entry.operation, error, entry.sql)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_log_level_parsing() {
        assert_eq!(LogLevel::parse_str("debug"), LogLevel::Debug);
        assert_eq!(LogLevel::parse_str("DEBUG"), LogLevel::Debug);
        assert_eq!(LogLevel::parse_str("warn"), LogLevel::Warn);
        assert_eq!(LogLevel::parse_str("4"), LogLevel::Debug);
        assert_eq!(LogLevel::parse_str("invalid"), LogLevel::Off);
    }
    
    #[test]
    fn test_query_operation_detection() {
        assert_eq!(QueryOperation::from_sql("SELECT * FROM users"), QueryOperation::Select);
        assert_eq!(QueryOperation::from_sql("INSERT INTO users"), QueryOperation::Insert);
        assert_eq!(QueryOperation::from_sql("UPDATE users SET"), QueryOperation::Update);
        assert_eq!(QueryOperation::from_sql("DELETE FROM users"), QueryOperation::Delete);
        assert_eq!(QueryOperation::from_sql("BEGIN"), QueryOperation::Transaction);
    }
    
    #[test]
    fn test_query_log_entry() {
        let entry = QueryLogEntry::new("SELECT * FROM users")
            .with_table("users")
            .with_duration(Duration::from_millis(50))
            .with_rows(10);
        
        assert_eq!(entry.operation, QueryOperation::Select);
        assert_eq!(entry.table, Some("users".to_string()));
        assert_eq!(entry.rows, Some(10));
        assert!(entry.success);
    }
    
    #[test]
    fn test_slow_query_detection() {
        let fast_entry = QueryLogEntry::new("SELECT 1")
            .with_duration(Duration::from_millis(10));
        
        let slow_entry = QueryLogEntry::new("SELECT 1")
            .with_duration(Duration::from_millis(200));
        
        assert!(!fast_entry.is_slow(100));
        assert!(slow_entry.is_slow(100));
    }
    
    #[test]
    fn test_query_stats() {
        let stats = QueryStats {
            total_queries: 100,
            slow_queries: 5,
            total_time_ms: 500,
            threshold_ms: 100,
        };
        
        assert_eq!(stats.avg_query_time_ms(), 5.0);
        assert_eq!(stats.slow_query_percentage(), 5.0);
    }
    
    #[test]
    fn test_query_timer() {
        let timer = QueryTimer::start("SELECT * FROM users")
            .with_table("users");
        
        std::thread::sleep(Duration::from_millis(10));
        let entry = timer.finish_with_rows(5);
        
        assert!(entry.duration.unwrap() >= Duration::from_millis(10));
        assert_eq!(entry.rows, Some(5));
    }
}
