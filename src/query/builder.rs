use super::{
    ConditionValue, Operator, OrGroup, QueryBuilder, QueryFragment, WhereCondition, db_sql,
};
use crate::error::{Error, Result};
use crate::model::Model;
use std::marker::PhantomData;

impl<M: Model> QueryBuilder<M> {
    fn split_select_alias(value: &str) -> &str {
        let trimmed = value.trim();
        let lowered = trimmed.to_ascii_lowercase();
        if let Some(index) = lowered.find(" as ") {
            trimmed[..index].trim()
        } else {
            trimmed
        }
    }

    fn simple_column_reference(value: &str) -> Option<(&str, &str)> {
        let value = Self::split_select_alias(value);
        if value.is_empty()
            || value.starts_with('"')
            || value.ends_with('"')
            || value.starts_with('`')
            || value.ends_with('`')
            || value.contains('(')
            || value.contains(')')
            || value.contains('*')
            || value.contains(' ')
        {
            return None;
        }

        match value.split_once('.') {
            Some((table, column))
                if !table.is_empty() && !column.is_empty() && !column.contains('.') =>
            {
                Some((table, column))
            }
            Some(_) => None,
            None => Some(("", value)),
        }
    }

    fn validate_model_column_reference(kind: &str, value: &str) -> std::result::Result<(), String> {
        let Some((table, column)) = Self::simple_column_reference(value) else {
            return Ok(());
        };

        let reference = if table.is_empty() {
            column.to_string()
        } else {
            format!("{}.{}", table, column)
        };
        db_sql::validate_identifier_reference(kind, &reference)?;

        if table.is_empty() || table == M::table_name() {
            if M::column_names().contains(&column) {
                Ok(())
            } else {
                Err(format!(
                    "unknown {} '{}' for model '{}'; known columns: {}",
                    kind,
                    reference,
                    M::table_name(),
                    M::column_names().join(", ")
                ))
            }
        } else {
            Ok(())
        }
    }

    fn validate_condition(condition: &WhereCondition) -> std::result::Result<(), String> {
        match (&condition.operator, &condition.value) {
            (Operator::Raw, ConditionValue::RawExpr(raw_sql)) => {
                let kind = if condition.column.is_empty() {
                    "WHERE raw SQL"
                } else {
                    "WHERE raw column expression"
                };
                db_sql::validate_raw_sql_fragment(kind, raw_sql)
            }
            (Operator::SubqueryIn, ConditionValue::Subquery(query_sql))
            | (Operator::SubqueryNotIn, ConditionValue::Subquery(query_sql)) => {
                db_sql::validate_subquery_sql(query_sql)
            }
            _ => Ok(()),
        }?;

        if !condition.column.is_empty() {
            Self::validate_model_column_reference("WHERE column", &condition.column)?;
        }

        Ok(())
    }

    fn validate_or_group(group: &OrGroup) -> std::result::Result<(), String> {
        for condition in &group.conditions {
            Self::validate_condition(condition)?;
        }

        for nested_group in &group.nested_groups {
            Self::validate_or_group(nested_group)?;
        }

        Ok(())
    }

    fn validate_query_fragments(&self) -> Result<()> {
        for condition in &self.conditions {
            Self::validate_condition(condition).map_err(Error::invalid_query)?;
        }

        for group in &self.or_groups {
            Self::validate_or_group(group).map_err(Error::invalid_query)?;
        }

        for (column, _) in &self.order_by {
            Self::validate_model_column_reference("ORDER BY column", column)
                .map_err(Error::invalid_query)?;
        }

        for column in &self.group_by {
            Self::validate_model_column_reference("GROUP BY column", column)
                .map_err(Error::invalid_query)?;
        }

        if let Some(columns) = &self.select_columns {
            for column in columns {
                Self::validate_model_column_reference("SELECT column", column)
                    .map_err(Error::invalid_query)?;
            }
        }

        Ok(())
    }

