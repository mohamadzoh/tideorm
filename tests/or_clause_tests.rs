//! OR-clause tests: how `OrGroup` builders record their conditions,
//! and — against PostgreSQL — what the fluent `begin_or()` API actually returns.

use tideorm::query::{ConditionValue, LogicalOp, Operator, OrGroup};

#[path = "or_clause/core_unit_tests.rs"]
mod core_unit_tests;

#[cfg(all(feature = "postgres", feature = "runtime-tokio"))]
#[path = "support/postgres_test_config.rs"]
mod test_config;

#[cfg(all(feature = "postgres", feature = "runtime-tokio"))]
#[path = "or_clause/fluent_or_integration_tests.rs"]
mod fluent_or_integration_cases;
