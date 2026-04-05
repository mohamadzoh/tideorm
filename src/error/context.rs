/// Extra rendered context attached to query-oriented errors.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Table name, if known.
    pub table: Option<String>,
    /// Column name, if known.
    pub column: Option<String>,
    /// Rendered conditions involved in the failure.
    pub conditions: Vec<String>,
    /// Logical operator chain for the rendered conditions.
    pub operator_chain: Option<String>,
    /// Rendered SQL query, if available.
    pub query: Option<String>,
}

impl std::fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(ref table) = self.table {
            parts.push(format!("table: {}", table));
        }
        if let Some(ref column) = self.column {
            parts.push(format!("column: {}", column));
        }
        if !self.conditions.is_empty() {
            parts.push(format!("conditions: {}", self.conditions.join(" | ")));
        }
        if let Some(ref operator_chain) = self.operator_chain {
            parts.push(format!("operator_chain: {}", operator_chain));
        }
        if let Some(ref query) = self.query {
            parts.push(format!("query: {}", query));
        }
        write!(f, "{}", parts.join(", "))
    }
}

impl ErrorContext {
    /// Start building extra table, column, and query details for an error.
    pub fn new() -> Self {
        Self {
            table: None,
            column: None,
            conditions: Vec::new(),
            operator_chain: None,
            query: None,
        }
    }

    /// Attach the table name involved in the failure.
    pub fn table(mut self, table: impl Into<String>) -> Self {
        self.table = Some(table.into());
        self
    }

    /// Attach the column name involved in the failure.
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }

    /// Add one rendered condition to the context.
    pub fn condition(mut self, condition: impl Into<String>) -> Self {
        self.conditions.push(condition.into());
        self
    }

    /// Replace the collected rendered conditions.
    pub fn conditions(mut self, conditions: Vec<String>) -> Self {
        self.conditions = conditions;
        self
    }

    /// Attach the rendered logical operator chain.
    pub fn operator_chain(mut self, operator_chain: impl Into<String>) -> Self {
        self.operator_chain = Some(operator_chain.into());
        self
    }

    /// Attach the rendered SQL query.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self::new()
    }
}
