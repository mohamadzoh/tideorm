//! Split the exported client script without splitting function bodies or literals.
#[allow(dead_code)]
#[path = "../../src/internal/sql_lexer.rs"]
mod lexer;

pub fn statements(sql: &str, mysql: bool) -> Vec<&str> {
    let mut remaining = sql;
    let mut delimiter = ";";
    let mut result = Vec::new();
    while !remaining.trim().is_empty() {
        remaining = remaining.trim_start();
        if let Some(directive) = remaining.strip_prefix("DELIMITER ") {
            let end = directive.find('\n').unwrap_or(directive.len());
            delimiter = directive[..end].trim();
            remaining = &directive[end..];
            continue;
        }
        let end = if delimiter != ";" {
            remaining.find(delimiter)
        } else if mysql {
            let mut pos = 0;
            let mut boundary = None;
            while pos < remaining.len() {
                let rest = &remaining[pos..];
                let ch = rest.chars().next().unwrap();
                if matches!(ch, '\'' | '"' | '`') {
                    pos += lexer::quoted_len(rest, ch, ch != '`');
                } else if rest.starts_with("/*") {
                    pos += lexer::block_comment_len(rest, false);
                } else if rest.starts_with("--") || ch == '#' {
                    pos += rest.find('\n').unwrap_or(rest.len());
                } else if ch == ';' {
                    boundary = Some(pos);
                    break;
                } else {
                    pos += ch.len_utf8();
                }
            }
            boundary
        } else {
            lexer::tokens(remaining)
                .find(|token| token.kind == lexer::Kind::Symbol && token.text == ";")
                .map(|token| token.start)
        };
        if let Some(end) = end {
            result.push(&remaining[..end]);
            remaining = &remaining[end + delimiter.len()..];
        } else {
            result.push(remaining);
            break;
        }
    }
    result
}
