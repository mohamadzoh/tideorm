use super::*;
use crate::internal::sql_builder::SqlBuilder;

// =============================================================================
// INDEX GENERATION HELPERS
// =============================================================================

/// Full-text index definition
#[derive(Debug, Clone)]
pub struct FullTextIndex {
    /// Index name
    pub name: String,
    /// Table name
    pub table: String,
    /// Columns to index
    pub columns: Vec<String>,
    /// Index configuration
    pub config: FullTextIndexConfig,
}

/// Configuration for full-text indexes
#[derive(Debug, Clone, Default)]
pub struct FullTextIndexConfig {
    /// Language configuration (PostgreSQL)
    pub language: Option<String>,
    /// Index type: GIN or GiST (PostgreSQL)
    pub pg_index_type: PgFullTextIndexType,
    /// Parser type (MySQL)
    pub mysql_parser: Option<String>,
}

/// PostgreSQL full-text index type
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PgFullTextIndexType {
    /// GIN index - faster lookups, slower updates
    #[default]
    GIN,
    /// GiST index - slower lookups, faster updates, supports ranking
    GiST,
}

impl FullTextIndex {
    /// Create a new full-text index
    pub fn new(name: impl Into<String>, table: impl Into<String>, columns: Vec<String>) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            columns,
            config: FullTextIndexConfig::default(),
        }
    }

    /// Set the language
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.config.language = Some(lang.into());
        self
    }

    /// Set PostgreSQL index type
    pub fn pg_index_type(mut self, index_type: PgFullTextIndexType) -> Self {
        self.config.pg_index_type = index_type;
        self
    }

    /// Generate CREATE INDEX statement for PostgreSQL
    pub fn to_postgres_sql(&self) -> String {
        let language = self.config.language.as_deref().unwrap_or("english");
        let index_type = match self.config.pg_index_type {
            PgFullTextIndexType::GIN => "GIN",
            PgFullTextIndexType::GiST => "GiST",
        };

        let mut params = Vec::new();
        let tsvector_expr = if self.columns.len() == 1 {
            SqlBuilder::new(DatabaseType::Postgres, &mut params)
                .raw("to_tsvector('")
                .raw(&escape_string(language))
                .raw("', COALESCE(")
                .ident(&self.columns[0])
                .raw(", ''))")
                .into_sql()
        } else {
            let mut builder = SqlBuilder::new(DatabaseType::Postgres, &mut params)
                .raw("to_tsvector('")
                .raw(&escape_string(language))
                .raw("', ");
            for (i, col) in self.columns.iter().enumerate() {
                if i > 0 {
                    builder = builder.raw(" || ' ' || ");
                }
                builder = builder.raw("COALESCE(").ident(col).raw(", '')");
            }
            builder.raw(")").into_sql()
        };

        SqlBuilder::new(DatabaseType::Postgres, &mut params)
            .raw("CREATE INDEX ")
            .ident(&self.name)
            .raw(" ON ")
            .ident(&self.table)
            .raw(" USING ")
            .raw(index_type)
            .raw(" ((")
            .raw(&tsvector_expr)
            .raw("))")
            .into_sql()
    }

    /// Generate CREATE FULLTEXT INDEX statement for MySQL
    pub fn to_mysql_sql(&self) -> String {
        let mut params = Vec::new();
        let mut builder = SqlBuilder::new(DatabaseType::MySQL, &mut params)
            .raw("CREATE FULLTEXT INDEX ")
            .ident(&self.name)
            .raw(" ON ")
            .ident(&self.table)
            .raw("(");
        for (i, col) in self.columns.iter().enumerate() {
            if i > 0 {
                builder = builder.raw(", ");
            }
            builder = builder.ident(col);
        }
        builder = builder.raw(")");
        if let Some(parser) = &self.config.mysql_parser {
            builder = builder.raw(" WITH PARSER ").raw(parser);
        }
        builder.into_sql()
    }

    /// Generate CREATE VIRTUAL TABLE statement for SQLite FTS5
    pub fn to_sqlite_sql(&self) -> Vec<String> {
        let mut params = Vec::new();
        let fts_table = format!("{}_fts", self.table);

        let mut columns_builder = SqlBuilder::new(DatabaseType::SQLite, &mut params);
        for (i, col) in self.columns.iter().enumerate() {
            if i > 0 {
                columns_builder = columns_builder.raw(", ");
            }
            columns_builder = columns_builder.ident(col);
        }
        let columns_str = columns_builder.into_sql();

        let mut new_columns_str = String::new();
        let mut old_columns_str = String::new();
        for (i, col) in self.columns.iter().enumerate() {
            if i > 0 {
                new_columns_str.push_str(", ");
                old_columns_str.push_str(", ");
            }
            new_columns_str.push_str(&SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("new.")
                .ident(col)
                .into_sql());
            old_columns_str.push_str(&SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("old.")
                .ident(col)
                .into_sql());
        }

        vec![
            // Create FTS5 virtual table
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("CREATE VIRTUAL TABLE IF NOT EXISTS ")
                .ident(&fts_table)
                .raw(" USING fts5(")
                .raw(&columns_str)
                .raw(", content=")
                .ident(&self.table)
                .raw(", content_rowid=")
                .ident("rowid")
                .raw(")")
                .into_sql(),
            // Create triggers to keep FTS table in sync
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("CREATE TRIGGER IF NOT EXISTS ")
                .ident(&format!("{}_ai", self.table))
                .raw(" AFTER INSERT ON ")
                .ident(&self.table)
                .raw(" BEGIN INSERT INTO \"")
                .raw(&fts_table)
                .raw("\"(rowid, ")
                .raw(&columns_str)
                .raw(") VALUES (new.rowid, ")
                .raw(&new_columns_str)
                .raw("); END")
                .into_sql(),
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("CREATE TRIGGER IF NOT EXISTS ")
                .ident(&format!("{}_ad", self.table))
                .raw(" AFTER DELETE ON ")
                .ident(&self.table)
                .raw(" BEGIN INSERT INTO ")
                .ident(&fts_table)
                .raw("(")
                .ident(&fts_table)
                .raw(", rowid, ")
                .raw(&columns_str)
                .raw(") VALUES('delete', old.rowid, ")
                .raw(&old_columns_str)
                .raw("); END")
                .into_sql(),
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("CREATE TRIGGER IF NOT EXISTS ")
                .ident(&format!("{}_au", self.table))
                .raw(" AFTER UPDATE ON ")
                .ident(&self.table)
                .raw(" BEGIN INSERT INTO ")
                .ident(&fts_table)
                .raw("(")
                .ident(&fts_table)
                .raw(", rowid, ")
                .raw(&columns_str)
                .raw(") VALUES('delete', old.rowid, ")
                .raw(&old_columns_str)
                .raw("); INSERT INTO ")
                .ident(&fts_table)
                .raw("(rowid, ")
                .raw(&columns_str)
                .raw(") VALUES (new.rowid, ")
                .raw(&new_columns_str)
                .raw("); END")
                .into_sql(),
        ]
    }

    /// Generate CREATE INDEX for the current database type
    pub fn to_sql(&self, db_type: DatabaseType) -> Vec<String> {
        match db_type {
            DatabaseType::Postgres => vec![self.to_postgres_sql()],
            DatabaseType::MySQL | DatabaseType::MariaDB => vec![self.to_mysql_sql()],
            DatabaseType::SQLite => self.to_sqlite_sql(),
        }
    }
}

