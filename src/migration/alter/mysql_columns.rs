//! Reading and restating a MySQL or MariaDB column's declaration, which
//! `MODIFY COLUMN` writes whole.

use crate::config::DatabaseType;
use crate::internal::sql_safety::quote_ident;

/// A MySQL column's declaration, as `SHOW CREATE TABLE` writes it.
#[derive(Debug, Default, PartialEq)]
pub(super) struct KeptAttributes {
    /// Its type: `int(11)`, `varchar(20)`, `enum('a','b')`.
    pub(super) column_type: String,
    /// Its `CHARACTER SET` and `COLLATE` clauses, which `SHOW CREATE TABLE`
    /// writes only where they differ from the table's defaults, as written.
    pub(super) character: String,
    /// Everything after them: `NOT NULL DEFAULT '5'`, `AUTO_INCREMENT`,
    /// `DEFAULT NULL COMMENT '..'`.
    pub(super) rest: String,
}

/// Whether a MySQL column of type `type_sql` holds text, and so takes a
/// character set and a collation.
pub(super) fn holds_text(type_sql: &str) -> bool {
    let type_sql = type_sql.trim().to_ascii_uppercase();
    [
        "CHAR",
        "VARCHAR",
        "NCHAR",
        "NVARCHAR",
        "NATIONAL",
        "TINYTEXT",
        "TEXT",
        "MEDIUMTEXT",
        "LONGTEXT",
        "ENUM",
        "SET(",
        "SET (",
    ]
    .iter()
    .any(|prefix| type_sql.starts_with(prefix))
}

