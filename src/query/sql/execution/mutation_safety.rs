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
        let is_vacuous = |condition: &crate::query::WhereCondition| {
            crate::query::condition_is_vacuous(condition, |column| self.is_json_column(column))
        };
        self.clauses
            .conditions
            .iter()
            .any(|condition| !is_vacuous(condition))
            || self
                .clauses
                .or_groups
                .iter()
                .any(|group| group.is_restrictive_with(&is_vacuous))
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

    /// Refuse a bulk mutation whose condition cannot exclude a row.
    ///
    /// `ensure_mutation_has_explicit_filters` only proves filters were
    /// *declared* on the builder. This folds the condition they built, so one
    /// that collapsed to a constant TRUE (an empty `NOT IN`, a raw `1 = 1`, an
    /// OR with such a branch) cannot turn a targeted mutation into a
    /// full-table one. It judges the caller's filters, never the default
    /// live-rows scope, whose `deleted_at IS NULL` would leave a TRUE beside
    /// it unknown. With `trash_scope_counts` (`restore()` and
    /// `force_delete()`, as [`ensure_trash_mutation_has_filters`](Self::ensure_trash_mutation_has_filters)
    /// allows) `only_trashed()`'s scope counts too, so the trash can be
    /// restored or emptied whole; any other mutation needs a filter of its own
    /// inside the trash.
    pub(super) fn ensure_mutation_can_exclude_a_row(
        &self,
        db_type: DatabaseType,
        operation: &str,
        trash_scope_counts: bool,
    ) -> Result<()> {
        let condition = if trash_scope_counts && self.clauses.only_trashed {
            self.build_condition_for_db(db_type)
        } else {
            self.filter_condition(db_type)
        };
        if condition.is_trivially_true() {
            return Err(Error::query(format!(
                "{} rendered a WHERE clause that matches every row; unfiltered bulk mutations are blocked",
                operation
            )));
        }
        Ok(())
    }

    /// The WHERE body of a bulk mutation, its values bound in place, refused
    /// when [its condition cannot exclude a row](Self::ensure_mutation_can_exclude_a_row).
    pub(crate) fn mutation_where_bound(
        &self,
        db_type: DatabaseType,
        operation: &str,
        trash_scope_counts: bool,
    ) -> Result<BoundSql> {
        self.ensure_mutation_can_exclude_a_row(db_type, operation, trash_scope_counts)?;
        Self::render_condition(db_type, &self.build_condition_for_db(db_type))
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
