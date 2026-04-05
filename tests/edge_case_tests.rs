//! Edge case & comprehensive tests for TideORM v0.7
//!
//! These tests cover gaps identified in the test audit, including:
//! - Error From impls & conversion edge cases
//! - Config: malformed URLs, MariaDB detection, feature matrix
//! - Cache: capacity boundaries, concurrent stress, TTL edge cases
//! - Profiling: N+1 detection, complexity scoring, report formatting
//! - Logging: operation detection, stats accumulation, slow-query thresholds
//! - Fulltext: empty query, regex-special chars, long text
//! - Query analyzer: comprehensive SQL pattern analysis
//! - Soft delete: trait method contracts
//! - DatabaseType: exhaustive feature-flag parity between MySQL & MariaDB

#[path = "edge_case/core_tests.rs"]
mod core_tests;

#[path = "edge_case/additional_tests.rs"]
mod additional_tests;
