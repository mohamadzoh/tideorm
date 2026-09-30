use crate::internal::sql_lexer::{Kind, Token, significant};
use std::fmt;

/// Heuristic analyzer for rendered SQL strings.
pub struct QueryAnalyzer;

impl QueryAnalyzer {
    /// Run simple SQL heuristics against a rendered query string.
    pub fn analyze(sql: &str) -> Vec<QuerySuggestion> {
        let mut suggestions = Vec::new();
        let tokens: Vec<_> = significant(sql).collect();

        if has_sequence(&tokens, &["SELECT", "*"]) {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Avoid SELECT *",
                "Specify columns explicitly to reduce data transfer and improve performance.",
                "Change to: .select([\"id\", \"name\", \"email\"])",
            ));
        }

        let operation = crate::logging::QueryOperation::from_sql(sql);
        if matches!(
            operation,
            crate::logging::QueryOperation::Update | crate::logging::QueryOperation::Delete
        ) && !crate::internal::sql_lexer::top_words(sql)
            .iter()
            .any(|word| word.eq_ignore_ascii_case("where"))
        {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Critical,
                "Missing WHERE clause",
                "UPDATE/DELETE without WHERE will affect all rows!",
                "Add a WHERE condition: .where_eq(\"id\", value)",
            ));
        }

        if tokens.windows(2).any(|pair| {
            pair[0].kind == crate::internal::sql_lexer::Kind::Word
                && pair[0].text.eq_ignore_ascii_case("like")
                && pair[1].kind == crate::internal::sql_lexer::Kind::Literal
                && pair[1].text.starts_with("'%")
        }) {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Leading wildcard in LIKE",
                "LIKE '%pattern' cannot use indexes and will be slow on large tables.",
                "Consider using full-text search or restructure the query.",
            ));
        }

        if has_sequence(&tokens, &["OR"]) {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "OR conditions detected",
                "OR conditions may prevent index usage. Consider using UNION or restructuring.",
                "Use .where_in(\"column\", values) instead of multiple OR conditions.",
            ));
        }

        if has_sequence(&tokens, &["ORDER", "BY"]) && !has_sequence(&tokens, &["LIMIT"]) {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "ORDER BY without LIMIT",
                "Ordering all rows can be expensive. Consider adding a LIMIT.",
                "Add .limit(100) to restrict result set.",
            ));
        }

        if has_sequence(&tokens, &["NOT", "IN"]) {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Info,
                "NOT IN detected",
                "NOT IN may have unexpected NULL handling. Consider using NOT EXISTS.",
                "Use .where_not_exists(subquery) for more predictable behavior.",
            ));
        }

        if function_in_where(&tokens) {
            suggestions.push(QuerySuggestion::new(
                SuggestionLevel::Warning,
                "Function in WHERE clause",
                "Functions in WHERE prevent index usage. Store computed values or use expression indexes.",
                "Create a computed column or expression index.",
            ));
        }

        if tokens.windows(3).any(|p| {
            p[0].kind == crate::internal::sql_lexer::Kind::Word
                && p[0].text.to_ascii_uppercase().ends_with("ID")
                && p[1].text == "="
                && p[2].kind == crate::internal::sql_lexer::Kind::Literal
        }) {
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
        let tokens: Vec<_> = significant(sql).collect();
        let mut score = 0;

        score += match crate::logging::QueryOperation::from_sql(sql) {
            crate::logging::QueryOperation::Select => 1,
            crate::logging::QueryOperation::Insert => 2,
            crate::logging::QueryOperation::Update | crate::logging::QueryOperation::Delete => 3,
            _ => 0,
        };

        score += tokens
            .iter()
            .filter(|t| {
                t.kind == crate::internal::sql_lexer::Kind::Word
                    && t.text.eq_ignore_ascii_case("join")
            })
            .count()
            * 2;
        score += tokens
            .iter()
            .filter(|t| {
                t.kind == crate::internal::sql_lexer::Kind::Word
                    && t.text.eq_ignore_ascii_case("select")
            })
            .count()
            .saturating_sub(1)
            * 3;

        let agg_functions = [
            ["COUNT", "("],
            ["SUM", "("],
            ["AVG", "("],
            ["MAX", "("],
            ["MIN", "("],
            ["GROUP", "BY"],
        ];
        for func in agg_functions {
            if has_sequence(&tokens, &func) {
                score += 1;
            }
        }

        if has_sequence(&tokens, &["ORDER", "BY"]) {
            score += 1;
        }

        if has_sequence(&tokens, &["DISTINCT"]) {
            score += 1;
        }

        QueryComplexity::from_score(score)
    }
}

fn has_sequence(tokens: &[Token<'_>], words: &[&str]) -> bool {
    tokens.windows(words.len()).any(|window| {
        window.iter().zip(words).all(|(token, word)| {
            matches!(token.kind, Kind::Word | Kind::Symbol) && token.text.eq_ignore_ascii_case(word)
        })
    })
}

fn function_in_where(tokens: &[Token<'_>]) -> bool {
    let mut scopes = vec![false];
    for (index, token) in tokens.iter().enumerate() {
        if token.kind == Kind::Word {
            match token.text.to_ascii_uppercase().as_str() {
                "WHERE" => *scopes.last_mut().unwrap() = true,
                "SELECT" | "GROUP" | "ORDER" | "HAVING" | "LIMIT" | "RETURNING" | "UNION"
                | "EXCEPT" | "INTERSECT" => *scopes.last_mut().unwrap() = false,
                "LOWER" | "UPPER" | "DATE" | "YEAR" | "MONTH"
                    if *scopes.last().unwrap()
                        && tokens.get(index + 1).is_some_and(|next| next.text == "(") =>
                {
                    return true;
                }
                _ => {}
            }
        } else if token.text == "(" {
            scopes.push(*scopes.last().unwrap());
        } else if token.text == ")" && scopes.len() > 1 {
            scopes.pop();
        }
    }
    false
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
