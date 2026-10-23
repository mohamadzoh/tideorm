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
    fn own_table() -> Option<(&'static str, Option<&'static str>)> {
        None
    }
}

impl OrGroup {
    #[must_use]
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// let group = OrGroup::new();
    /// assert!(group.is_empty());
    /// ```
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
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// let group = OrGroup::new().nested_or(|q| q.where_eq("active", true).where_eq("role", "admin"));
    /// assert_eq!(group.condition_count(), 2);
    /// ```
    pub fn nested_or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        self.nested_groups.push(f(OrGroup::new()));
        self
    }

    #[must_use]
    ///
    /// ```
    /// use tideorm::prelude::*;
    /// let group = OrGroup::new().nested_and(|q| q.where_eq("active", true).where_eq("role", "admin"));
    /// assert_eq!(group.condition_count(), 2);
    /// ```
    pub fn nested_and<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        self.nested_groups.push(f(OrGroup::and_group()));
        self
    }

    ///
    /// ```
    /// use tideorm::prelude::*;
    /// assert!(OrGroup::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.nested_groups.iter().all(OrGroup::is_empty)
    }

    /// True when this group still restricts the row set, with `is_vacuous`
    /// telling which conditions restrict nothing (the caller's model-aware
    /// [`condition_is_vacuous`](super::condition_is_vacuous)).
    ///
    /// The two combinators are not symmetric. An `And` group survives as long as
    /// one member restricts; a single vacuous member of an `Or` group makes the
    /// whole group true, so an `Or` group only counts when *every* member
    /// restricts. An empty nested group renders nothing, and is no member.
    pub(crate) fn is_restrictive_with(
        &self,
        is_vacuous: &impl Fn(&WhereCondition) -> bool,
    ) -> bool {
        match self.combine_with {
            LogicalOp::And => {
                self.conditions
                    .iter()
                    .any(|condition| !is_vacuous(condition))
                    || self
                        .nested_groups
                        .iter()
                        .any(|group| group.is_restrictive_with(is_vacuous))
            }
            LogicalOp::Or => {
                !self.is_empty()
                    && self
                        .conditions
                        .iter()
                        .all(|condition| !is_vacuous(condition))
                    && self
                        .nested_groups
                        .iter()
                        .filter(|group| !group.is_empty())
                        .all(|group| group.is_restrictive_with(is_vacuous))
            }
        }
    }

    ///
    /// ```
    /// use tideorm::prelude::*;
    /// assert_eq!(OrGroup::new().where_eq("active", true).condition_count(), 1);
    /// ```
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
