#[test]
fn invalid_relation_columns_fail_at_compile_time() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/invalid_primary_key_*.rs");
    t.compile_fail("tests/ui/invalid_relation_*.rs");
    t.compile_fail("tests/ui/invalid_skip_default.rs");
    t.compile_fail("tests/ui/invalid_validate_custom.rs");
    t.compile_fail("tests/ui/invalid_validate_rule_keeps_the_model.rs");
    t.compile_fail("tests/ui/invalid_field_type.rs");
    t.compile_fail("tests/ui/invalid_hidden_field.rs");
    t.compile_fail("tests/ui/invalid_validate_bounds.rs");
    t.compile_fail("tests/ui/invalid_field_index.rs");
    t.compile_fail("tests/ui/invalid_encrypted_lookup.rs");
    t.compile_fail("tests/ui/invalid_ambiguous_field_name.rs");
    t.compile_fail("tests/ui/invalid_generic_model.rs");
    t.compile_fail("tests/ui/invalid_scope_name.rs");
}
