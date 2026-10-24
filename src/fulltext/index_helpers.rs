use super::*;

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
        let index_type = match self.config.pg_index_type {
            PgFullTextIndexType::GIN => "GIN",
            PgFullTextIndexType::GiST => "GiST",
        };

        let mut params = Vec::new();
        SqlBuilder::new(DatabaseType::Postgres, &mut params)
            .raw("CREATE INDEX ")
            .ident(&self.name)
            .raw(" ON ")
            .table(&self.table)
            .raw(" USING ")
            .raw(index_type)
            .raw(" ((")
            .raw(&pg_tsvector(self.config.language.as_deref(), &self.columns))
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
            .table(&self.table)
            .raw("(")
            .raw(&column_list(DatabaseType::MySQL, &self.columns, ""))
            .raw(")");
        // `WITH PARSER` takes an identifier, so it goes through the same quoting
        // path as every other identifier in this module rather than being pasted
        // in raw. A name that is not a real parser plugin then fails loudly on
        // the server instead of silently extending the statement.
        if let Some(parser) = &self.config.mysql_parser {
            builder = builder.raw(" WITH PARSER ").ident(parser);
        }
        builder.into_sql()
    }

    /// Generate CREATE VIRTUAL TABLE statement for SQLite FTS5, the statement
    /// that indexes the rows the table already holds, and the triggers that
    /// keep it in sync with the table
    ///
    /// A declared `rowid` column is supported only when it aliases SQLite's
    /// row identity (`INTEGER PRIMARY KEY`). Other shadowing columns are
    /// rejected before index installation or rebuild, and before searches.
    ///
    /// The index follows the table's rowid. SQLite keeps rowids through a
    /// `VACUUM` only for a table with an `INTEGER PRIMARY KEY`; after
    /// vacuuming any other table, a UUID- or text-keyed one, run
    /// [`sqlite_rebuild_sql`](Self::sqlite_rebuild_sql) before anything reads
    /// or writes it, or searches return other rows.
    ///
    /// A table has one SQLite full-text index, the `<table>_fts` table its
    /// searches read, whatever the index is named. Applying the same index
    /// again changes nothing. A conflicting definition under a maintenance
    /// trigger's name is rejected and left untouched. Applying one over a column the table's index
    /// does not hold is refused rather than keeping the first index's columns:
    /// drop `<table>_fts` and its maintenance triggers before changing the index.
    ///
    /// The index of a table in an attached database (`tenant.posts`) is
    /// created in that database. Its content table and its triggers' tables
    /// are named without it, as FTS5 and SQLite's triggers require.
    pub fn to_sqlite_sql(&self) -> Vec<String> {
        let mut params = Vec::new();
        let (schema, table) = self.sqlite_table_parts();
        let rowid_guard = sqlite_rowid_guard(table, schema);
        let fts_table = format!("{table}_fts");
        let columns = column_list(DatabaseType::SQLite, &self.columns, "");

        let insert_new = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("INSERT INTO ")
            .ident(&fts_table)
            .raw("(rowid, ")
            .raw(&columns)
            .raw(") VALUES (new.rowid, ")
            .raw(&column_list(DatabaseType::SQLite, &self.columns, "new."))
            .raw(");")
            .into_sql();
        let delete_old = SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("INSERT INTO ")
            .ident(&fts_table)
            .raw("(")
            .ident(&fts_table)
            .raw(", rowid, ")
            .raw(&columns)
            .raw(") VALUES('delete', old.rowid, ")
            .raw(&column_list(DatabaseType::SQLite, &self.columns, "old."))
            .raw(");")
            .into_sql();

        let update = format!("{delete_old} {insert_new}");
        let conflicts = [
            self.sqlite_trigger_conflict("ai", "INSERT", &insert_new),
            self.sqlite_trigger_conflict("ad", "DELETE", &delete_old),
            self.sqlite_trigger_conflict("au", "UPDATE", &update),
        ]
        .join(" OR ");
        vec![
            format!(
                "SELECT {rowid_guard}, CASE WHEN {conflicts} THEN {} ELSE 1 END",
                sqlite_fail("TideORM full-text trigger name conflict")
            ),
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("CREATE VIRTUAL TABLE IF NOT EXISTS ")
                .raw(&self.sqlite_qualified(&fts_table))
                .raw(" USING fts5(")
                .raw(&columns)
                .raw(", content=")
                .ident(table)
                .raw(", content_rowid=")
                .ident("rowid")
                .raw(")")
                .into_sql(),
            // An index already there, which the statement above kept, has to
            // hold these columns. They are qualified: SQLite reads an unknown
            // bare `"column"` as a string.
            SqlBuilder::new(DatabaseType::SQLite, &mut params)
                .raw("SELECT ")
                .raw(&column_list(
                    DatabaseType::SQLite,
                    &self.columns,
                    &format!("{}.", self.sqlite_qualified(&fts_table)),
                ))
                .raw(" FROM ")
                .raw(&self.sqlite_qualified(&fts_table))
                .raw(" LIMIT 0")
                .into_sql(),
            self.sqlite_trigger("ai", "INSERT", &insert_new),
            self.sqlite_trigger("ad", "DELETE", &delete_old),
            self.sqlite_trigger("au", "UPDATE", &update),
            // An external-content table starts empty: without a rebuild, rows
            // written before the index was created are never found.
            self.sqlite_rebuild_sql(),
        ]
    }

    /// The SQLite statement that indexes the table's rows afresh, by their
    /// rowids as they are now: what a `VACUUM` of a table without an
    /// `INTEGER PRIMARY KEY` calls for (see [`to_sqlite_sql`](Self::to_sqlite_sql)).
    pub fn sqlite_rebuild_sql(&self) -> String {
        let (schema, table) = self.sqlite_table_parts();
        let rowid_guard = sqlite_rowid_guard(table, schema);
        let fts_table = format!("{table}_fts");
        SqlBuilder::new(DatabaseType::SQLite, &mut Vec::new())
            .raw("INSERT INTO ")
            .raw(&self.sqlite_qualified(&fts_table))
            .raw("(")
            .ident(&fts_table)
            .raw(") SELECT CASE WHEN ")
            .raw(&rowid_guard)
            .raw(" THEN 'rebuild' END")
            .into_sql()
    }

    /// Refuse a user trigger (or an obsolete definition) under a maintenance
    /// trigger's name. IF NOT EXISTS alone silently leaves the index unmaintained.
    fn sqlite_trigger_conflict(&self, suffix: &str, event: &str, body: &str) -> String {
        let (schema, table) = self.sqlite_table_parts();
        let name = format!("{table}_{suffix}");
        // sqlite_schema removes IF NOT EXISTS and the trigger name's schema.
        let expected = format!(
            "CREATE TRIGGER {} AFTER {event} ON {} BEGIN {body} END",
            quote_ident(DatabaseType::SQLite, &name),
            quote_ident(DatabaseType::SQLite, table)
        );
        let catalog = match schema {
            Some(schema) => format!(
                "{}.sqlite_schema",
                quote_ident(DatabaseType::SQLite, schema)
            ),
            None => String::from("main.sqlite_schema"),
        };
        let name = escape_sql_literal_for_db(DatabaseType::SQLite, &name);
        let expected = escape_sql_literal_for_db(DatabaseType::SQLite, &expected);
        format!(
            "EXISTS (SELECT 1 FROM {catalog} WHERE type = 'trigger' AND name = '{name}' COLLATE NOCASE AND sql IS NOT '{expected}')"
        )
    }

    /// Render the `AFTER <event>` trigger named `<table>_<suffix>`.
    fn sqlite_trigger(&self, suffix: &str, event: &str, body: &str) -> String {
        let (_, table) = self.sqlite_table_parts();
        let mut params = Vec::new();
        SqlBuilder::new(DatabaseType::SQLite, &mut params)
            .raw("CREATE TRIGGER IF NOT EXISTS ")
            .raw(&self.sqlite_qualified(&format!("{table}_{suffix}")))
            .raw(" AFTER ")
            .raw(event)
            .raw(" ON ")
            .ident(table)
            .raw(" BEGIN ")
            .raw(body)
            .raw(" END")
            .into_sql()
    }

    /// The attached database the table is named in, if any, and its name.
    fn sqlite_table_parts(&self) -> (Option<&str>, &str) {
        match self.table.split_once('.') {
            Some((schema, table))
                if is_safe_identifier_segment(schema) && is_safe_identifier_segment(table) =>
            {
                (Some(schema), table)
            }
            _ => (None, &self.table),
        }
    }

    /// `name` quoted, in the table's attached database when it has one.
    fn sqlite_qualified(&self, name: &str) -> String {
        let quoted = quote_ident(DatabaseType::SQLite, name);
        match self.sqlite_table_parts() {
            (Some(schema), _) => format!("{}.{quoted}", quote_ident(DatabaseType::SQLite, schema)),
            (None, _) => quoted,
        }
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

/// Require the `rowid` spelling used by FTS content, triggers, and joins to
/// identify the actual SQLite row. Other rowid aliases may themselves be
/// shadowed, so switching blindly to `_rowid_` or `oid` is not safe.
/// An INTEGER PRIMARY KEY alias has no separate primary-key index; checking
/// that distinguishes it from composite keys and INTEGER PRIMARY KEY DESC.
pub(super) fn sqlite_rowid_guard(table: &str, schema: Option<&str>) -> String {
    let table = escape_sql_literal_for_db(DatabaseType::SQLite, table);
    let schema = schema
        .map(|schema| {
            format!(
                ", '{}'",
                escape_sql_literal_for_db(DatabaseType::SQLite, schema)
            )
        })
        .unwrap_or_default();
    format!(
        "CASE WHEN EXISTS (SELECT 1 FROM pragma_table_xinfo('{table}'{schema}) AS c \
         WHERE c.name = 'rowid' COLLATE NOCASE AND NOT \
         (upper(c.type) = 'INTEGER' AND c.pk = 1 AND NOT EXISTS \
         (SELECT 1 FROM pragma_index_list('{table}'{schema}) WHERE origin = 'pk'))) \
         THEN {} ELSE 1 END",
        sqlite_fail("TideORM full-text rowid is shadowed by a non-identity column")
    )
}

/// An SQLite expression that fails, when evaluated, with an error naming
/// `message`: `json_extract` reports a malformed path with the path in it.
/// `message` is a constant of ours, never caller text.
fn sqlite_fail(message: &str) -> String {
    format!("json_extract('{{}}', '{message}')")
}

/// Highlight search terms in text
///
/// Each whole word of `query` is marked, regardless of case. `text` is not
/// HTML-escaped; [`HighlightConfig`] escapes it around the tags it inserts.
pub fn highlight_text(text: &str, query: &str, start_tag: &str, end_tag: &str) -> String {
    match term_pattern(query) {
        Some(pattern) => mark_matches(text, &pattern, start_tag, end_tag, false).0,
        None => text.to_string(),
    }
}

/// The words of `query` as one case-insensitive whole-word pattern, or `None`
/// when there are none (or too many to compile).
///
/// One pattern marks every term in one pass, so a term cannot match inside the
/// tags an earlier one inserted (`b` inside `<b>`), and the longest terms come
/// first, so `rust-lang` is marked whole rather than as `rust`.
pub(crate) fn term_pattern(query: &str) -> Option<regex::Regex> {
    let mut words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    words.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));
    let alternation: Vec<String> = words.into_iter().map(regex::escape).collect();
    regex::Regex::new(&format!(r"(?i)\b(?:{})\b", alternation.join("|"))).ok()
}

