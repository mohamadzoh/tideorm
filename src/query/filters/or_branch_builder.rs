use super::*;

/// Fluent builder for chained OR conditions
#[derive(Debug)]
pub struct OrBranchBuilder<M: Model> {
    query: QueryBuilder<M>,
    branches: Vec<OrBranch>,
    current_branch: OrBranch,
}

impl<M: Model> OrBranchBuilder<M> {
    #[must_use]
    pub fn new(query: QueryBuilder<M>) -> Self {
        Self {
            query,
            branches: Vec::new(),
            current_branch: OrBranch::new(),
        }
    }

    #[must_use]
    pub fn or_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_eq(column, value);
        self
    }

    #[must_use]
    pub fn or_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not(column, value);
        self
    }

    #[must_use]
    pub fn or_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gt(column, value);
        self
    }

    #[must_use]
    pub fn or_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_gte(column, value);
        self
    }

    #[must_use]
    pub fn or_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lt(column, value);
        self
    }

    #[must_use]
    pub fn or_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_lte(column, value);
        self
    }

    #[must_use]
    pub fn or_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_like(column, pattern);
        self
    }

    #[must_use]
    pub fn or_where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_contains(column, value);
        self
    }

    #[must_use]
    pub fn or_where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_starts_with(column, value);
        self
    }

    #[must_use]
    pub fn or_where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_ends_with(column, value);
        self
    }

    #[must_use]
    pub fn or_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_in(column, values);
        self
    }

    #[must_use]
    pub fn or_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not_in(column, values);
        self
    }

    #[must_use]
    pub fn or_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_null(column);
        self
    }

    #[must_use]
    pub fn or_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_not_null(column);
        self
    }

    #[must_use]
    pub fn or_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_between(column, min, max);
        self
    }

    #[must_use]
    pub fn or_where_raw(mut self, raw_sql: &str) -> Self {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }
        self.current_branch = OrBranch::new().where_raw(raw_sql);
        self
    }

    #[must_use]
    pub fn and_where_eq(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_eq(column, value);
        self
    }

    #[must_use]
    pub fn and_where_not(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_not(column, value);
        self
    }

    #[must_use]
    pub fn and_where_gt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_gt(column, value);
        self
    }

    #[must_use]
    pub fn and_where_gte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_gte(column, value);
        self
    }

    #[must_use]
    pub fn and_where_lt(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_lt(column, value);
        self
    }

    #[must_use]
    pub fn and_where_lte(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_lte(column, value);
        self
    }

    #[must_use]
    pub fn and_where_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_like(column, pattern);
        self
    }

    #[must_use]
    pub fn and_where_contains(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_contains(column, value);
        self
    }

    #[must_use]
    pub fn and_where_starts_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_starts_with(column, value);
        self
    }

    #[must_use]
    pub fn and_where_ends_with(
        mut self,
        column: impl crate::columns::IntoColumnName,
        value: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_ends_with(column, value);
        self
    }

    #[must_use]
    pub fn and_where_not_like(
        mut self,
        column: impl crate::columns::IntoColumnName,
        pattern: &str,
    ) -> Self {
        self.current_branch = self.current_branch.where_not_like(column, pattern);
        self
    }

    #[must_use]
    pub fn and_where_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.current_branch = self.current_branch.where_in(column, values);
        self
    }

    #[must_use]
    pub fn and_where_not_in<V: Into<serde_json::Value>>(
        mut self,
        column: impl crate::columns::IntoColumnName,
        values: Vec<V>,
    ) -> Self {
        self.current_branch = self.current_branch.where_not_in(column, values);
        self
    }

    #[must_use]
    pub fn and_where_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.current_branch = self.current_branch.where_null(column);
        self
    }

    #[must_use]
    pub fn and_where_not_null(mut self, column: impl crate::columns::IntoColumnName) -> Self {
        self.current_branch = self.current_branch.where_not_null(column);
        self
    }

    #[must_use]
    pub fn and_where_between(
        mut self,
        column: impl crate::columns::IntoColumnName,
        min: impl Into<serde_json::Value>,
        max: impl Into<serde_json::Value>,
    ) -> Self {
        self.current_branch = self.current_branch.where_between(column, min, max);
        self
    }

    #[must_use]
    pub fn and_where_raw(mut self, raw_sql: &str) -> Self {
        self.current_branch = self.current_branch.where_raw(raw_sql);
        self
    }

    #[must_use]
    pub fn end_or(mut self) -> QueryBuilder<M> {
        if !self.current_branch.is_empty() {
            self.branches.push(self.current_branch);
        }

        if !self.branches.is_empty() {
            let mut or_group = OrGroup::new();

            for branch in self.branches {
                if branch.conditions.len() == 1 {
                    if let Some(condition) = branch.conditions.into_iter().next() {
                        or_group.conditions.push(condition);
                    }
                } else {
                    let mut nested = OrGroup::new();
                    nested.combine_with = LogicalOp::And;
                    nested.conditions = branch.conditions;
                    or_group.nested_groups.push(nested);
                }
            }

            self.query.or_groups.push(or_group);
        }

        self.query
    }

    pub fn branch_count(&self) -> usize {
        let current = if self.current_branch.is_empty() { 0 } else { 1 };
        self.branches.len() + current
    }

    pub fn total_conditions(&self) -> usize {
        let mut total: usize = self.branches.iter().map(|b| b.len()).sum();
        total += self.current_branch.len();
        total
    }
}
