//! # TideORM Full-Text Search Example
//!
//! **Category:** Search & Querying
//!
//! This example demonstrates TideORM's full-text search capabilities across
//! PostgreSQL (tsvector/tsquery), MySQL (FULLTEXT), and SQLite (FTS5).
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example fulltext_demo
//! ```

use tideorm::prelude::*;
use tideorm::fulltext::{
    FullTextConfig, FullTextIndex,
    SearchMode, SearchWeights, HighlightConfig,
    PgFullTextIndexType,
    highlight_text, generate_snippet, pg_headline_sql,
};
use tideorm::config::DatabaseType;

// =============================================================================
// MODEL DEFINITION
// =============================================================================

/// Article model for full-text search demonstration
#[tideorm::model]
#[tide(table = "articles")]
pub struct Article {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub author: String,
    pub tags: String,
    pub published: bool,
}

// =============================================================================
// INDEX GENERATION EXAMPLES
// =============================================================================

fn index_generation_examples() {
    println!("\n=== Full-Text Index Generation ===\n");
    
    // PostgreSQL GIN index (faster lookups)
    println!("PostgreSQL GIN Index (single column):");
    let pg_gin_single = FullTextIndex::new(
        "idx_articles_title_fts",
        "articles",
        vec!["title".to_string()]
    )
    .language("english")
    .pg_index_type(PgFullTextIndexType::GIN);
    
    println!("  {}\n", pg_gin_single.to_postgres_sql());
    
    // PostgreSQL GIN index (multiple columns)
    println!("PostgreSQL GIN Index (multiple columns):");
    let pg_gin_multi = FullTextIndex::new(
        "idx_articles_search",
        "articles",
        vec!["title".to_string(), "content".to_string(), "tags".to_string()]
    )
    .language("english")
    .pg_index_type(PgFullTextIndexType::GIN);
    
    println!("  {}\n", pg_gin_multi.to_postgres_sql());
    
    // PostgreSQL GiST index (faster updates, supports ranking)
    println!("PostgreSQL GiST Index:");
    let pg_gist = FullTextIndex::new(
        "idx_articles_content_gist",
        "articles",
        vec!["content".to_string()]
    )
    .language("english")
    .pg_index_type(PgFullTextIndexType::GiST);
    
    println!("  {}\n", pg_gist.to_postgres_sql());
    
    // MySQL FULLTEXT index
    println!("MySQL FULLTEXT Index:");
    let mysql_idx = FullTextIndex::new(
        "idx_articles_fulltext",
        "articles",
        vec!["title".to_string(), "content".to_string()]
    );
    
    println!("  {}\n", mysql_idx.to_mysql_sql());
    
    // SQLite FTS5 virtual table and triggers
    println!("SQLite FTS5 Setup:");
    let sqlite_idx = FullTextIndex::new(
        "idx_articles_fts",
        "articles",
        vec!["title".to_string(), "content".to_string()]
    );
    
    for (i, sql) in sqlite_idx.to_sqlite_sql().iter().enumerate() {
        let label = match i {
            0 => "Virtual Table",
            1 => "Insert Trigger",
            2 => "Delete Trigger",
            3 => "Update Trigger",
            _ => "Other",
        };
        println!("  {}:\n    {}\n", label, sql);
    }
    
    // Generate for current database type
    println!("Generate for specific database:");
    let index = FullTextIndex::new("idx", "table", vec!["col".to_string()]);
    println!("  Postgres: {} statement(s)", index.to_sql(DatabaseType::Postgres).len());
    println!("  MySQL: {} statement(s)", index.to_sql(DatabaseType::MySQL).len());
    println!("  SQLite: {} statement(s)", index.to_sql(DatabaseType::SQLite).len());
}

// =============================================================================
// SEARCH CONFIGURATION EXAMPLES
// =============================================================================

