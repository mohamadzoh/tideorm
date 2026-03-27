//! Comprehensive Unit Tests for TideORM
//!
//! These tests verify core functionality without requiring a database connection.
//! Run with: `cargo test --test unit_tests`
#![allow(clippy::approx_constant)]

#[path = "unit_tests/core.rs"]
mod core;

#[path = "unit_tests/edge_cases.rs"]
mod edge_cases;

#[path = "unit_tests/migration_and_builders.rs"]
mod migration_and_builders;

#[path = "unit_tests/optional_features.rs"]
mod optional_features;

#[path = "unit_tests/relations_and_types.rs"]
mod relations_and_types;

#[path = "unit_tests/cache_and_seeding.rs"]
mod cache_and_seeding;
