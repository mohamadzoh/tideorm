//! Public-API tests for TideORM.
//!
//! Black-box: these live in a normal integration-test target and reach the crate
//! only through its public API, without needing a database connection.
//! Run with: `cargo test --test public_api_tests`
//!
//! Not to be confused with `tests/unit/`, which is white-box — those files are
//! `#[path]`-included straight into `src/`, can see private items, and run as part
//! of `cargo test --lib`.
#![allow(clippy::approx_constant)]

#[path = "public_api_tests/core.rs"]
mod core;

#[path = "public_api_tests/edge_cases.rs"]
mod edge_cases;

#[path = "public_api_tests/migration_and_builders.rs"]
mod migration_and_builders;

#[path = "public_api_tests/optional_features.rs"]
mod optional_features;

#[path = "public_api_tests/relations_and_types.rs"]
mod relations_and_types;

#[path = "public_api_tests/cache_and_seeding.rs"]
mod cache_and_seeding;
