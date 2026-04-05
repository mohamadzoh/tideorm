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
        "SELECT \"scoped_query_test_users\".* FROM \"scoped_query_test_users\" WHERE \"active\" = true AND \"verified_at\" IS NOT NULL AND \"role\" = 'admin'"
    );
}