fn search_config_examples() {
    println!("\n=== Search Configuration ===\n");
    
    // Default configuration
    let default_config = FullTextConfig::default();
    println!("Default config:");
    println!("  Language: {:?}", default_config.language);
    println!("  Mode: {:?}", default_config.mode);
    
    // Custom configuration
    let custom_config = FullTextConfig::new()
        .language("german")
        .mode(SearchMode::Boolean)
        .min_word_length(3)
        .max_word_length(50)
        .stop_words(vec![
            "der".to_string(), "die".to_string(), "das".to_string(),
            "und".to_string(), "oder".to_string(),
        ]);
    
    println!("\nCustom German config:");
    println!("  Language: {:?}", custom_config.language);
    println!("  Mode: {:?}", custom_config.mode);
    println!("  Min word length: {:?}", custom_config.min_word_length);
    println!("  Max word length: {:?}", custom_config.max_word_length);
    println!("  Stop words: {:?}", custom_config.stop_words);
    
    // Search weights for ranking
    println!("\nSearch Weights:");
    let weights = SearchWeights::new(1.0, 0.5, 0.3, 0.1);
    println!("  A (highest): {}", weights.a);
    println!("  B: {}", weights.b);
    println!("  C: {}", weights.c);
    println!("  D (lowest): {}", weights.d);
    println!("  PostgreSQL array: {}", weights.to_pg_array());
    
    // Configuration with weights
    let ranked_config = FullTextConfig::new()
        .language("english")
        .mode(SearchMode::Natural)
        .weights(SearchWeights::new(1.0, 0.8, 0.5, 0.2));
    
    println!("\nRanked search config:");
    println!("  Has weights: {}", ranked_config.weights.is_some());
}

// =============================================================================
// SEARCH MODE EXAMPLES
// =============================================================================

fn search_mode_examples() {
    println!("\n=== Search Modes ===\n");
    
    let modes = vec![
        (SearchMode::Natural, "Natural language (default) - matches words naturally"),
        (SearchMode::Boolean, "Boolean mode - supports +, -, * operators"),
        (SearchMode::Phrase, "Phrase mode - matches exact phrases"),
        (SearchMode::Prefix, "Prefix mode - matches word prefixes"),
        (SearchMode::Fuzzy, "Fuzzy mode - matches similar words (PostgreSQL)"),
        (SearchMode::Proximity(3), "Proximity mode - words within N positions"),
    ];
    
    for (mode, description) in modes {
        println!("  {} - {}", mode, description);
    }
    
    println!("\nBoolean mode examples:");
    println!("  '+rust +programming' - must contain both words");
    println!("  'rust -javascript'   - must contain rust, not javascript");
    println!("  'rust*'              - prefix match (rustacean, rustic, etc.)");
    println!("  '\"rust programming\"' - exact phrase match");
}

// =============================================================================
// HIGHLIGHTING EXAMPLES
// =============================================================================

fn highlighting_examples() {
    println!("\n=== Text Highlighting ===\n");
    
    let text = "The quick brown fox jumps over the lazy dog. \
                The fox is known for being quick and clever. \
                Dogs are often loyal companions.";
    
    // Simple highlighting
    println!("Original text:");
    println!("  {}\n", text);
    
    println!("Highlight 'fox':");
    let highlighted = highlight_text(text, "fox", "<b>", "</b>");
    println!("  {}\n", highlighted);
    
    println!("Highlight 'quick fox' (multiple words):");
    let highlighted_multi = highlight_text(text, "quick fox", "<mark>", "</mark>");
    println!("  {}\n", highlighted_multi);
    
    // Case-insensitive highlighting
    let mixed_case = "The QUICK Brown FOX jumped.";
    println!("Case-insensitive ('quick fox' in mixed case text):");
    let case_highlight = highlight_text(mixed_case, "quick fox", "<em>", "</em>");
    println!("  {}\n", case_highlight);
    
    // Different tag styles
    println!("Different highlight styles:");
    println!("  HTML bold: {}", highlight_text("The fox ran.", "fox", "<b>", "</b>"));
    println!("  HTML mark: {}", highlight_text("The fox ran.", "fox", "<mark>", "</mark>"));
    println!("  HTML span: {}", highlight_text("The fox ran.", "fox", "<span class=\"hl\">", "</span>"));
    println!("  ANSI color: {}", highlight_text("The fox ran.", "fox", "\x1b[33m", "\x1b[0m"));
}

