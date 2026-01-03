//! Migration Test Module
//!
//! This module contains comprehensive tests for TideORM's migration system.
//! 
//! ## Running the Tests
//!
//! ```bash
//! # Run all migration tests
//! cargo run --example migration_test_runner
//! 
//! # Or run with specific database URL
//! POSTGRESQL_DATABASE_URL=postgres://localhost/migration_tests cargo run --example migration_test_runner
//! ```

pub mod migrations;
pub mod test_utils;
