//! Case conversion for the names a model derives: table and column names
//! (snake_case) and the entity enum variants (PascalCase).
//!
//! The word boundaries decide the table and column an existing model maps to, so
//! they reproduce `convert_case` 0.11 exactly: the same rules over the same grapheme
//! clusters, including its treatment of digits as separate words (`sha256` →
//! `sha_256`).

use unicode_segmentation::UnicodeSegmentation;

/// `UserProfile` → `user_profile`, `HTTPRequest` → `http_request`.
pub(crate) fn to_snake_case(ident: &str) -> String {
    words(ident)
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// `created_at` → `CreatedAt`, `HTTPRequest` → `HttpRequest`.
pub(crate) fn to_pascal_case(ident: &str) -> String {
    words(ident).into_iter().map(capitalize).collect()
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.as_str().to_lowercase().chars())
            .collect(),
        None => String::new(),
    }
}

#[derive(Clone, Copy)]
enum GraphemeClass {
    Lower,
    Upper,
    Digit,
    Other,
}

/// Only ASCII digits are digits, and only a grapheme whose upper- and lowercase
/// forms differ has a case, so `ª` and `ǅ` are neither upper nor lower.
fn classify(grapheme: &str) -> GraphemeClass {
    if grapheme.chars().all(|c| c.is_ascii_digit()) {
        return GraphemeClass::Digit;
    }
    let (upper, lower) = (grapheme.to_uppercase(), grapheme.to_lowercase());
    if upper == lower {
        GraphemeClass::Other
    } else if grapheme == upper {
        GraphemeClass::Upper
    } else if grapheme == lower {
        GraphemeClass::Lower
    } else {
        GraphemeClass::Other
    }
}

/// Splits at `_` (dropped), between a lowercase and an uppercase letter, between a
/// letter and a digit in either order, and before the last capital of an acronym
/// (`XMLParser` → `XML`, `Parser`). Empty words are kept, so `_private` stays
/// `_private` in snake_case.
fn words(ident: &str) -> Vec<&str> {
    use GraphemeClass::{Digit, Lower, Upper};

    let graphemes: Vec<(usize, &str)> = ident.grapheme_indices(true).collect();
    let classes: Vec<GraphemeClass> = graphemes.iter().map(|&(_, g)| classify(g)).collect();
    let mut words = Vec::new();
    let mut start = 0;
    for (i, &(offset, grapheme)) in graphemes.iter().enumerate() {
        if grapheme == "_" {
            words.push(&ident[start..offset]);
            start = offset + 1;
            continue;
        }
        let next = classes.get(i + 1).copied();
        let after_next = classes.get(i + 2).copied();
        let boundary_follows = matches!(
            (classes[i], next, after_next),
            (Lower, Some(Upper | Digit), _)
                | (Upper, Some(Digit), _)
                | (Digit, Some(Lower | Upper), _)
                | (Upper, Some(Upper), Some(Lower))
        );
        if boundary_follows {
            let end = graphemes[i + 1].0;
            words.push(&ident[start..end]);
            start = end;
        }
    }
    words.push(&ident[start..]);
    words
}