    /// Create a new query builder
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
            database: None,
            conditions: Vec::new(),
            or_groups: Vec::new(),
            order_by: Vec::new(),
            limit_value: None,
            offset_value: None,
            select_columns: None,
            raw_select_expressions: Vec::new(),
            include_trashed: false,
            only_trashed: false,
            joins: Vec::new(),
            invalid_query_reason: None,
            group_by: Vec::new(),
            having_conditions: Vec::new(),
            unions: Vec::new(),
            window_functions: Vec::new(),
            ctes: Vec::new(),
            cache_options: None,
            cache_key: None,
        }
    }

    pub(crate) fn with_database(mut self, database: crate::database::Database) -> Self {
        self.database = Some(database);
        self
    }

    pub(super) fn current_db(&self) -> Result<crate::database::Database> {
        if let Some(database) = &self.database {
            Ok(database.clone())
        } else {
            crate::database::__current_db()
        }
    }

    /// Consolidate the current query clauses into a reusable fragment.
    pub fn consolidate(&self) -> QueryFragment<M> {
        QueryFragment {
            _marker: PhantomData,
            conditions: self.conditions.clone(),
            or_groups: self.or_groups.clone(),
            order_by: self.order_by.clone(),
            limit_value: self.limit_value,
            offset_value: self.offset_value,
            select_columns: self.select_columns.clone(),
            raw_select_expressions: self.raw_select_expressions.clone(),
            group_by: self.group_by.clone(),
            having_conditions: self.having_conditions.clone(),
            joins: self.joins.clone(),
            unions: self.unions.clone(),
            window_functions: self.window_functions.clone(),
            ctes: self.ctes.clone(),
            cache_options: self.cache_options.clone(),
            cache_key: self.cache_key.clone(),
            invalid_query_reason: self.invalid_query_reason.clone(),
            include_trashed: self.include_trashed,
            only_trashed: self.only_trashed,
        }
    }

    /// Apply a reusable fragment to the current query builder.
    pub fn apply(mut self, fragment: &QueryFragment<M>) -> Self {
        self.conditions.extend(fragment.conditions.clone());
        self.or_groups.extend(fragment.or_groups.clone());

        if self.order_by.is_empty() {
            self.order_by.extend(fragment.order_by.clone());
        }

        if self.limit_value.is_none() {
            self.limit_value = fragment.limit_value;
        }

        if self.offset_value.is_none() {
            self.offset_value = fragment.offset_value;
        }

        if self.select_columns.is_none() {
            self.select_columns = fragment.select_columns.clone();
        }

        self.raw_select_expressions
            .extend(fragment.raw_select_expressions.clone());

        self.group_by.extend(fragment.group_by.clone());
        self.having_conditions
            .extend(fragment.having_conditions.clone());
        self.joins.extend(fragment.joins.clone());
        self.unions.extend(fragment.unions.clone());
        self.window_functions
            .extend(fragment.window_functions.clone());
        self.ctes.extend(fragment.ctes.clone());

        if self.cache_options.is_none() {
            self.cache_options = fragment.cache_options.clone();
        }

        if self.cache_key.is_none() {
            self.cache_key = fragment.cache_key.clone();
        }

        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = fragment.invalid_query_reason.clone();
        }

        if fragment.only_trashed {
            self.only_trashed = true;
            self.include_trashed = false;
        } else if fragment.include_trashed {
            self.include_trashed = true;
            self.only_trashed = false;
        }

        self
    }

    pub(super) fn invalidate_query(&mut self, reason: String) {
        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = Some(reason);
        }
    }

    pub(super) fn ensure_query_is_valid(&self) -> Result<()> {
        if let Some(reason) = &self.invalid_query_reason {
            return Err(Error::invalid_query(reason.clone()));
        }

        self.validate_query_fragments()?;

        Ok(())
    }

    pub(super) fn validate_join_clause(
        table: &str,
        alias: Option<&str>,
        left_column: &str,
        right_column: &str,
    ) -> std::result::Result<(), String> {
        db_sql::validate_identifier("JOIN table", table)?;

        if let Some(alias) = alias {
            db_sql::validate_identifier("JOIN alias", alias)?;
        }

        db_sql::validate_join_column(left_column)?;
        db_sql::validate_join_column(right_column)?;
        Ok(())
    }
}
