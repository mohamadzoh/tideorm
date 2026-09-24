use crate::config::DatabaseType;
use crate::internal::{
    MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder, Token, Tokenizer, Value,
    inject_parameters, placeholder,
};

/// One marker per bound value, numbered from 1.
pub(crate) fn placeholders(db_type: DatabaseType, count: usize) -> Vec<String> {
    (1..=count)
        .map(|index| placeholder(db_type, index))
        .collect()
}

/// Render `sql` with every bound value written in as a literal, for display only.
///
/// The literals come from sea-query's own [`inject_parameters`], so they are
/// spelled exactly as the backend's query builder spells them. That function
/// indexes `params` by each placeholder it finds and panics on one without a
/// value — which a caller's raw fragment can easily carry — so a statement whose
/// placeholders do not line up with its values is returned unchanged.
pub(crate) fn inline_parameters(db_type: DatabaseType, sql: &str, params: &[Value]) -> String {
    if !placeholders_match_values(db_type, sql, params.len()) {
        return sql.to_string();
    }

    match db_type {
        DatabaseType::Postgres => inject_parameters(sql, params, &PostgresQueryBuilder),
        DatabaseType::MySQL | DatabaseType::MariaDB => {
            inject_parameters(sql, params, &MysqlQueryBuilder)
        }
        DatabaseType::SQLite => inject_parameters(sql, params, &SqliteQueryBuilder),
    }
}

/// Whether the placeholders `inject_parameters` will find in `sql` are exactly
/// the `bound` values.
///
/// The public tokenizer always scans like MySQL, and the backends only disagree
/// about a backslash escaping a quote. Generated SQL is backslash-free, so a
/// statement that contains one — only a raw fragment can — is left alone.
fn placeholders_match_values(db_type: DatabaseType, sql: &str, bound: usize) -> bool {
    if sql.contains('\\') {
        return false;
    }

    let mut tokens = Tokenizer::new(sql).iter().peekable();
    let mut seen = vec![false; bound];
    let mut positional = 0;

    while let Some(token) = tokens.next() {
        match (db_type, token) {
            (DatabaseType::Postgres, Token::Punctuation("$")) => {
                let Some(Token::Unquoted(number)) = tokens.peek() else {
                    continue;
                };
                let Ok(index) = number.parse::<usize>() else {
                    continue;
                };
                match index.checked_sub(1).and_then(|slot| seen.get_mut(slot)) {
                    Some(slot) => *slot = true,
                    None => return false,
                }
            }
            (
                DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite,
                Token::Punctuation("?"),
            ) => positional += 1,
            _ => {}
        }
    }

    match db_type {
        DatabaseType::Postgres => seen.into_iter().all(|used| used),
        DatabaseType::MySQL | DatabaseType::MariaDB | DatabaseType::SQLite => positional == bound,
    }
}

