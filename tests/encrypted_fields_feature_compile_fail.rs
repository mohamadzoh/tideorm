#[cfg(not(feature = "encrypted-fields"))]
#[test]
fn encrypted_fields_require_feature_flag() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/invalid_encrypted_fields_without_feature.rs");
}

#[cfg(feature = "encrypted-fields")]
#[test]
fn encrypted_fields_require_feature_flag() {}
