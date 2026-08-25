#[path = "core/validation_tests.rs"]
mod validation_test_cases;

// =============================================================================
// QUERY MODULE TESTS
// =============================================================================

#[cfg(test)]
#[path = "core/query_tests.rs"]
mod query_tests;

// =============================================================================
// SOFT DELETE TESTS
// =============================================================================

#[cfg(test)]
#[path = "core/soft_delete_tests.rs"]
mod soft_delete_tests;

// =============================================================================
// DATABASE TYPE CONVERSION TESTS
// =============================================================================

#[cfg(test)]
#[path = "core/type_conversion_tests.rs"]
mod type_conversion_tests;

// =============================================================================
// JSON AND ARRAY TYPES TESTS
// =============================================================================

#[cfg(test)]
#[path = "core/json_array_types_tests.rs"]
mod json_array_types_tests;

// =============================================================================
// JOIN AND AGGREGATION TESTS
// =============================================================================

#[cfg(test)]
#[path = "core/join_aggregation_tests.rs"]
mod join_aggregation_tests;
