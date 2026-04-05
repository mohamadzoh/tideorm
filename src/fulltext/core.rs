use super::*;

// FULL-TEXT SEARCH CONFIGURATION
// =============================================================================

/// Full-text search configuration for different databases
#[derive(Debug, Clone, Default)]
pub struct FullTextConfig {
    /// Language for stemming/parsing (e.g., "english", "simple")
    pub language: Option<String>,
    /// Search mode
    pub mode: SearchMode,
    /// Minimum word length to index
    pub min_word_length: Option<u32>,
    /// Maximum word length to index
    pub max_word_length: Option<u32>,
    /// Custom stop words to exclude
    pub stop_words: Vec<String>,
    /// Weight configuration for ranked searches
    pub weights: Option<SearchWeights>,
}

impl FullTextConfig {
    /// Create a new full-text search configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the language for text analysis
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    /// Set the search mode
    pub fn mode(mut self, mode: SearchMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set minimum word length
    pub fn min_word_length(mut self, len: u32) -> Self {
        self.min_word_length = Some(len);
        self
    }

    /// Set maximum word length
    pub fn max_word_length(mut self, len: u32) -> Self {
        self.max_word_length = Some(len);
        self
    }

    /// Add stop words to exclude from indexing
    pub fn stop_words(mut self, words: Vec<String>) -> Self {
        self.stop_words = words;
        self
    }

    /// Set search weights for ranking
    pub fn weights(mut self, weights: SearchWeights) -> Self {
        self.weights = Some(weights);
        self
    }
}

/// Search mode for full-text queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// Natural language search (default)
    /// Finds rows that match the search terms naturally
    #[default]
    Natural,
    /// Boolean search mode
    /// Uses backend-specific boolean search behavior.
    /// MySQL forwards native boolean operators, while PostgreSQL and SQLite
    /// sanitize user input into literal terms to avoid query-parser injection.
    Boolean,
    /// Phrase search mode
    /// Matches exact phrases
    Phrase,
    /// Prefix search mode
    /// Matches words that start with the given prefix.
    /// Backends that require parser syntax build that syntax from sanitized
    /// literal terms instead of trusting raw user operators.
    Prefix,
    /// Fuzzy search mode (PostgreSQL only)
    /// Matches similar words using trigrams
    Fuzzy,
    /// Proximity search
    /// Finds terms within a certain distance of each other
    Proximity(u32),
}

impl fmt::Display for SearchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchMode::Natural => write!(f, "natural"),
            SearchMode::Boolean => write!(f, "boolean"),
            SearchMode::Phrase => write!(f, "phrase"),
            SearchMode::Prefix => write!(f, "prefix"),
            SearchMode::Fuzzy => write!(f, "fuzzy"),
            SearchMode::Proximity(d) => write!(f, "proximity({})", d),
        }
    }
}

/// Weight configuration for PostgreSQL tsvector ranking
#[derive(Debug, Clone)]
pub struct SearchWeights {
    /// Weight for 'A' category (highest priority, e.g., title)
    pub a: f32,
    /// Weight for 'B' category
    pub b: f32,
    /// Weight for 'C' category  
    pub c: f32,
    /// Weight for 'D' category (lowest priority, e.g., body)
    pub d: f32,
}

impl Default for SearchWeights {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.4,
            c: 0.2,
            d: 0.1,
        }
    }
}

impl SearchWeights {
    /// Create new weights
    pub fn new(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self { a, b, c, d }
    }

    /// Convert to PostgreSQL weights array format
    pub fn to_pg_array(&self) -> String {
        format!("'{{{},{},{},{}}}'", self.d, self.c, self.b, self.a)
    }
}

// =============================================================================
// SEARCH RESULT TYPES
// =============================================================================

/// A search result with ranking information
#[derive(Debug, Clone)]
pub struct SearchResult<T> {
    /// The matched record
    pub record: T,
    /// Relevance score (higher = more relevant)
    pub rank: f64,
    /// Highlighted snippets (if requested)
    pub highlights: Vec<HighlightedField>,
}

impl<T> SearchResult<T> {
    /// Create a new search result
    pub fn new(record: T, rank: f64) -> Self {
        Self {
            record,
            rank,
            highlights: Vec::new(),
        }
    }

    /// Add highlighted fields
    pub fn with_highlights(mut self, highlights: Vec<HighlightedField>) -> Self {
        self.highlights = highlights;
        self
    }
}

/// A field with highlighted search matches
#[derive(Debug, Clone)]
pub struct HighlightedField {
    /// Field name
    pub field: String,
    /// Field value with highlighted matches
    pub highlighted: String,
    /// Original value
    pub original: String,
    /// Number of matches found
    pub match_count: usize,
}

impl HighlightedField {
    /// Create a new highlighted field
    pub fn new(
        field: impl Into<String>,
        highlighted: impl Into<String>,
        original: impl Into<String>,
    ) -> Self {
        let highlighted = highlighted.into();
        let original = original.into();
        // Count matches by looking for start tags
        let match_count = highlighted.matches("<mark>").count();
        Self {
            field: field.into(),
            highlighted,
            original,
            match_count,
        }
    }
}

// =============================================================================
// FULL-TEXT SEARCH TRAIT
// =============================================================================

/// Trait for models that support full-text search
pub trait FullTextSearch: Model + Sized {
    /// Perform a simple full-text search on specified columns
    fn search(columns: &[&str], query: &str) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query)
    }

    /// Perform a full-text search with custom configuration
    fn search_with_config(
        columns: &[&str],
        query: &str,
        config: FullTextConfig,
    ) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).config(config)
    }

    /// Search with ranking (returns results ordered by relevance)
    fn search_ranked(columns: &[&str], query: &str) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).with_ranking()
    }

    /// Search with highlighting
    fn search_highlighted(
        columns: &[&str],
        query: &str,
        start_tag: &str,
        end_tag: &str,
    ) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).with_highlights(start_tag, end_tag)
    }
}

// Implement FullTextSearch for all Models
impl<T: Model> FullTextSearch for T {}
