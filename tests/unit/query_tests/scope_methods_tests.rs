use super::ScopedQueryTestUserQueryScopes as _;
use super::{DatabaseType, ModelTrait, ScopedQueryTestUser};

#[test]
fn model_local_scope_methods_chain_on_query_builder() {
    let sql = ScopedQueryTestUser::query()
        .active()
        .verified()
        .role("admin")
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT \"scoped_query_test_users\".\"id\", \"scoped_query_test_users\".\"active\", \"scoped_query_test_users\".\"verified_at\", \"scoped_query_test_users\".\"role\" FROM \"scoped_query_test_users\" WHERE \"active\" = TRUE AND \"verified_at\" IS NOT NULL AND \"role\" = 'admin'"
    );
}

#[test]
fn scope_parameters_may_be_mut_and_name_the_model_as_self() {
    let other = ScopedQueryTestUser {
        role: "editor".into(),
        ..Default::default()
    };
    let sql = ScopedQueryTestUser::query()
        .same_role_as(&other)
        .at_most(0)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(sql.contains("\"role\" = 'editor'"), "{sql}");
    assert!(sql.ends_with("LIMIT 1"), "{sql}");
}