/// `text` with every match of `pattern` between the tags, HTML-escaped around
/// them when `escape` is set, and how many matches were marked.
pub(crate) fn mark_matches(
    text: &str,
    pattern: &regex::Regex,
    start_tag: &str,
    end_tag: &str,
    escape: bool,
) -> (String, usize) {
    let plain = |marked: &mut String, part: &str| {
        if escape {
            push_escaped_html(marked, part);
        } else {
            marked.push_str(part);
        }
    };
    let mut marked = String::with_capacity(text.len());
    let mut matches = 0;
    let mut last = 0;
    for found in pattern.find_iter(text) {
        plain(&mut marked, &text[last..found.start()]);
        marked.push_str(start_tag);
        plain(&mut marked, found.as_str());
        marked.push_str(end_tag);
        last = found.end();
        matches += 1;
    }
    plain(&mut marked, &text[last..]);
    (marked, matches)
}

/// `text` with the five characters HTML gives a meaning replaced by entities.
pub(crate) fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    push_escaped_html(&mut escaped, text);
    escaped
}

/// Append `text` to `escaped` as [`escape_html`] escapes it.
fn push_escaped_html(escaped: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
}

/// The `fragment_words` words either side of the word holding the first match
/// of `pattern`, or the first words of `text` when nothing matches, with `...`
/// where words were left out.
pub(crate) fn snippet_around_first_match(
    text: &str,
    pattern: Option<&regex::Regex>,
    fragment_words: usize,
) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let center = pattern
        .and_then(|pattern| pattern.find(text))
        .map_or(0, |found| {
            let before = &text[..found.start()];
            let index = before.split_whitespace().count();
            // A match that starts inside a word belongs to that word.
            if before
                .chars()
                .next_back()
                .is_some_and(|ch| !ch.is_whitespace())
            {
                index - 1
            } else {
                index
            }
        });
    let start = center.saturating_sub(fragment_words);
    let end = center
        .saturating_add(fragment_words)
        .saturating_add(1)
        .min(words.len());

    let mut snippet = words[start..end].join(" ");
    if start > 0 {
        snippet.insert_str(0, "...");
    }
    if end < words.len() {
        snippet.push_str("...");
    }
    snippet
}

