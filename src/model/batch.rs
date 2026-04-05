#![allow(missing_docs)]

use crate::columns::IntoColumnName;
use crate::error::{Error, Result};
use crate::internal::sql_safety::quote_ident;
use crate::query::{LogicalOp, OrGroup, QueryBuilder};

use super::Model;

mod builder_setup_filters;
mod sql_execution;
mod validation_helpers;

/// Builder for batch update operations.
pub struct BatchUpdateBuilder<M: Model> {
    _marker: std::marker::PhantomData<M>,
    updates: std::collections::HashMap<String, UpdateValue>,
    conditions: Vec<crate::query::WhereCondition>,
    returning: bool,
    limit_value: Option<u64>,
}

/// Value for batch update operations.
#[derive(Debug, Clone)]
pub enum UpdateValue {
    Value(serde_json::Value),
    UnsafeRaw(String),
    Increment(i64),
    Decrement(i64),
    Multiply(f64),
    Divide(f64),
    ArrayAppend(serde_json::Value),
    ArrayRemove(serde_json::Value),
    JsonSet(String, serde_json::Value),
    Coalesce(serde_json::Value),
}

impl<M: Model> Default for BatchUpdateBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/model_batch_tests.rs"]
mod tests;
