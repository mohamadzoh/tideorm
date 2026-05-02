//! Tests for OR Clause functionality in TideORM
//!
//! These tests verify the OR clause query builder features:
//! - OrGroup construction and methods
//! - or_where and or_where_* methods on QueryBuilder
//! - Nested OR groups
//! - OR with BatchUpdateBuilder
//!
//! Note: Some tests require a database connection.

#![allow(clippy::approx_constant)]

use tideorm::query::{ConditionValue, LogicalOp, Operator, OrGroup, Order};

#[path = "or_clause/core_unit_tests.rs"]
mod core_unit_tests;

#[path = "or_clause/mid_scenarios.rs"]
mod mid_scenarios;

#[cfg(all(feature = "postgres", feature = "runtime-tokio"))]
#[path = "or_clause/fluent_or_integration_tests.rs"]
mod fluent_or_integration_cases;
