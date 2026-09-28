struct SearchSegment {
    text: String,
    quoted: bool,
    /// The `+ - ~ < >` written right before a quoted phrase, as in
    /// `-"java script"`; an unquoted term keeps its operator in `text`.
    operator: Option<char>,
}

impl SearchSegment {
    /// Whether a boolean search leaves out what this segment matches.
    fn excluded(&self) -> bool {
        if self.quoted {
            self.operator == Some('-')
        } else {
            self.text.starts_with('-')
        }
    }
}

/// The operators a boolean search reads before a term.
fn is_boolean_operator(ch: char) -> bool {
    matches!(ch, '+' | '-' | '~' | '<' | '>')
}

/// Close the segment gathered in `current`, dropping it when it is blank.
fn flush_segment(
    segments: &mut Vec<SearchSegment>,
    current: &mut String,
    quoted: bool,
    operator: Option<char>,
) {
    let text = current.trim();
    if !text.is_empty() {
        segments.push(SearchSegment {
            text: text.to_string(),
            quoted,
            operator,
        });
    }
    current.clear();
}

/// Split search input on whitespace, keeping double-quoted phrases whole.
///
/// An operator written against the opening quote belongs to the phrase, and
/// an unterminated quote still yields its text as a quoted segment.
fn split_search_segments(input: &str) -> Vec<SearchSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut operator = None;

    for ch in input.chars() {
        match ch {
            '"' if !in_quotes => {
                let pending = current.trim();
                operator = (!pending.is_empty() && pending.chars().all(is_boolean_operator))
                    .then(|| pending.chars().last())
                    .flatten();
                if operator.is_some() {
                    current.clear();
                } else {
                    flush_segment(&mut segments, &mut current, false, None);
                }
                in_quotes = true;
            }
            '"' => {
                flush_segment(&mut segments, &mut current, true, operator.take());
                in_quotes = false;
            }
            _ if ch.is_whitespace() && !in_quotes => {
                flush_segment(&mut segments, &mut current, false, None);
            }
            _ => current.push(ch),
        }
    }

    flush_segment(&mut segments, &mut current, in_quotes, operator.take());
    segments
}

