use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn ensure_mutation_query_is_safe(&self, operation: &str) -> Result<()> {
        if !self.joins.is_empty()
            || !self.group_by.is_empty()
            || !self.having_conditions.is_empty()
            || !self.unions.is_empty()
            || !self.ctes.is_empty()
            || !self.window_functions.is_empty()
            || self.select_columns.is_some()
            || !self.raw_select_expressions.is_empty()
            || !self.subquery_select_expressions.is_empty()
            || !self.order_by.is_empty()
            || self.limit_value.is_some()
            || self.offset_value.is_some()
        {
            return Err(Error::invalid_query(format!(
                "{} does not support SELECT/JOIN/ORDER/GROUP specific query modifiers",
                operation
            )));
        }

        Ok(())
    }

    fn has_explicit_mutation_filters(&self) -> bool {
        !self.conditions.is_empty()
            || self
                .or_groups
                .iter()
                .any(|group| group.condition_count() > 0)
    }

    pub(crate) fn ensure_mutation_has_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
                "{} requires at least one explicit filter; unfiltered bulk mutations are blocked",
                operation
            )))
        }
    }

    pub(crate) fn ensure_mutation_has_no_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Err(Error::invalid_query(format!(
                "{} does not accept WHERE filters; use delete() when you intend to target specific rows",
                operation
            )))
        } else {
            Ok(())
        }
    }
}
