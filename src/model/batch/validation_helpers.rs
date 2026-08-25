use super::*;

impl<M: Model> BatchUpdateBuilder<M> {
    pub(crate) fn validate_update_column(column: &str) -> Result<()> {
        let is_safe_identifier = {
            let mut chars = column.chars();
            matches!(chars.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
                && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        };

        if is_safe_identifier && M::column_from_str(column).is_some() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
                "unsafe update column '{}': batch updates require a known model field/column name using only ASCII letters, numbers, and underscores",
                column
            )))
        }
    }

    pub(crate) fn quote_update_column(
        column: &str,
        db_type: crate::config::DatabaseType,
    ) -> Result<String> {
        Self::validate_update_column(column)?;
        let canonical_column = M::canonical_column_name(column).unwrap_or(column);
        Ok(Self::quote_identifier(canonical_column, db_type))
    }

    pub(crate) fn quote_identifier(name: &str, db_type: crate::config::DatabaseType) -> String {
        quote_ident(db_type, name)
    }

    /// A batch update needs at least one filter that can actually exclude a row.
    ///
    /// Counting conditions is not enough: an empty candidate list for a negative
    /// membership test renders constant-true, so a caller whose filter list came
    /// back empty would silently rewrite the whole table. Shared with the
    /// `QueryBuilder` mutation terminals so both paths reject the same shapes.
    pub(crate) fn has_explicit_filters(&self) -> bool {
        self.conditions
            .iter()
            .any(|condition| !crate::query::condition_is_vacuous(condition))
    }

    pub(crate) fn ensure_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_filters() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
                "{} requires at least one explicit filter that can exclude a row; unfiltered bulk mutations are blocked",
                operation
            )))
        }
    }

    pub(crate) fn validate_json_path(path: &str) -> Result<Vec<&str>> {
        let stripped = path.strip_prefix("$.").ok_or_else(|| {
            Error::invalid_query(format!(
                "unsafe JSON path '{}': only $.field or $.field.subfield paths are supported",
                path
            ))
        })?;

        let segments: Vec<&str> = stripped.split('.').collect();
        if segments.is_empty()
            || segments.iter().any(|segment| {
                segment.is_empty()
                    || !segment
                        .chars()
                        .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
                    || segment
                        .chars()
                        .next()
                        .map(|ch| ch.is_ascii_digit())
                        .unwrap_or(true)
            })
        {
            return Err(Error::invalid_query(format!(
                "unsafe JSON path '{}': only simple identifier segments are supported",
                path
            )));
        }

        Ok(segments)
    }

    pub(crate) fn postgres_json_path_literal(segments: &[&str]) -> String {
        format!(
            "{{{}}}",
            segments
                .iter()
                .map(|segment| format!("\"{}\"", segment))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

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
}