/// Whether a run of characters can stand on its own as a search term.
///
/// A run made up only of punctuation carries no lexeme: PostgreSQL rejects the
/// resulting empty quoted lexeme with `syntax error in tsquery`, and FTS5
/// rejects an empty `MATCH` operand, so such runs are dropped rather than
/// forwarded to either query parser.
pub(crate) fn has_searchable_char(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// The searchable words of `input`, quotes and operators aside.
pub(crate) fn search_words(input: &str) -> Vec<&str> {
    input
        .split(|ch: char| ch.is_whitespace() || ch == '"')
        .filter(|word| has_searchable_char(word))
        .collect()
}

fn extract_postgres_lexemes(input: &str) -> Vec<String> {
    let mut lexemes = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if ch.is_alphanumeric() || matches!(ch, '_' | '\'') {
            current.push(ch);
        } else if !current.is_empty() {
            lexemes.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        lexemes.push(current);
    }

    lexemes.retain(|lexeme| has_searchable_char(lexeme));
    lexemes
}

fn format_postgres_lexeme(lexeme: &str, prefix: bool) -> String {
    let escaped = lexeme.replace('\'', "''");
    if prefix {
        format!("'{}':*", escaped)
    } else {
        format!("'{}'", escaped)
    }
}

/// One tsquery phrase per search segment: its lexemes joined by
/// `joiner(segment.quoted)`, parenthesized when there is more than one, with
/// whether a boolean search excludes it.
fn postgres_tsquery_phrases(
    input: &str,
    prefix: bool,
    joiner: impl Fn(bool) -> &'static str,
) -> Vec<(String, bool)> {
    split_search_segments(input)
        .into_iter()
        .filter_map(|segment| {
            let lexemes: Vec<String> = extract_postgres_lexemes(&segment.text)
                .iter()
                .map(|lexeme| format_postgres_lexeme(lexeme, prefix))
                .collect();

            let phrase = match lexemes.len() {
                0 => return None,
                1 => lexemes.into_iter().next()?,
                _ => format!("({})", lexemes.join(joiner(segment.quoted))),
            };
            Some((phrase, segment.excluded()))
        })
        .collect()
}

pub(crate) fn sanitize_postgres_tsquery_literals(input: &str, prefix: bool) -> String {
    postgres_tsquery_phrases(input, prefix, |quoted| if quoted { " <-> " } else { " & " })
        .into_iter()
        .map(|(phrase, _)| phrase)
        .collect::<Vec<_>>()
        .join(" & ")
}

/// The furthest apart a PostgreSQL proximity search looks, in words.
const POSTGRES_PROXIMITY_LIMIT: u32 = 64;

/// Each pair of neighbouring terms within `distance` words of each other,
/// either way round.
///
/// A tsquery's `<n>` means exactly `n` apart and in that order, so "within" is
/// written out as every distance up to `distance`, both ways, capped at
/// [`POSTGRES_PROXIMITY_LIMIT`] to keep the query small.
pub(crate) fn sanitize_postgres_proximity_tsquery_literals(input: &str, distance: u32) -> String {
    let phrases: Vec<String> = postgres_tsquery_phrases(input, false, |_| " <-> ")
        .into_iter()
        .map(|(phrase, _)| phrase)
        .collect();
    if phrases.len() < 2 {
        return phrases.concat();
    }
    let within = |left: &str, right: &str| {
        let alternatives: Vec<String> = (1..=distance.clamp(1, POSTGRES_PROXIMITY_LIMIT))
            .flat_map(|apart| {
                [
                    format!("{left} <{apart}> {right}"),
                    format!("{right} <{apart}> {left}"),
                ]
            })
            .collect();
        format!("({})", alternatives.join(" | "))
    };
    phrases
        .windows(2)
        .map(|pair| within(&pair[0], &pair[1]))
        .collect::<Vec<_>>()
        .join(" & ")
}

/// A boolean search as a tsquery: every term and phrase is required, and one
/// written with a leading `-` is excluded.
pub(crate) fn sanitize_postgres_boolean_tsquery(input: &str) -> String {
    postgres_tsquery_phrases(input, false, |quoted| if quoted { " <-> " } else { " & " })
        .into_iter()
        .map(|(phrase, excluded)| {
            if excluded {
                format!("!{phrase}")
            } else {
                phrase
            }
        })
        .collect::<Vec<_>>()
        .join(" & ")
}

/// `text` between double quotes, each one in it doubled: an FTS5 string,
/// which FTS5 tokenizes instead of parsing, or a `ts_headline` option value.
pub(crate) fn double_quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// Rewrite user input as a list of FTS5 string literals.
///
/// Every segment becomes a double-quoted string, so FTS5 operators (`*`, `:`,
/// `^`, `AND`/`OR`/`NOT`, column filters) are matched as text rather than
/// parsed. Segments that tokenize to nothing are dropped, which means a
/// whitespace-only or punctuation-only query returns an empty string; callers
/// must treat that as "no terms" instead of emitting `MATCH ''`, which FTS5
/// rejects with `fts5: syntax error near ""`. The functions below build the
/// other search modes from the same literals, joined by operators of their own.
pub(crate) fn escape_fts5_query_literal_terms(input: &str) -> String {
    split_search_segments(input)
        .into_iter()
        .filter(|segment| has_searchable_char(&segment.text))
        .map(|segment| double_quoted(&segment.text))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A boolean search for FTS5: the terms and phrases, less every one written
/// with a leading `-`. FTS5's `NOT` needs something to subtract from, so a
/// search of exclusions only has no operand.
pub(crate) fn fts5_boolean_query(input: &str) -> String {
    let (excluded, included): (Vec<_>, Vec<_>) = split_search_segments(input)
        .into_iter()
        .filter(|segment| has_searchable_char(&segment.text))
        .partition(SearchSegment::excluded);
    if included.is_empty() {
        return String::new();
    }
    let included: Vec<String> = included
        .iter()
        .map(|segment| double_quoted(&segment.text))
        .collect();
    let mut query = format!("({})", included.join(" AND "));
    for segment in excluded {
        query.push_str(" NOT ");
        query.push_str(&double_quoted(&segment.text));
    }
    query
}

/// Every word of `input` as one FTS5 phrase.
pub(crate) fn fts5_phrase_query(input: &str) -> String {
    let words = search_words(input);
    if words.is_empty() {
        String::new()
    } else {
        double_quoted(&words.join(" "))
    }
}

/// Every word of `input` as an FTS5 prefix, all of them required.
pub(crate) fn fts5_prefix_query(input: &str) -> String {
    search_words(input)
        .into_iter()
        .map(|word| format!("{}*", double_quoted(word)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The words of `input` within `distance` words of each other.
pub(crate) fn fts5_near_query(input: &str, distance: u32) -> String {
    let words: Vec<String> = search_words(input).into_iter().map(double_quoted).collect();
    match words.as_slice() {
        [] => String::new(),
        [word] => word.clone(),
        _ => format!("NEAR({}, {distance})", words.join(" ")),
    }
}

/// Characters InnoDB's full-text parser gives a meaning, in every search mode.
const MYSQL_FULLTEXT_OPERATORS: &[char] = &['+', '-', '<', '>', '(', ')', '~', '*', '"', '@'];

/// Rewrite user input into an `AGAINST(..)` operand MySQL can always parse.
///
/// The operand is bound, so this is not about injection: InnoDB parses it, and
/// a dangling operator (`*`, a trailing `-`, an unclosed quote) is a syntax
/// error even in natural-language mode. Outside boolean mode the operator
/// characters become spaces. In boolean mode each term keeps one leading
/// `+ - ~ < >` and a trailing `*`, quoted phrases stay whole with the operator
/// written before them, and any other operator character separates words,
/// parentheses included. An empty result means the input had no searchable
/// terms.
pub(crate) fn sanitize_mysql_fulltext_query(input: &str, boolean: bool) -> String {
    if !boolean {
        let text: String = input
            .chars()
            .map(|ch| {
                if MYSQL_FULLTEXT_OPERATORS.contains(&ch) {
                    ' '
                } else {
                    ch
                }
            })
            .collect();
        return text
            .split_whitespace()
            .filter(|word| has_searchable_char(word))
            .collect::<Vec<_>>()
            .join(" ");
    }

    split_search_segments(input)
        .into_iter()
        .filter_map(|segment| {
            if segment.quoted {
                let phrase = segment.text.replace('"', " ");
                return has_searchable_char(&phrase).then(|| {
                    format!(
                        "{}\"{}\"",
                        segment.operator.map(String::from).unwrap_or_default(),
                        phrase.trim()
                    )
                });
            }
            let operator = segment
                .text
                .chars()
                .next()
                .filter(|ch| is_boolean_operator(*ch));
            let prefix = segment.text.ends_with('*');
            // Operator characters inside a term separate words, as MySQL's own
            // tokenizer reads them; the operator binds to the first word and
            // the prefix star to the last.
            let body: String = segment
                .text
                .chars()
                .map(|ch| {
                    if MYSQL_FULLTEXT_OPERATORS.contains(&ch) {
                        ' '
                    } else {
                        ch
                    }
                })
                .collect();
            let words: Vec<&str> = body
                .split_whitespace()
                .filter(|word| has_searchable_char(word))
                .collect();
            (!words.is_empty()).then(|| {
                format!(
                    "{}{}{}",
                    operator.map(String::from).unwrap_or_default(),
                    words.join(" "),
                    if prefix { "*" } else { "" }
                )
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