// =============================================================================
// HIGHLIGHTING UTILITIES
// =============================================================================

/// Highlight search terms in text
pub fn highlight_text(text: &str, query: &str, start_tag: &str, end_tag: &str) -> String {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut result = text.to_string();

    // Pre-compile all regex patterns outside the loop to avoid regex_creation_in_loops
    let patterns: Vec<regex::Regex> = words
        .iter()
        .filter_map(|word| regex::Regex::new(&format!(r"(?i)\b{}\b", regex::escape(word))).ok())
        .collect();

    for pattern in &patterns {
        result = pattern
            .replace_all(&result, |caps: &regex::Captures| {
                format!("{}{}{}", start_tag, &caps[0], end_tag)
            })
            .to_string();
    }

    result
}

/// Generate highlighted snippets from text
pub fn generate_snippet(
    text: &str,
    query: &str,
    fragment_words: usize,
    start_tag: &str,
    end_tag: &str,
) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let query_words_owned: Vec<String> =
        query.split_whitespace().map(|w| w.to_lowercase()).collect();

    // Find the first matching word position
    let mut match_pos = None;
    for (i, word) in words.iter().enumerate() {
        let word_lower = word.to_lowercase();
        if query_words_owned.iter().any(|q| word_lower.contains(q)) {
            match_pos = Some(i);
            break;
        }
    }

    if let Some(pos) = match_pos {
        let start = pos.saturating_sub(fragment_words);
        let end = (pos + fragment_words).min(words.len());

        let snippet_words: Vec<String> = words[start..end]
            .iter()
            .map(|w| {
                let word_lower = w.to_lowercase();
                if query_words_owned.iter().any(|q| word_lower.contains(q)) {
                    format!("{}{}{}", start_tag, w, end_tag)
                } else {
                    w.to_string()
                }
            })
            .collect();

        let mut snippet = snippet_words.join(" ");
        if start > 0 {
            snippet = format!("...{}", snippet);
        }
        if end < words.len() {
            snippet = format!("{}...", snippet);
        }
        snippet
    } else {
        // No match found, return beginning of text
        let end = fragment_words.min(words.len());
        let snippet = words[..end].join(" ");
        if end < words.len() {
            format!("{}...", snippet)
        } else {
            snippet
        }
    }
}

/// PostgreSQL-specific highlighting using ts_headline
pub fn pg_headline_sql(
    column: &str,
    query: &str,
    language: &str,
    start_tag: &str,
    end_tag: &str,
) -> String {
    let column = format_identifier_reference(DatabaseType::Postgres, column)
        .unwrap_or_else(|| quote_ident(DatabaseType::Postgres, column));
    let mut params = Vec::new();
    SqlBuilder::new(DatabaseType::Postgres, &mut params)
        .raw("ts_headline('")
        .raw(&escape_string(language))
        .raw("', ")
        .raw(&column)
        .raw(", plainto_tsquery('")
        .raw(&escape_string(language))
        .raw("', '")
        .raw(&escape_string(query))
        .raw("'), 'StartSel=")
        .raw(&escape_string(start_tag))
        .raw(", StopSel=")
        .raw(&escape_string(end_tag))
        .raw(", MaxWords=35, MinWords=15')")
        .into_sql()
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Escape a string for SQL queries
fn escape_string(s: &str) -> String {
    escape_sql_literal(s).replace('\\', "\\\\")
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
#[path = "../../tests/unit/fulltext_tests.rs"]
mod tests;
