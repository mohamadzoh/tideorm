#![allow(missing_docs)]

use crate::columns::IntoColumnName;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident;
use crate::query::{LogicalOp, OrGroup, QueryBuilder};

use super::Model;

/// Builder for batch update operations.
pub struct BatchUpdateBuilder<M: Model> {
    _marker: std::marker::PhantomData<M>,
    updates: std::collections::HashMap<String, UpdateValue>,
    conditions: Vec<crate::query::WhereCondition>,
    returning: bool,
    limit_value: Option<u64>,
}

/// Value for batch update operations.
#[derive(Debug, Clone)]
pub enum UpdateValue {
    Value(serde_json::Value),
    UnsafeRaw(String),
    Increment(i64),
    Decrement(i64),
    Multiply(f64),
    Divide(f64),
    ArrayAppend(serde_json::Value),
    ArrayRemove(serde_json::Value),
    JsonSet(String, serde_json::Value),
    Coalesce(serde_json::Value),
}

impl<M: Model> BatchUpdateBuilder<M> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            updates: std::collections::HashMap::new(),
            conditions: Vec::new(),
            returning: false,
            limit_value: None,
        }
    }

    pub fn set(mut self, field: impl IntoColumnName, value: impl Into<serde_json::Value>) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::Value(value.into()),
        );
        self
    }

    pub fn set_trusted_raw(mut self, field: impl IntoColumnName, expression: &str) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::UnsafeRaw(expression.to_string()),
        );
        self
    }

    pub fn set_if(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
        condition: bool,
    ) -> Self {
        if condition {
            self.updates.insert(
                field.column_name().to_string(),
                UpdateValue::Value(value.into()),
            );
        }
        self
    }

    pub fn increment(mut self, field: impl IntoColumnName, by: i64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Increment(by));
        self
    }

    pub fn decrement(mut self, field: impl IntoColumnName, by: i64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Decrement(by));
        self
    }

    pub fn multiply(mut self, field: impl IntoColumnName, by: f64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Multiply(by));
        self
    }

    pub fn divide(mut self, field: impl IntoColumnName, by: f64) -> Self {
        self.updates
            .insert(field.column_name().to_string(), UpdateValue::Divide(by));
        self
    }

    pub fn array_append(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::ArrayAppend(value.into()),
        );
        self
    }

    pub fn array_remove(
        mut self,
        field: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::ArrayRemove(value.into()),
        );
        self
    }

    pub fn json_set(
        mut self,
        field: impl IntoColumnName,
        path: &str,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::JsonSet(path.to_string(), value.into()),
        );
        self
    }

    pub fn coalesce(
        mut self,
        field: impl IntoColumnName,
        default: impl Into<serde_json::Value>,
    ) -> Self {
        self.updates.insert(
            field.column_name().to_string(),
            UpdateValue::Coalesce(default.into()),
        );
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.limit_value = Some(n);
        self
    }

    pub fn returning(mut self) -> Self {
        self.returning = true;
        self
    }

    pub fn where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_not(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_gte(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Gte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_lte(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Lte,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    pub fn where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::NotIn,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    pub fn where_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    pub fn where_not_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::IsNotNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    pub fn where_between(
        mut self,
        column: impl IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Between,
            value: crate::query::ConditionValue::Range(min.into(), max.into()),
        });
        self
    }

    pub fn where_like(mut self, column: impl IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    pub fn where_contains(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_starts_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn where_ends_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: column.column_name().to_string(),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn or_where_eq(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Eq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_not(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::NotEq,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_gt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Gt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_lt(
        mut self,
        column: impl IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Lt,
            value: crate::query::ConditionValue::Single(value.into()),
        });
        self
    }

    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::In,
            value: crate::query::ConditionValue::List(
                values.into_iter().map(|v| v.into()).collect(),
            ),
        });
        self
    }

    pub fn or_where_null(mut self, column: impl IntoColumnName) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::IsNull,
            value: crate::query::ConditionValue::None,
        });
        self
    }

    pub fn or_where_like(mut self, column: impl IntoColumnName, pattern: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::Like,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(
                pattern.to_string(),
            )),
        });
        self
    }

    pub fn or_where_contains(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn or_where_starts_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "{}%",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    pub fn or_where_ends_with(mut self, column: impl IntoColumnName, value: &str) -> Self {
        self.conditions.push(crate::query::WhereCondition {
            column: format!("__OR__{}", column.column_name()),
            operator: crate::query::Operator::LikeEscaped,
            value: crate::query::ConditionValue::Single(serde_json::Value::String(format!(
                "%{}",
                crate::columns::escape_like_literal(value)
            ))),
        });
        self
    }

    fn validate_update_column(column: &str) -> Result<()> {
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

    fn quote_update_column(column: &str, db_type: crate::config::DatabaseType) -> Result<String> {
        Self::validate_update_column(column)?;
        Ok(Self::quote_identifier(column, db_type))
    }

    fn quote_identifier(name: &str, db_type: crate::config::DatabaseType) -> String {
        quote_ident(db_type, name)
    }

    fn has_explicit_filters(&self) -> bool {
        !self.conditions.is_empty()
    }

    fn ensure_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_filters() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
                "{} requires at least one explicit filter; unfiltered bulk mutations are blocked",
                operation
            )))
        }
    }

    fn validate_json_path(path: &str) -> Result<Vec<&str>> {
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

    fn postgres_json_path_literal(segments: &[&str]) -> String {
        format!(
            "{{{}}}",
            segments
                .iter()
                .map(|segment| format!("\"{}\"", segment))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    fn offset_postgres_placeholders(sql: &str, offset: usize) -> String {
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
                        if let Some(tag_end) = dollar_quote_tag_bounds(&chars, index) {
                            if tag_end == index + 1 || !chars[index + 1].is_ascii_digit() {
                                output.extend(chars[index..=tag_end].iter());
                                state = ScanState::DollarQuoted {
                                    tag_start: index,
                                    tag_end,
                                };
                                index = tag_end + 1;
                                continue;
                            }
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
                    if backslash_escapes && chars[index] == '\\' {
                        if let Some(next) = chars.get(index + 1) {
                            output.push(*next);
                            index += 2;
                            continue;
                        }
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

    fn build_assignment_sql(
        column: &str,
        value: &UpdateValue,
        db_type: crate::config::DatabaseType,
        params: &mut Vec<crate::internal::Value>,
    ) -> Result<String> {
        let col = Self::quote_update_column(column, db_type)?;

        match value {
            UpdateValue::Value(value) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(value),
                );
                Ok(format!("{} = {}", col, placeholder))
            }
            UpdateValue::UnsafeRaw(expression) => Ok(format!("{} = {}", col, expression)),
            UpdateValue::Increment(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::BigInt(Some(*by)),
                );
                Ok(format!("{} = {} + {}", col, col, placeholder))
            }
            UpdateValue::Decrement(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::BigInt(Some(*by)),
                );
                Ok(format!("{} = {} - {}", col, col, placeholder))
            }
            UpdateValue::Multiply(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::Double(Some(*by)),
                );
                Ok(format!("{} = {} * {}", col, col, placeholder))
            }
            UpdateValue::Divide(by) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::Double(Some(*by)),
                );
                Ok(format!("{} = {} / {}", col, col, placeholder))
            }
            UpdateValue::ArrayAppend(value) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(value),
                );
                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => {
                        format!("{} = array_append({}, {})", col, col, placeholder)
                    }
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!("{} = JSON_ARRAY_APPEND({}, '$', {})", col, col, placeholder)
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!("{} = json_insert({}, '$[#]', {})", col, col, placeholder)
                    }
                })
            }
            UpdateValue::ArrayRemove(value) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(value),
                );
                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => {
                        format!("{} = array_remove({}, {})", col, col, placeholder)
                    }
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!(
                            "{} = JSON_REMOVE({}, JSON_UNQUOTE(JSON_SEARCH({}, 'one', {})))",
                            col, col, col, placeholder
                        )
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!(
                            "{} = (SELECT json_group_array(value) FROM json_each({}) WHERE value != {})",
                            col, col, placeholder
                        )
                    }
                })
            }
            UpdateValue::JsonSet(path, value) => {
                let segments = Self::validate_json_path(path)?;
                let path_placeholder = match db_type {
                    crate::config::DatabaseType::Postgres => crate::internal::push_param(
                        db_type,
                        params,
                        crate::internal::Value::String(Some(Self::postgres_json_path_literal(
                            &segments,
                        ))),
                    ),
                    crate::config::DatabaseType::MySQL
                    | crate::config::DatabaseType::MariaDB
                    | crate::config::DatabaseType::SQLite => crate::internal::push_param(
                        db_type,
                        params,
                        crate::internal::Value::String(Some(path.clone())),
                    ),
                };
                let json_text = serde_json::to_string(value)?;
                let value_placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::Value::String(Some(json_text)),
                );

                Ok(match db_type {
                    crate::config::DatabaseType::Postgres => format!(
                        "{} = jsonb_set({}, {}::text[], CAST({} AS jsonb))",
                        col, col, path_placeholder, value_placeholder
                    ),
                    crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB => {
                        format!(
                            "{} = JSON_SET({}, {}, CAST({} AS JSON))",
                            col, col, path_placeholder, value_placeholder
                        )
                    }
                    crate::config::DatabaseType::SQLite => {
                        format!(
                            "{} = json_set({}, {}, json({}))",
                            col, col, path_placeholder, value_placeholder
                        )
                    }
                })
            }
            UpdateValue::Coalesce(default) => {
                let placeholder = crate::internal::push_param(
                    db_type,
                    params,
                    crate::internal::json_to_db_value(default),
                );
                Ok(format!("{} = COALESCE({}, {})", col, col, placeholder))
            }
        }
    }

    fn build_set_clause_with_params_for_db(
        &self,
        db_type: crate::config::DatabaseType,
    ) -> Result<(Vec<String>, Vec<crate::internal::Value>)> {
        let mut params = Vec::new();
        let mut set_parts = Vec::with_capacity(self.updates.len());

        for (column, value) in &self.updates {
            set_parts.push(Self::build_assignment_sql(
                column,
                value,
                db_type,
                &mut params,
            )?);
        }

        Ok((set_parts, params))
    }

    fn ensure_backend_supports_returning(db_type: crate::config::DatabaseType) -> Result<()> {
        if !db_type.supports_returning() {
            return Err(Error::query(format!(
                "{} does not support RETURNING clause",
                db_type
            )));
        }

        Ok(())
    }

    fn build_where_query(&self) -> QueryBuilder<M> {
        let mut query = QueryBuilder::new().with_trashed();
        let mut or_conditions = Vec::new();

        for condition in &self.conditions {
            if let Some(column) = condition.column.strip_prefix("__OR__") {
                let mut or_condition = condition.clone();
                or_condition.column = column.to_string();
                or_conditions.push(or_condition);
            } else {
                query.conditions.push(condition.clone());
            }
        }

        if !or_conditions.is_empty() {
            query.or_groups.push(OrGroup {
                conditions: or_conditions,
                nested_groups: Vec::new(),
                combine_with: LogicalOp::Or,
            });
        }

        query
    }

    pub async fn execute(self) -> Result<u64> {
        if self.updates.is_empty() {
            return Ok(0);
        }

        self.ensure_explicit_filters("update")?;

        let _ = self.returning;

        let db_type = crate::database::require_db()?.backend();
        let (set_parts, mut params) = self.build_set_clause_with_params_for_db(db_type)?;

        let query = self.build_where_query();
        let (mut where_sql, where_params) = query.build_where_clause_with_condition_for_db(db_type);

        if matches!(db_type, crate::config::DatabaseType::Postgres) {
            where_sql = Self::offset_postgres_placeholders(&where_sql, params.len());
        }
        params.extend(where_params);

        let table = Self::quote_identifier(M::table_name(), db_type);
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !where_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }

        if let Some(limit) = self.limit_value {
            if matches!(
                db_type,
                crate::config::DatabaseType::MySQL | crate::config::DatabaseType::MariaDB
            ) {
                sql.push_str(&format!(" LIMIT {}", limit));
            }
        }

        let rows_affected = crate::Database::execute_with_params(&sql, params).await?;
        if rows_affected > 0 {
            crate::QueryCache::global().invalidate_model(M::table_name());
        }
        Ok(rows_affected)
    }

    pub async fn execute_returning(self) -> Result<Vec<M>> {
        if self.updates.is_empty() {
            return Ok(vec![]);
        }

        self.ensure_explicit_filters("update")?;

        let db_type = crate::database::require_db()?.backend();
        Self::ensure_backend_supports_returning(db_type)?;

        let (set_parts, mut params) = self.build_set_clause_with_params_for_db(db_type)?;

        let query = self.build_where_query();
        let (mut where_sql, where_params) = query.build_where_clause_with_condition_for_db(db_type);

        if matches!(db_type, crate::config::DatabaseType::Postgres) {
            where_sql = Self::offset_postgres_placeholders(&where_sql, params.len());
        }
        params.extend(where_params);

        let table = Self::quote_identifier(M::table_name(), db_type);
        let mut sql = format!("UPDATE {} SET {}", table, set_parts.join(", "));

        if !where_sql.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_sql);
        }

        sql.push_str(" RETURNING *");

        let models = crate::Database::raw_with_params::<M>(&sql, params).await?;
        if !models.is_empty() {
            crate::QueryCache::global().invalidate_model(M::table_name());
        }
        Ok(models)
    }
}

impl<M: Model> Default for BatchUpdateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../testing/model_batch_tests.rs"]
mod tests;
