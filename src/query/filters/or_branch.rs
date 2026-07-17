use super::*;

/// A single branch in an OR expression
#[derive(Debug, Clone)]
pub struct OrBranch {
    pub conditions: Vec<WhereCondition>,
}

impl OrBranch {
    #[must_use]
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.conditions.len()
    }
}

impl_or_where_condition_methods!(OrBranch);

impl Default for OrBranch {
    fn default() -> Self {
        Self::new()
    }
}
