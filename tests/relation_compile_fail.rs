#[test]
fn invalid_relation_columns_fail_at_compile_time() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/invalid_relation_*.rs");
}