// =============================================================================
// SNIPPET GENERATION EXAMPLES
// =============================================================================

fn snippet_examples() {
    println!("\n=== Snippet Generation ===\n");
    
    let long_text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                    Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
                    The quick brown fox jumps over the lazy dog in the meadow. \
                    Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris. \
                    Duis aute irure dolor in reprehenderit in voluptate velit esse cillum. \
                    The clever fox escaped through the forest at dawn.";
    
    println!("Long text ({} chars):", long_text.len());
    println!("  {}...\n", &long_text[..80]);
    
    // Generate snippet with different fragment sizes
    println!("Snippets for 'fox' with different sizes:");
    
    for size in [3, 5, 10, 15] {
        let snippet = generate_snippet(long_text, "fox", size, "<b>", "</b>");
        println!("\n  Fragment words = {}:", size);
        println!("    {}", snippet);
    }
    
    // Snippet when match is at the beginning
    println!("\nSnippet when match is near the beginning ('Lorem'):");
    let start_snippet = generate_snippet(long_text, "Lorem", 5, "<b>", "</b>");
    println!("  {}", start_snippet);
    
    // Snippet when there's no match
    println!("\nSnippet when there's no match ('xyz'):");
    let no_match = generate_snippet(long_text, "xyz", 5, "<b>", "</b>");
    println!("  {}", no_match);
    
    // Multiple word snippet
    println!("\nSnippet with multiple search words ('fox lazy'):");
    let multi_word = generate_snippet(long_text, "fox lazy", 5, "<mark>", "</mark>");
    println!("  {}", multi_word);
}

// =============================================================================
// POSTGRESQL HEADLINE SQL EXAMPLES
// =============================================================================

fn pg_headline_examples() {
    println!("\n=== PostgreSQL ts_headline SQL Generation ===\n");
    
    // Basic headline
    println!("Basic ts_headline:");
    let basic = pg_headline_sql("content", "search term", "english", "<b>", "</b>");
    println!("  {}\n", basic);
    
    // With different language
    println!("German language:");
    let german = pg_headline_sql("body", "suchbegriff", "german", "<mark>", "</mark>");
    println!("  {}\n", german);
    
    // Complex query
    println!("Complex multi-word query:");
    let complex = pg_headline_sql(
        "article_body",
        "rust programming async await tokio",
        "english",
        "<span class=\"highlight\">",
        "</span>"
    );
    println!("  {}\n", complex);
    
    // Using in a full SELECT
    println!("Example in full SELECT query:");
    let query = format!(
        "SELECT id, title, {} AS highlighted_content FROM articles WHERE ...",
        pg_headline_sql("content", "search query", "english", "<b>", "</b>")
    );
    println!("  {}", query);
}

// =============================================================================
// HIGHLIGHT CONFIG EXAMPLES
// =============================================================================

fn highlight_config_examples() {
    println!("\n=== HighlightConfig Examples ===\n");
    
    // Default config
    let default = HighlightConfig::default();
    println!("Default HighlightConfig:");
    println!("  start_tag: {}", default.start_tag);
    println!("  end_tag: {}", default.end_tag);
    println!("  max_length: {:?}", default.max_length);
    println!("  fragment_words: {:?}", default.fragment_words);
    
    // Custom config
    let custom = HighlightConfig {
        start_tag: "<span class=\"search-match\">".to_string(),
        end_tag: "</span>".to_string(),
        max_length: Some(200),
        fragment_words: Some(15),
    };
    
    println!("\nCustom HighlightConfig:");
    println!("  start_tag: {}", custom.start_tag);
    println!("  end_tag: {}", custom.end_tag);
    println!("  max_length: {:?}", custom.max_length);
    println!("  fragment_words: {:?}", custom.fragment_words);
}

// =============================================================================
// SEARCH RESULT EXAMPLES
// =============================================================================

