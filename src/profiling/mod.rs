//! Performance Profiling for TideORM
//!
//! Use this module to answer performance questions with actual timings instead
//! of guesses.
//!
//! `GlobalProfiler` records statistics for real query execution paths.
//! `Profiler` is for manually assembled reports in tests, benchmarks, or local
//! experiments.
//!
//! Practical split:
//! - use `GlobalProfiler` when you want process-wide timings for real query execution paths
//! - use `Profiler` when you want to build a focused report around one benchmark, test, or experiment
//! - use `QueryAnalyzer` only as a heuristic pass over rendered SQL, not as a substitute for backend query plans

mod analyzer;
mod global;
mod report;
mod utils;

pub use self::analyzer::{QueryAnalyzer, QueryComplexity, QuerySuggestion, SuggestionLevel};
#[doc(hidden)]
pub use self::global::__profile_future;
pub use self::global::{GlobalProfiler, GlobalStats};
pub use self::report::{ProfileReport, ProfiledQuery, Profiler};

#[cfg(test)]
#[path = "../../tests/unit/profiling_tests.rs"]
mod tests;
