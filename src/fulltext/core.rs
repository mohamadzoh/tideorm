use super::*;

/// Full-text search configuration for different databases
#[derive(Debug, Clone, Default)]
pub struct FullTextConfig {
    /// Language for stemming/parsing (e.g., "english", "simple")
    pub language: Option<String>,
    /// Search mode
    pub mode: SearchMode,
    /// Search terms shorter than this many characters are left out of the
    /// query. The index is untouched: this filters what is searched for.
    pub min_word_length: Option<u32>,
    /// Search terms longer than this many characters are left out of the query.
    pub max_word_length: Option<u32>,
    /// Search terms left out of the query, compared without regard to case.
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

    /// Leave search terms shorter than `len` characters out of the query.
    pub fn min_word_length(mut self, len: u32) -> Self {
        self.min_word_length = Some(len);
        self
    }

    /// Leave search terms longer than `len` characters out of the query.
    pub fn max_word_length(mut self, len: u32) -> Self {
        self.max_word_length = Some(len);
        self
    }

    /// Leave these words out of the query, compared without regard to case.
    pub fn stop_words(mut self, words: Vec<String>) -> Self {
        self.stop_words = words;
        self
    }

    /// Whether any term filter is set.
    pub(crate) fn filters_terms(&self) -> bool {
        self.min_word_length.is_some()
            || self.max_word_length.is_some()
            || !self.stop_words.is_empty()
    }

    /// Whether the query token survives the term filters. The operators around
    /// a term (`+`, `-`, `*`, quotes, parentheses) count toward neither its
    /// length nor its stop-word match, and a token that is only operators is
    /// kept for the backend's sanitizer to deal with.
    pub(crate) fn keeps_term(&self, token: &str) -> bool {
        let word = token.trim_matches(|character: char| !character.is_alphanumeric());
        if word.is_empty() {
            return true;
        }
        let length = word.chars().count();
        let too_short = self
            .min_word_length
            .is_some_and(|min| length < min as usize);
        let too_long = self
            .max_word_length
            .is_some_and(|max| length > max as usize);
        let lowered = word.to_lowercase();
        let stop_word = self
            .stop_words
            .iter()
            .any(|stop| stop.to_lowercase() == lowered);
        !(too_short || too_long || stop_word)
    }

    /// Set search weights for ranking
    pub fn weights(mut self, weights: SearchWeights) -> Self {
        self.weights = Some(weights);
        self
    }
}

/// Search mode for full-text queries
///
/// Every mode builds the backend's query syntax from the words of the search
/// text, so no operator a user types reaches a query parser as syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    /// The words, as the backend's natural-language search reads them
    /// (default).
    #[default]
    Natural,
    /// Every term is required, one written `-term` or `-"a phrase"` is left
    /// out, and a quoted phrase matches as a phrase. MySQL also reads its own
    /// `~ < > *` operators, and there a term written without `+` is optional.
    /// SQLite cannot subtract from nothing, so a search of exclusions only
    /// matches nothing there.
    Boolean,
    /// The words, in order, as one phrase.
    Phrase,
    /// Words beginning with each search word, all of them required.
    Prefix,
    /// The words within this many words of each other, in either order; at
    /// most 64 on PostgreSQL.
    Proximity(u32),
}

impl fmt::Display for SearchMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchMode::Natural => write!(f, "natural"),
            SearchMode::Boolean => write!(f, "boolean"),
            SearchMode::Phrase => write!(f, "phrase"),
            SearchMode::Prefix => write!(f, "prefix"),
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
        format!("'{}'", self.pg_array())
    }

    /// The `{D,C,B,A}` array `ts_rank_cd` takes, lowest weight first.
    pub(super) fn pg_array(&self) -> String {
        format!("{{{},{},{},{}}}", self.d, self.c, self.b, self.a)
    }
}

/// A search result with ranking information
#[derive(Debug, Clone)]
pub struct SearchResult<T> {
    /// The matched record
    pub record: T,
    /// Relevance score (higher = more relevant)
    pub rank: f64,
    /// The searched fields with their matches marked, filled by
    /// [`get_ranked`](super::FullTextSearchBuilder::get_ranked) when the search
    /// asked for them with [`highlight`](super::FullTextSearchBuilder::highlight).
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
    /// Number of matches marked in `highlighted`
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
        let match_count = highlighted.matches("<mark>").count();
        Self {
            field: field.into(),
            highlighted,
            original,
            match_count,
        }
    }
}

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

    /// Search with ranking enabled; see [`FullTextSearchBuilder::with_ranking`].
    fn search_ranked(columns: &[&str], query: &str) -> FullTextSearchBuilder<Self> {
        FullTextSearchBuilder::new(columns, query).with_ranking()
    }
}

impl<T: Model> FullTextSearch for T {}
