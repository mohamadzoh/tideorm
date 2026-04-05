//! Tests for QueryBuilder functionality
//!
//! These tests verify the critical query builder features:
//! - WHERE conditions (all operators)
//! - COUNT optimization
//! - Bulk DELETE
//! - ORDER BY, LIMIT, OFFSET
//!
//! Note: These tests require a test database connection.
//! Set TEST_DATABASE_URL environment variable to run.

use tideorm::query::{ConditionValue, Order};

// ============================================================================
// Unit tests for QueryBuilder construction (no DB needed)
// ============================================================================

#[cfg(test)]
#[path = "query_builder_tests/query_builder_unit_tests.rs"]
mod query_builder_unit_tests;

// ============================================================================
// Database Pool Configuration Tests
// ============================================================================

#[cfg(test)]
#[path = "query_builder_tests/pool_config_tests.rs"]
mod pool_config_tests;

// ============================================================================
// WHERE Clause Builder Tests
// ============================================================================

#[cfg(test)]
#[path = "query_builder_tests/where_clause_tests.rs"]
mod where_clause_tests;

// ============================================================================
// Subquery and Raw Expression Tests
// ============================================================================

#[cfg(test)]
#[path = "query_builder_tests/subquery_tests.rs"]
mod subquery_tests;

// ============================================================================
// Raw Expression Tests
// ============================================================================

#[cfg(test)]
#[path = "query_builder_tests/raw_expression_tests.rs"]
mod raw_expression_tests;

// ============================================================================
// Bulk Delete Tests
// ============================================================================

#[cfg(test)]
#[path = "query_builder_tests/bulk_delete_tests.rs"]
mod bulk_delete_tests;
