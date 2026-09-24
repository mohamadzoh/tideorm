// With the feature on, `#[tideorm(encrypted)]` is legal, so there is nothing to check.
#![cfg(not(feature = "encrypted-fields"))]

#[test]
fn encrypted_fields_require_feature_flag() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/invalid_encrypted_fields_without_feature.rs");
}
