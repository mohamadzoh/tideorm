use super::{OrBranchBuilder, OrGroup, QueryBuilder, WhereCondition};
use crate::model::Model;

impl<M: Model> QueryBuilder<M> {
    /// Match rows that meet any of the conditions `f` adds, ANDed with the
    /// query's other filters: `.where_eq(a).where_any(|g| g.where_eq(b).where_eq(c))`
    /// is `a AND (b OR c)`. Each call adds a group of its own.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().where_any(|q| q.where_eq(User::ACTIVE, true).where_eq(User::NAME, "Ada")).validate().unwrap();
    /// ```
    pub fn where_any<F>(self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        self.push_group(f(OrGroup::new()))
    }

    /// Match rows that meet all of the conditions `f` adds, as one group
    /// ANDed with the query's other filters. Its use is nesting:
    /// `where_all(|g| g.where_eq(a).nested_or(|o| o.where_eq(b).where_eq(c)))`
    /// is `(a AND (b OR c))`, which [`where_any`](Self::where_any) can then
    /// hold a group of as well.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query()
    ///     .where_all(|q| q.where_eq(User::ACTIVE, true).nested_or(|o| o.where_eq(User::NAME, "Ada").where_eq(User::NAME, "Grace")))
    ///     .validate()
    ///     .unwrap();
    /// ```
    pub fn where_all<F>(self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        self.push_group(f(OrGroup::and_group()))
    }

    /// Add `group` among the query's groups unless it holds no condition.
    fn push_group(mut self, group: OrGroup) -> Self {
        if !group.is_empty() {
            self.clauses.or_groups.push(group);
        }
        self
    }

    /// Start building a fluent OR expression with chained AND conditions.
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// # #[tideorm::model(table = "users", soft_delete)]
    /// # struct User { #[tideorm(primary_key)] id: i64, name: String, active: bool, created_at: Option<chrono::DateTime<chrono::Utc>>, deleted_at: Option<chrono::DateTime<chrono::Utc>> }
    /// User::query().begin_or().or_where_eq(User::ACTIVE, true).and_where_gt(User::ID, 1).end_or().validate().unwrap();
    /// ```
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
