use super::*;

impl<M: Model> QueryBuilder<M> {
    pub(crate) fn ensure_mutation_query_is_safe(&self, operation: &str) -> Result<()> {
        match self.mutation_blocker() {
            Some(part) => Err(Error::query(format!(
                "{} keeps a query's filters and scope only; {} cannot be part of it",
                operation, part
            ))),
            None => Ok(()),
        }
    }

    fn has_explicit_mutation_filters(&self) -> bool {
        self.clauses
            .conditions
            .iter()
            .any(|condition| !crate::query::condition_is_vacuous(condition))
            || self.clauses.or_groups.iter().any(OrGroup::is_restrictive)
    }

    pub(crate) fn ensure_mutation_has_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Ok(())
        } else {
            Err(Error::query(format!(
                "{} requires at least one explicit filter that can exclude a row; unfiltered bulk mutations are blocked",
                operation
            )))
        }
    }

    /// [`ensure_mutation_has_explicit_filters`](Self::ensure_mutation_has_explicit_filters)
    /// for `restore()` and `force_delete()`, where a caller's `only_trashed()`
    /// counts: it confines the statement to the trash, so restoring all of it
    /// or emptying it needs no other filter. A model without soft delete has no
    /// trash, and there the scope would be dropped and the statement would
    /// reach live rows, so `only_trashed()` is refused.
    pub(crate) fn ensure_trash_mutation_has_filters(&self, operation: &str) -> Result<()> {
        if self.clauses.only_trashed {
            if !M::soft_delete_enabled() {
                return Err(Error::query(format!(
                    "{} with only_trashed() on '{}', which has no soft delete and so no trash",
                    operation,
                    M::table_name()
                )));
            }
            return Ok(());
        }
        self.ensure_mutation_has_explicit_filters(operation)
    }

    /// True when a rendered WHERE body cannot exclude any row.
    ///
    /// Covers both an empty body and the constant-true placeholders sea-query can
    /// emit (an empty `Condition::all()` lowers to `TRUE`; SQLite's containment
    /// of an empty array renders `1 = 1`). Parentheses and whitespace are stripped before matching,
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
            return Err(Error::query(format!(
                "{} rendered a WHERE clause that matches every row; unfiltered bulk mutations are blocked",
                operation
            )));
        }

        Ok(())
    }

    pub(crate) fn ensure_mutation_has_no_explicit_filters(&self, operation: &str) -> Result<()> {
        if self.has_explicit_mutation_filters() {
            Err(Error::query(format!(
                "{} does not accept WHERE filters; use delete() when you intend to target specific rows",
                operation
            )))
        } else {
            Ok(())
        }
    }
}