/// Shift every `$n` placeholder in `sql` up by `offset`.
///
/// Fragments rendered on their own number their placeholders from `$1`; spliced
/// behind `offset` values already bound in the surrounding statement, they have
/// to continue that numbering instead. Quoted literals, quoted identifiers,
/// comments and dollar-quoted strings are skipped, so a `$5` inside any of them
/// is left alone.
pub(crate) fn offset_postgres_placeholders(sql: &str, offset: usize) -> String {
    if offset == 0 {
        return sql.to_string();
    }

    #[derive(Clone, Copy)]
    enum ScanState {
        Normal,
        SingleQuoted { backslash_escapes: bool },
        DoubleQuoted,
        LineComment,
        BlockComment,
        DollarQuoted { tag_start: usize, tag_end: usize },
    }

    fn dollar_quote_tag_bounds(chars: &[char], start: usize) -> Option<usize> {
        if chars.get(start) != Some(&'$') {
            return None;
        }

        let mut index = start + 1;
        while index < chars.len() {
            match chars[index] {
                '$' => return Some(index),
                ch if ch == '_' || ch.is_ascii_alphanumeric() => index += 1,
                _ => return None,
            }
        }

        None
    }

    fn has_escape_string_prefix(chars: &[char], quote_index: usize) -> bool {
        if quote_index == 0 {
            return false;
        }

        let prefix = chars[quote_index - 1];
        if prefix != 'e' && prefix != 'E' {
            return false;
        }

        if quote_index == 1 {
            return true;
        }

        !matches!(chars[quote_index - 2], '_' | '$' | 'a'..='z' | 'A'..='Z' | '0'..='9')
    }

    let mut output = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let mut index = 0;
    let mut state = ScanState::Normal;

    while index < chars.len() {
        match state {
            ScanState::Normal => match chars[index] {
                '\'' => {
                    output.push(chars[index]);
                    state = ScanState::SingleQuoted {
                        backslash_escapes: has_escape_string_prefix(&chars, index),
                    };
                    index += 1;
                }
                '"' => {
                    output.push(chars[index]);
                    state = ScanState::DoubleQuoted;
                    index += 1;
                }
                '-' if chars.get(index + 1) == Some(&'-') => {
                    output.push(chars[index]);
                    output.push(chars[index + 1]);
                    state = ScanState::LineComment;
                    index += 2;
                }
                '/' if chars.get(index + 1) == Some(&'*') => {
                    output.push(chars[index]);
                    output.push(chars[index + 1]);
                    state = ScanState::BlockComment;
                    index += 2;
                }
                '$' => {
                    if let Some(tag_end) = dollar_quote_tag_bounds(&chars, index)
                        && (tag_end == index + 1 || !chars[index + 1].is_ascii_digit())
                    {
                        output.extend(chars[index..=tag_end].iter());
                        state = ScanState::DollarQuoted {
                            tag_start: index,
                            tag_end,
                        };
                        index = tag_end + 1;
                        continue;
                    }

                    let start = index + 1;
                    let mut end = start;
                    while end < chars.len() && chars[end].is_ascii_digit() {
                        end += 1;
                    }

                    if end > start {
                        let number: usize = chars[start..end]
                            .iter()
                            .collect::<String>()
                            .parse()
                            .unwrap_or(0);
                        if number > 0 {
                            output.push('$');
                            output.push_str(&(number + offset).to_string());
                            index = end;
                            continue;
                        }
                    }

                    output.push(chars[index]);
                    index += 1;
                }
                _ => {
                    output.push(chars[index]);
                    index += 1;
                }
            },
            ScanState::SingleQuoted { backslash_escapes } => {
                output.push(chars[index]);
                if backslash_escapes
                    && chars[index] == '\\'
                    && let Some(next) = chars.get(index + 1)
                {
                    output.push(*next);
                    index += 2;
                    continue;
                }
                if chars[index] == '\'' {
                    if chars.get(index + 1) == Some(&'\'') {
                        output.push(chars[index + 1]);
                        index += 2;
                        continue;
                    }
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::DoubleQuoted => {
                output.push(chars[index]);
                if chars[index] == '"' {
                    if chars.get(index + 1) == Some(&'"') {
                        output.push(chars[index + 1]);
                        index += 2;
                        continue;
                    }
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::LineComment => {
                output.push(chars[index]);
                if chars[index] == '\n' {
                    state = ScanState::Normal;
                }
                index += 1;
            }
            ScanState::BlockComment => {
                output.push(chars[index]);
                if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    output.push(chars[index + 1]);
                    state = ScanState::Normal;
                    index += 2;
                    continue;
                }
                index += 1;
            }
            ScanState::DollarQuoted { tag_start, tag_end } => {
                let tag_len = tag_end - tag_start + 1;
                if chars[index] == '$'
                    && chars.get(index..index + tag_len) == Some(&chars[tag_start..=tag_end])
                {
                    output.extend(chars[index..index + tag_len].iter());
                    state = ScanState::Normal;
                    index += tag_len;
                    continue;
                }

                output.push(chars[index]);
                index += 1;
            }
        }
    }

    output
}