/// The words of a MySQL column definition, as byte ranges of `text`: a quoted
/// literal (with its doubled quotes) or a parenthesized group is one word,
/// and any other word ends at whitespace or before a `(`.
fn definition_words(text: &str) -> Vec<(usize, usize)> {
    let mut words = Vec::new();
    let mut position = 0;
    while let Some(offset) = text[position..].find(|character: char| !character.is_whitespace()) {
        let start = position + offset;
        let rest = &text[start..];
        let first = rest.chars().next().unwrap_or(' ');
        let length = match first {
            '\'' | '"' | '`' => {
                let mut end = 1;
                loop {
                    match rest[end..].find(first) {
                        // A doubled quote is one quote inside the literal.
                        Some(found) if rest[end + found + 1..].starts_with(first) => {
                            end += found + 2;
                        }
                        Some(found) => break end + found + 1,
                        None => break rest.len(),
                    }
                }
            }
            '(' => {
                let mut depth = 0usize;
                rest.char_indices()
                    .find(|&(_, character)| {
                        match character {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        depth == 0
                    })
                    .map_or(rest.len(), |(index, _)| index + 1)
            }
            _ => rest
                .find(|character: char| character.is_whitespace() || character == '(')
                .unwrap_or(rest.len()),
        };
        words.push((start, start + length));
        position = start + length;
    }
    words
}

/// What `create_table`, a MySQL or MariaDB `SHOW CREATE TABLE`, declares for
/// `column`: its type, its character set and collation, and after them
/// `NOT NULL DEFAULT '5'`, `NOT NULL AUTO_INCREMENT`,
/// `DEFAULT NULL COMMENT '..'`. `None` when the table has no such column.
pub(super) fn mysql_column_attributes(create_table: &str, column: &str) -> Option<KeptAttributes> {
    // The first word of a column's definition that is not part of its type.
    const ATTRIBUTES: [&str; 17] = [
        "NOT",
        "NULL",
        "DEFAULT",
        "AUTO_INCREMENT",
        "ON",
        "COMMENT",
        "GENERATED",
        "AS",
        "INVISIBLE",
        "VISIBLE",
        "CHECK",
        "COLUMN_FORMAT",
        "STORAGE",
        "SRID",
        "PRIMARY",
        "UNIQUE",
        "REFERENCES",
    ];

    let quoted = quote_ident(DatabaseType::MySQL, column);
    let definition = create_table.lines().map(str::trim).find_map(|line| {
        let head = line.get(..quoted.len())?;
        let rest = line.get(quoted.len()..)?;
        (head.eq_ignore_ascii_case(&quoted) && rest.starts_with(' ')).then_some(rest)
    })?;
    let definition = definition.trim().trim_end_matches(',');
    let mut character: Vec<&str> = Vec::new();
    // Where the type ends: at the first word that is not part of it.
    let mut type_end = None;
    // How many of the next words belong to a CHARACTER SET or COLLATE clause.
    let mut clause_words = 0;
    let mut attributes_start = definition.len();

    for (start, end) in definition_words(definition) {
        let word = &definition[start..end];
        if clause_words > 0 {
            character.push(word);
            clause_words -= 1;
            continue;
        }
        clause_words = if word.eq_ignore_ascii_case("CHARACTER") {
            2
        } else if word.eq_ignore_ascii_case("CHARSET") || word.eq_ignore_ascii_case("COLLATE") {
            1
        } else {
            0
        };
        if clause_words > 0 {
            type_end.get_or_insert(start);
            character.push(word);
        } else if ATTRIBUTES
            .iter()
            .any(|attribute| word.eq_ignore_ascii_case(attribute))
        {
            attributes_start = start;
            break;
        }
    }

    let type_end = type_end.unwrap_or(attributes_start);
    Some(KeptAttributes {
        column_type: definition[..type_end].trim_end().to_string(),
        character: character.join(" "),
        rest: definition[attributes_start..].to_string(),
    })
}

/// `rest`, the attributes a MySQL column declares after its type, with its
/// nullability `nullable` and its default `default` (`Some(None)` drops it)
/// where they are given, written first as `SHOW CREATE TABLE` writes them.
/// A `NOT NULL` column defaults to no NULL.
pub(super) fn restate_attributes(
    rest: &str,
    nullable: Option<bool>,
    default: Option<&Option<String>>,
) -> String {
    let words = definition_words(rest);
    let is = |index: usize, keyword: &str| {
        words
            .get(index)
            .is_some_and(|&(start, end)| rest[start..end].eq_ignore_ascii_case(keyword))
    };
    let mut stored_nullable = None;
    let mut stored_default = None;
    let mut kept = vec![true; words.len()];
    let mut index = 0;
    while index < words.len() {
        if is(index, "NOT") && is(index + 1, "NULL") {
            stored_nullable = Some(false);
            kept[index..index + 2].fill(false);
            index += 2;
        } else if is(index, "NULL") {
            stored_nullable = Some(true);
            kept[index] = false;
            index += 1;
        } else if is(index, "DEFAULT") && index + 1 < words.len() {
            let mut last = index + 1;
            // A function's arguments follow its name: CURRENT_TIMESTAMP(6).
            if let (Some(&(_, value_end)), Some(&(next_start, _))) =
                (words.get(last), words.get(last + 1))
                && next_start == value_end
                && rest[next_start..].starts_with('(')
            {
                last += 1;
            }
            stored_default = Some(rest[words[index + 1].0..words[last].1].to_string());
            kept[index..=last].fill(false);
            index = last + 1;
        } else {
            index += 1;
        }
    }

    let nullable = nullable.or(stored_nullable);
    let default = match default {
        Some(default) => default.clone(),
        None => stored_default,
    }
    .filter(|default| nullable != Some(false) || !default.eq_ignore_ascii_case("NULL"));

    let mut attributes: Vec<String> = Vec::new();
    match nullable {
        Some(true) => attributes.push("NULL".to_string()),
        Some(false) => attributes.push("NOT NULL".to_string()),
        None => {}
    }
    if let Some(default) = default {
        attributes.push(format!("DEFAULT {default}"));
    }
    // The other words as written, with the text between two that stood
    // together.
    let mut others = String::new();
    let mut previous: Option<usize> = None;
    for (index, &(start, end)) in words.iter().enumerate() {
        if !kept[index] {
            continue;
        }
        match previous {
            Some(previous) if previous + 1 == index => {
                others.push_str(&rest[words[previous].1..start]);
            }
            Some(_) => others.push(' '),
            None => {}
        }
        others.push_str(&rest[start..end]);
        previous = Some(index);
    }
    if !others.is_empty() {
        attributes.push(others);
    }
    attributes.join(" ")
}
