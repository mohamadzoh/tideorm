use super::*;

/// A group of conditions combined with a logical operator
#[derive(Debug, Clone)]
pub struct OrGroup {
    pub conditions: Vec<WhereCondition>,
    pub nested_groups: Vec<OrGroup>,
    pub combine_with: LogicalOp,
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

    #[must_use]
    pub fn nested_or<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let nested = f(OrGroup::new());
        self.nested_groups.push(nested);
        self
    }

    #[must_use]
    pub fn nested_and<F>(mut self, f: F) -> Self
    where
        F: FnOnce(OrGroup) -> OrGroup,
    {
        let mut nested = OrGroup::new();
        nested.combine_with = LogicalOp::And;
        nested = f(nested);
        self.nested_groups.push(nested);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.nested_groups.is_empty()
    }

    pub fn condition_count(&self) -> usize {
        let nested_count: usize = self.nested_groups.iter().map(|g| g.condition_count()).sum();
        self.conditions.len() + nested_count
    }
}

impl_or_where_condition_methods!(OrGroup);

impl Default for OrGroup {
    fn default() -> Self {
        Self::new()
    }
}