/// Generate highlighted snippets from text
///
/// The `fragment_words` words either side of the first whole-word match of
/// `query`, with each whole-word match between the tags as [`highlight_text`]
/// marks it; the first words of `text` when nothing matches. `text` is not
/// HTML-escaped.
pub fn generate_snippet(
    text: &str,
    query: &str,
    fragment_words: usize,
    start_tag: &str,
    end_tag: &str,
) -> String {
    let pattern = term_pattern(query);
    let snippet = snippet_around_first_match(text, pattern.as_ref(), fragment_words);
    match pattern {
        Some(pattern) => mark_matches(&snippet, &pattern, start_tag, end_tag, false).0,
        None => snippet,
    }
}

/// PostgreSQL-specific highlighting using ts_headline
///
/// `start_tag` and `end_tag` end up inside `ts_headline`'s comma-separated
/// option string, so they are double-quoted there (with embedded `"` doubled)
/// to keep a caller-supplied tag from being read as another option such as
/// `MaxWords` or `MaxFragments`.
pub fn pg_headline_sql(
    column: &str,
    query: &str,
    language: &str,
    start_tag: &str,
    end_tag: &str,
) -> String {
    let column = format_identifier_reference(DatabaseType::Postgres, column)
        .unwrap_or_else(|| quote_ident(DatabaseType::Postgres, column));
    let options = format!(
        "StartSel={}, StopSel={}, MaxWords=35, MinWords=15",
        double_quoted(start_tag),
        double_quoted(end_tag),
    );

    let mut params = Vec::new();
    SqlBuilder::new(DatabaseType::Postgres, &mut params)
        .raw("ts_headline('")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, language))
        .raw("', ")
        .raw(&column)
        .raw(", plainto_tsquery('")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, language))
        .raw("', '")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, query))
        .raw("'), '")
        .raw(&escape_sql_literal_for_db(DatabaseType::Postgres, &options))
        .raw("')")
        .into_sql()
}
