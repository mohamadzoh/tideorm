/// Extra rendered context attached to query-oriented errors.
#[derive(Debug, Clone, Default)]
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
        let parts: Vec<String> = self
            .entries()
            .into_iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect();
        write!(f, "{}", parts.join(", "))
    }
}

impl ErrorContext {
    /// The fields that are set, as `(field name, rendered value)`, in the
    /// order every rendering lists them.
    pub(crate) fn entries(&self) -> Vec<(&'static str, String)> {
        [
            ("table", self.table.clone()),
            ("column", self.column.clone()),
            (
                "conditions",
                (!self.conditions.is_empty()).then(|| self.conditions.join(" | ")),
            ),
            ("operator_chain", self.operator_chain.clone()),
            ("query", self.query.clone()),
        ]
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
        .collect()
    }

    /// Start building extra table, column, and query details for an error.
    pub fn new() -> Self {
        Self::default()
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
