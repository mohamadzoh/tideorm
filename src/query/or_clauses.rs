use super::{OrBranchBuilder, OrGroup, QueryBuilder, WhereCondition};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Add an OR group built by `f`, ANDed with the query's other filters.
    #[must_use]
    pub fn or_where<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let group = f(OrGroup::new());
        if !group.is_empty() {
            self.clauses.or_groups.push(group);
        }
        self
    }

    /// Start building a fluent OR expression with chained AND conditions.
    pub fn begin_or(self) -> OrBranchBuilder<M> {
        OrBranchBuilder::new(self)
    }

    /// Add `condition` to the one OR group the `or_where_*` calls share,
    /// creating it among the query's groups on first use.
    pub(crate) fn push_or_condition(mut self, condition: WhereCondition) -> Self {
        match self
            .clauses
            .simple_or_group
            .and_then(|index| self.clauses.or_groups.get_mut(index))
        {
            Some(group) => group.conditions.push(condition),
            None => {
                let mut group = OrGroup::new();
                group.conditions.push(condition);
                self.clauses.simple_or_group = Some(self.clauses.or_groups.len());
                self.clauses.or_groups.push(group);
            }
        }
        self
    }
}

crate::query::condition_methods! {
    impl[M: Model] QueryBuilder<M> {
        or_where + raw => push_or_condition,
            "All `or_where_*` calls on a query join one OR group, ANDed with the rest of the query: `.where_eq(\"a\", 1).or_where_eq(\"b\", 2).or_where_eq(\"c\", 3)` matches `a = 1 AND (b = 2 OR c = 3)`.";
    }
}
