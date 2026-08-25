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
        self.conditions
            .iter()
            .any(|condition| !crate::query::condition_is_vacuous(condition))
            || self.or_groups.iter().any(OrGroup::is_restrictive)
    }

    pub(crate) fn ensure_mutation_has_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Ok(())
        } else {
            Err(Error::invalid_query(format!(
                "{} requires at least one explicit filter that can exclude a row; unfiltered bulk mutations are blocked",
                operation
            )))
        }
    }

    /// True when a rendered WHERE body cannot exclude any row.
    ///
    /// Covers both an empty body and the constant-true placeholders sea-query can
    /// emit (an empty `Condition::all()` lowers to `TRUE`; an empty `ne_all` set
    /// renders `1 = 1`). Parentheses and whitespace are stripped before matching,
    /// which is safe because only this closed set of literals is accepted — a real
    /// predicate never normalizes into it.
    fn is_unrestricted_where_body(where_sql: &str) -> bool {
        let mut normalized = String::with_capacity(where_sql.len());
        for character in where_sql.chars() {
            if character.is_whitespace() || character == '(' || character == ')' {
                continue;
            }
            normalized.push(character.to_ascii_uppercase());
        }

        matches!(normalized.as_str(), "" | "TRUE" | "1" | "1=1" | "TRUE=TRUE")
    }

    /// Reject a rendered WHERE body that does not actually restrict any row.
    ///
    /// `ensure_mutation_has_explicit_filters` only proves filters were *declared*
    /// on the builder. This checks what was actually rendered, so a predicate that
    /// collapsed away between builder and SQL cannot turn a targeted mutation into
    /// a full-table one.
    pub(crate) fn ensure_rendered_filter_is_restrictive(
        operation: &str,
        where_sql: &str,
    ) -> Result<()> {
        if Self::is_unrestricted_where_body(where_sql) {
            return Err(Error::invalid_query(format!(
                "{} rendered a WHERE clause that matches every row; unfiltered bulk mutations are blocked",
                operation
            )));
        }

        Ok(())
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
