use super::{
    ConditionValue, Operator, OrGroup, QueryBuilder, QueryFragment, WhereCondition, db_sql,
};
use crate::error::{Error, Result};
use crate::model::Model;
use std::marker::PhantomData;

impl<M: Model> QueryBuilder<M> {
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
        }
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
            order_by: self.order_by.clone(),
            group_by: self.group_by.clone(),
            having_conditions: self.having_conditions.clone(),
            joins: self.joins.clone(),
            invalid_query_reason: self.invalid_query_reason.clone(),
            include_trashed: self.include_trashed,
            only_trashed: self.only_trashed,
        }
    }

    /// Apply a reusable fragment to the current query builder.
    pub fn apply(mut self, fragment: &QueryFragment<M>) -> Self {
        self.conditions.extend(fragment.conditions.clone());

        if self.order_by.is_empty() {
            self.order_by.extend(fragment.order_by.clone());
        }

        self.group_by.extend(fragment.group_by.clone());
        self.having_conditions
            .extend(fragment.having_conditions.clone());
        self.joins.extend(fragment.joins.clone());

        if self.invalid_query_reason.is_none() {
            self.invalid_query_reason = fragment.invalid_query_reason.clone();
        }

        if fragment.include_trashed {
            self.include_trashed = true;
        }
        if fragment.only_trashed {
            self.only_trashed = true;
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