fn search_result_examples() {
    println!("\n=== SearchResult Structure ===\n");
    
    use tideorm::fulltext::{SearchResult, HighlightedField};
    
    // Create a search result
    let article = Article {
        id: 1,
        title: "Introduction to Rust Programming".to_string(),
        content: "Rust is a systems programming language...".to_string(),
        author: "Jane Doe".to_string(),
        tags: "rust, programming, systems".to_string(),
        published: true,
    };
    
    // Result with rank
    let result: SearchResult<Article> = SearchResult::new(article.clone(), 0.95);
    println!("SearchResult:");
    println!("  record.title: {}", result.record.title);
    println!("  rank: {}", result.rank);
    println!("  highlights: {} field(s)", result.highlights.len());
    
    // Result with highlights
    let highlights = vec![
        HighlightedField::new(
            "title",
            "Introduction to <b>Rust</b> <b>Programming</b>",
            "Introduction to Rust Programming"
        ),
        HighlightedField::new(
            "content",
            "<b>Rust</b> is a systems <b>programming</b> language...",
            "Rust is a systems programming language..."
        ),
    ];
    
    let result_with_highlights = SearchResult::new(article, 0.95)
        .with_highlights(highlights);
    
    println!("\nSearchResult with highlights:");
    println!("  rank: {}", result_with_highlights.rank);
    for hl in &result_with_highlights.highlights {
        println!("  {}:", hl.field);
        println!("    matches: {}", hl.match_count);
        println!("    highlighted: {}", hl.highlighted);
    }
}

// =============================================================================
// PRACTICAL SEARCH EXAMPLES
// =============================================================================

fn practical_search_examples() {
    println!("\n=== Practical Search Usage (Conceptual) ===\n");
    
    // NOTE: These examples show the API but require a database connection to run
    
    println!("Basic search (requires database):");
    print!(r#"
    // Simple full-text search
    let results = Article::search(&["title", "content"], "rust programming")
        .await?;
    
    // Search with configuration
    let results = Article::search_with_config(
        &["title", "content"],
        "rust async",
        FullTextConfig::new()
            .language("english")
            .mode(SearchMode::Boolean)
    ).get().await?;
    
    // Search with ranking
    let ranked = Article::search_ranked(&["title", "content"], "rust")
        .limit(10)
        .get_ranked()
        .await?;
    
    for result in ranked {{
        println!("{{id}}: {{title}} (rank: {{rank}})", result.record.id, result.record.title, result.rank);
    }}
    
    // Search with highlighting
    let highlighted = Article::search_highlighted(
        &["content"],
        "rust programming",
        "<mark>",
        "</mark>"
    ).get().await?;
    
    // Count matching results
    let count = Article::search(&["title", "content"], "rust")
        .count()
        .await?;
    
    // Get first result
    let first = Article::search(&["title"], "rust")
        .first()
        .await?;
    "#);
    
    println!("\nDifferent search modes:");
    print!(r#"
    // Natural language search (default)
    Article::search(&["content"], "learn rust programming").await?;
    
    // Boolean search with operators
    Article::search(&["content"], "+rust +async -javascript")
        .mode(SearchMode::Boolean)
        .get()
        .await?;
    
    // Phrase search
    Article::search(&["content"], "async await")
        .mode(SearchMode::Phrase)
        .get()
        .await?;
    
    // Prefix search (autocomplete)
    Article::search(&["title"], "prog")
        .mode(SearchMode::Prefix)
        .get()
        .await?;
    "#);
}

// =============================================================================
// MAIN
// =============================================================================

fn main() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║          TideORM Full-Text Search Demo                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    
    index_generation_examples();
    search_config_examples();
    search_mode_examples();
    highlighting_examples();
    snippet_examples();
    pg_headline_examples();
    highlight_config_examples();
    search_result_examples();
    practical_search_examples();
    
    println!("\n✓ Full-text search demo complete!");
    println!("\nNote: Search query execution requires a database connection.");
    println!("The examples above demonstrate the API and SQL generation.");
}
