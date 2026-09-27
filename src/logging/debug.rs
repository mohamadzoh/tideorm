use std::fmt;

use super::entry::QueryOperation;

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
    /// Why running the query would fail before it reached the database, if
    /// it would.
    pub error: Option<String>,
}

impl QueryDebugInfo {
    /// Start building a debug snapshot for one query.
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
            error: None,
        }
    }

    /// Add one rendered condition line.
    pub fn add_condition(&mut self, condition: impl Into<String>) {
        self.conditions.push(condition.into());
    }

    /// Add one ORDER BY fragment.
    pub fn add_order_by(&mut self, order: impl Into<String>) {
        self.order_by.push(order.into());
    }

    /// Attach the rendered SQL or preview text.
    pub fn with_sql(mut self, sql: impl Into<String>) -> Self {
        self.sql = sql.into();
        self
    }
}

impl fmt::Display for QueryDebugInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DEBUG_PREVIEW_BANNER: &str =
            "-- DEBUG PREVIEW (not executable, values are approximate)";

        writeln!(f, "═══════════════════════════════════════════════════")?;
        writeln!(f, "TIDEORM QUERY DEBUG")?;
        writeln!(f, "═══════════════════════════════════════════════════")?;
        writeln!(f, "Table:      {}", self.table)?;
        writeln!(f, "Operation:  {}", self.operation)?;
        if let Some(error) = &self.error {
            writeln!(f, "Invalid:    {}", error)?;
        }

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
            if self.sql.starts_with(DEBUG_PREVIEW_BANNER) {
                writeln!(f, "SQL Preview (non-executable):")?;
            } else {
                writeln!(f, "SQL:")?;
            }

            for line in self.sql.lines() {
                writeln!(f, "  {}", line)?;
            }
        }

        if !self.params.is_empty() {
            writeln!(f, "Params: {:?}", self.params)?;
        }

        write!(f, "═══════════════════════════════════════════════════")
    }
}
