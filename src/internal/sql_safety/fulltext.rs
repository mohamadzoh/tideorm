struct SearchSegment {
    text: String,
    quoted: bool,
}

fn split_search_segments(input: &str) -> Vec<SearchSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' if in_quotes => {
                let text = current.trim();
                if !text.is_empty() {
                    segments.push(SearchSegment {
                        text: text.to_string(),
                        quoted: true,
                    });
                }
                current.clear();
                in_quotes = false;
            }
            '"' => {
                let text = current.trim();
                if !text.is_empty() {
                    segments.push(SearchSegment {
                        text: text.to_string(),
                        quoted: false,
                    });
                    current.clear();
                }
                in_quotes = true;
            }
            _ if ch.is_whitespace() && !in_quotes => {
                let text = current.trim();
                if !text.is_empty() {
                    segments.push(SearchSegment {
                        text: text.to_string(),
                        quoted: false,
                    });
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    let text = current.trim();
    if !text.is_empty() {
        segments.push(SearchSegment {
            text: text.to_string(),
            quoted: in_quotes,
        });
    }

    segments
}

/// Whether a run of characters can stand on its own as a search term.
///
/// A run made up only of punctuation carries no lexeme: PostgreSQL rejects the
/// resulting empty quoted lexeme with `syntax error in tsquery`, and FTS5
/// rejects an empty `MATCH` operand, so such runs are dropped rather than
/// forwarded to either query parser.
fn has_searchable_char(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
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

pub(crate) fn sanitize_postgres_tsquery_literals(input: &str, prefix: bool) -> String {
    let parts: Vec<String> = split_search_segments(input)
        .into_iter()
        .filter_map(|segment| {
            let lexemes = extract_postgres_lexemes(&segment.text);
            if lexemes.is_empty() {
                return None;
            }

            let joiner = if segment.quoted { " <-> " } else { " & " };
            let formatted: Vec<String> = lexemes
                .iter()
                .map(|lexeme| format_postgres_lexeme(lexeme, prefix))
                .collect();

            let phrase = formatted.join(joiner);
            Some(if formatted.len() > 1 {
                format!("({})", phrase)
            } else {
                phrase
            })
        })
        .collect();

    if parts.is_empty() {
        String::new()
    } else {
        parts.join(" & ")
    }
}

pub(crate) fn sanitize_postgres_proximity_tsquery_literals(input: &str, distance: u32) -> String {
    let parts: Vec<String> = split_search_segments(input)
        .into_iter()
        .filter_map(|segment| {
            let lexemes = extract_postgres_lexemes(&segment.text);
            if lexemes.is_empty() {
                return None;
            }

            let phrase = lexemes
                .iter()
                .map(|lexeme| format_postgres_lexeme(lexeme, false))
                .collect::<Vec<_>>()
                .join(" <-> ");

            Some(if lexemes.len() > 1 {
                format!("({})", phrase)
            } else {
                phrase
            })
        })
        .collect();

    if parts.is_empty() {
        String::new()
    } else {
        parts.join(&format!(" <{}> ", distance))
    }
}

/// Rewrite user input as a list of FTS5 string literals.
///
/// Every segment becomes a double-quoted string, so FTS5 operators (`*`, `:`,
/// `^`, `AND`/`OR`/`NOT`, column filters) are matched as text rather than
/// parsed. Segments that tokenize to nothing are dropped, which means a
/// whitespace-only or punctuation-only query returns an empty string; callers
/// must treat that as "no terms" instead of emitting `MATCH ''`, which FTS5
/// rejects with `fts5: syntax error near ""`.
pub(crate) fn escape_fts5_query_literal_terms(input: &str) -> String {
    split_search_segments(input)
        .into_iter()
        .filter(|segment| has_searchable_char(&segment.text))
        .map(|segment| format!("\"{}\"", segment.text.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
