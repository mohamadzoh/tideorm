use std::fmt;
use std::time::{Duration, Instant, SystemTime};

/// Log level for query logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
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
    /// Parse a log level from environment-style text.
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

    /// Return the stable uppercase label used in logs.
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
    /// Classify a rendered SQL statement by its leading keyword.
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
        } else if sql_upper.starts_with("BEGIN")
            || sql_upper.starts_with("COMMIT")
            || sql_upper.starts_with("ROLLBACK")
        {
            Self::Transaction
        } else {
            Self::Unknown
        }
    }

    /// Return the display label for this operation.
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
    pub timestamp: SystemTime,
}

impl QueryLogEntry {
    /// Start a log entry from raw SQL.
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
            timestamp: SystemTime::now(),
        }
    }

    /// Attach rendered parameter values.
    pub fn with_params(mut self, params: Vec<String>) -> Self {
        self.params = params;
        self
    }

    /// Attach the table name when it is known.
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Attach the measured execution time.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Attach affected or returned row count.
    pub fn with_rows(mut self, rows: u64) -> Self {
        self.rows = Some(rows);
        self
    }

    /// Mark the entry as failed and store the error message.
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self
    }

    /// True when the recorded duration meets the slow-query threshold.
    pub fn is_slow(&self, threshold_ms: u64) -> bool {
        self.duration
            .map(|d| d.as_millis() as u64 >= threshold_ms)
            .unwrap_or(false)
    }

    /// Render the entry in the multiline console format used by trace logging.
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
    /// Start timing one query.
    pub fn start(sql: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            sql: sql.into(),
            table: None,
        }
    }

    /// Attach the table name to the finished entry.
    pub fn with_table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Return elapsed time without building a log entry.
    pub fn stop(&self) -> Duration {
        self.start.elapsed()
    }

    /// Stop timing and build a successful log entry.
    pub fn finish(self) -> QueryLogEntry {
        let duration = self.start.elapsed();
        let mut entry = QueryLogEntry::new(self.sql).with_duration(duration);
        if let Some(table) = self.table {
            entry = entry.with_table(table);
        }
        entry
    }

    /// Stop timing and attach row count to the resulting entry.
    pub fn finish_with_rows(self, rows: u64) -> QueryLogEntry {
        self.finish().with_rows(rows)
    }

    /// Stop timing and build a failed log entry.
    pub fn finish_with_error(self, error: impl Into<String>) -> QueryLogEntry {
        self.finish().with_error(error)
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
    /// Average duration in milliseconds, or zero when nothing has run.
    pub fn avg_query_time_ms(&self) -> f64 {
        if self.total_queries == 0 {
            0.0
        } else {
            self.total_time_ms as f64 / self.total_queries as f64
        }
    }

    /// Percentage of recorded queries that counted as slow.
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
        writeln!(
            f,
            "Slow Queries:      {} ({:.1}%)",
            self.slow_queries,
            self.slow_query_percentage()
        )?;
        writeln!(f, "Total Time:        {}ms", self.total_time_ms)?;
        writeln!(f, "Avg Query Time:    {:.2}ms", self.avg_query_time_ms())?;
        writeln!(f, "Slow Threshold:    {}ms", self.threshold_ms)?;
        write!(f, "═══════════════════════════════════════════════════")
    }
}
