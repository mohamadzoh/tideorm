use super::*;

/// A group of conditions combined with a logical operator
#[derive(Debug, Clone)]
pub struct OrGroup {
    pub conditions: Vec<WhereCondition>,
    pub nested_groups: Vec<OrGroup>,
    pub combine_with: LogicalOp,
}

/// An OR group belongs to no model yet, so a typed column in it keeps its
/// table; a query of that model reads the qualified name as its own column.
impl crate::columns::ConditionOwner for OrGroup {
    fn own_table() -> Option<&'static str> {
        None
    }
}

impl OrGroup {
    #[must_use]
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
            nested_groups: Vec::new(),
            combine_with: LogicalOp::Or,
        }
    }

    /// An empty group whose members are ANDed together.
    pub(crate) fn and_group() -> Self {
        Self {
            combine_with: LogicalOp::And,
            ..Self::new()
        }
    }

    #[must_use]
    pub fn nested_or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        self.nested_groups.push(f(OrGroup::new()));
        self
    }

    #[must_use]
    pub fn nested_and<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        self.nested_groups.push(f(OrGroup::and_group()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.nested_groups.is_empty()
    }

    /// True when this group still restricts the row set.
    ///
    /// The two combinators are not symmetric. An `And` group survives as long as
    /// one member restricts; a single vacuous member of an `Or` group makes the
    /// whole group true, so an `Or` group only counts when *every* member
    /// restricts. See [`condition_is_vacuous`](super::condition_is_vacuous).
    pub(crate) fn is_restrictive(&self) -> bool {
        match self.combine_with {
            LogicalOp::And => {
                self.conditions
                    .iter()
                    .any(|condition| !super::condition_is_vacuous(condition))
                    || self.nested_groups.iter().any(OrGroup::is_restrictive)
            }
            LogicalOp::Or => {
                !self.is_empty()
                    && self
                        .conditions
                        .iter()
                        .all(|condition| !super::condition_is_vacuous(condition))
                    && self.nested_groups.iter().all(OrGroup::is_restrictive)
            }
        }
    }

    pub fn condition_count(&self) -> usize {
        let nested_count: usize = self.nested_groups.iter().map(|g| g.condition_count()).sum();
        self.conditions.len() + nested_count
    }

    fn push_condition(mut self, condition: WhereCondition) -> Self {
        self.conditions.push(condition);
        self
    }
}

crate::query::condition_methods! {
    impl[] OrGroup {
        where + raw => push_condition,
            "The condition joins this group, combined with its other members by `combine_with`.";
    }
}

impl Default for OrGroup {
    fn default() -> Self {
        Self::new()
    }
}
