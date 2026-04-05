use std::fmt;

/// Heuristic analyzer for rendered SQL strings.
pub struct QueryAnalyzer;

impl QueryAnalyzer {
    /// Run simple SQL heuristics against a rendered query string.
    pub fn analyze(sql: &str) -> Vec<QuerySuggestion> {
        let mut suggestions = Vec::new();
        let sql_upper = sql.to_uppercase();

        if sql_upper.contains("SELECT *") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Avoid SELECT *",
                "Specify columns explicitly to reduce data transfer and improve performance.",
                "Change to: .select([\"id\", \"name\", \"email\"])",
            ));
        }

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

        if sql_upper.contains("LIKE '%") || sql_upper.contains("LIKE '%") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Leading wildcard in LIKE",
                "LIKE '%pattern' cannot use indexes and will be slow on large tables.",
                "Consider using full-text search or restructure the query.",
            ));
        }

        if sql_upper.contains(" OR ") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "OR conditions detected",
                "OR conditions may prevent index usage. Consider using UNION or restructuring.",
                "Use .where_in(\"column\", values) instead of multiple OR conditions.",
            ));
        }

        if sql_upper.contains("ORDER BY") && !sql_upper.contains("LIMIT") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "ORDER BY without LIMIT",
                "Ordering all rows can be expensive. Consider adding a LIMIT.",
                "Add .limit(100) to restrict result set.",
            ));
        }

        if sql_upper.contains("NOT IN") {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "NOT IN detected",
                "NOT IN may have unexpected NULL handling. Consider using NOT EXISTS.",
                "Use .where_not_exists(subquery) for more predictable behavior.",
            ));
        }

        let function_patterns = ["LOWER(", "UPPER(", "DATE(", "YEAR(", "MONTH("];
        for pattern in function_patterns {
            if sql_upper.contains(pattern) {
                suggestions.push(QuerySuggestion::new(
                    SuggestionLevel::Warning,
                    "Function in WHERE clause",
                    "Functions in WHERE prevent index usage. Store computed values or use expression indexes.",
                    "Create a computed column or expression index.",
                ));
                break;
            }
        }

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

        if sql_upper.starts_with("SELECT") {
            score += 1;
        } else if sql_upper.starts_with("INSERT") {
            score += 2;
        } else if sql_upper.starts_with("UPDATE") || sql_upper.starts_with("DELETE") {
            score += 3;
        }

        score += sql_upper.matches("JOIN").count() * 2;
        score += sql_upper.matches("SELECT").count().saturating_sub(1) * 3;

        let agg_functions = ["COUNT(", "SUM(", "AVG(", "MAX(", "MIN(", "GROUP BY"];
        for func in agg_functions {
            if sql_upper.contains(func) {
                score += 1;
            }
        }

        if sql_upper.contains("ORDER BY") {
            score += 1;
        }

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
