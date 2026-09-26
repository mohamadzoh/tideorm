use super::*;

/// Fluent builder for an OR of AND-ed branches, started by
/// [`QueryBuilder::begin_or`] and closed by [`end_or`](Self::end_or).
///
/// Each `or_where_*` call starts a new branch and each `and_where_*` call adds
/// to the current one, so
/// `.begin_or().or_where_eq("a", 1).and_where_eq("b", 2).or_where_eq("c", 3).end_or()`
/// adds `((a = 1 AND b = 2) OR c = 3)` to the query.
#[derive(Debug)]
pub struct OrBranchBuilder<M: Model> {
    query: QueryBuilder<M>,
    branches: Vec<OrGroup>,
    current_branch: OrGroup,
}

impl<M: Model> crate::columns::ConditionOwner for OrBranchBuilder<M> {
    fn own_table() -> Option<&'static str> {
        Some(M::table_name())
    }
}

impl<M: Model> OrBranchBuilder<M> {
    #[must_use]
    pub fn new(query: QueryBuilder<M>) -> Self {
        Self {
            query,
            branches: Vec::new(),
            current_branch: OrGroup::and_group(),
        }
    }

    fn start_branch(mut self, condition: WhereCondition) -> Self {
        let previous = std::mem::replace(&mut self.current_branch, OrGroup::and_group());
        if !previous.is_empty() {
            self.branches.push(previous);
        }
        self.current_branch.conditions.push(condition);
        self
    }

    fn extend_branch(mut self, condition: WhereCondition) -> Self {
        self.current_branch.conditions.push(condition);
        self
    }

    /// Close the OR expression and return the query it was started on.
    ///
    /// A single-condition branch becomes a plain member of the OR group; a
    /// longer one becomes a nested AND group. An empty expression adds nothing.
    #[must_use]
    pub fn end_or(mut self) -> QueryBuilder<M> {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }

        if !self.branches.is_empty() {
            let mut or_group = OrGroup::new();

            for mut branch in self.branches {
                if branch.conditions.len() == 1 {
                    or_group.conditions.append(&mut branch.conditions);
                } else {
                    or_group.nested_groups.push(branch);
                }
            }

            self.query.or_groups.push(or_group);
        }

        self.query
    }
}

crate::query::condition_methods! {
    impl[M: Model] OrBranchBuilder<M> {
        or_where + raw => start_branch, "Starts a new branch of the OR expression.";
        and_where + raw => extend_branch, "ANDs the condition into the current branch.";
    }
}
