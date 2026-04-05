//! Integration tests for SeaORM 2.0 features
//!
//! This file tests the following features:
//! - Strongly-typed columns
//! - Nested ActiveModel (cascade save)
//! - Self-referencing relations
//! - Linked partial select
//! - Join result consolidation

use tideorm::columns::{
    Column, ColumnEq, ColumnIn, ColumnLike, ColumnNullable, ColumnOperator, ColumnOrd,
};

#[path = "seaorm2_features_tests/join_consolidation.rs"]
mod join_consolidation_tests;

// =============================================================================
// STRONGLY-TYPED COLUMNS TESTS
// =============================================================================

#[path = "seaorm2_features_tests/typed_columns.rs"]
mod typed_columns;

// =============================================================================
// JOIN RESULT CONSOLIDATOR TESTS
// =============================================================================

// =============================================================================
// NESTED ACTIVE MODEL TESTS
// =============================================================================

#[path = "seaorm2_features_tests/nested_save.rs"]
mod nested_save;
