use crate::model::Model;
use crate::query::OrGroup;
use crate::config::DatabaseType;

#[derive(tideorm::Model)]
#[tideorm(table = "query_mutation_guard_users")]
struct MutationGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[derive(tideorm::Model)]
#[tideorm(table = "query_mutation_guard_soft_delete_users", soft_delete)]
struct SoftDeleteMutationGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[test]
fn mutation_guard_rejects_unfiltered_delete() {
    let err = MutationGuardUser::query()
        .ensure_mutation_has_explicit_filters("delete")
        .unwrap_err();
    assert!(err.to_string().contains("requires at least one explicit filter"));
}

#[test]
fn mutation_guard_accepts_basic_where_clause() {
    assert!(MutationGuardUser::query()
        .where_eq("id", 1)
        .ensure_mutation_has_explicit_filters("delete")
        .is_ok());
}

#[test]
fn mutation_guard_accepts_non_empty_or_group() {
    assert!(MutationGuardUser::query()
        .begin_or()
        .or_where_eq("name", "alice")
        .end_or()
        .ensure_mutation_has_explicit_filters("delete")
        .is_ok());
}

#[test]
fn mutation_guard_rejects_only_trashed_without_user_filters() {
    let err = SoftDeleteMutationGuardUser::query()
        .only_trashed()
        .ensure_mutation_has_explicit_filters("restore")
        .unwrap_err();
    assert!(err.to_string().contains("requires at least one explicit filter"));
}

#[test]
fn mutation_guard_rejects_empty_nested_or_groups() {
    let mut query = MutationGuardUser::query();
    query.or_groups.push(OrGroup::new().nested_or(|group| group));

    let err = query
        .ensure_mutation_has_explicit_filters("delete")
        .unwrap_err();
    assert!(err.to_string().contains("requires at least one explicit filter"));
}

#[test]
fn delete_all_accepts_unfiltered_queries() {
    assert!(MutationGuardUser::query()
        .ensure_mutation_has_no_explicit_filters("delete_all")
        .is_ok());
}

#[test]
fn delete_all_rejects_filtered_queries() {
    let err = MutationGuardUser::query()
        .where_eq("id", 1)
        .ensure_mutation_has_no_explicit_filters("delete_all")
        .unwrap_err();
    assert!(err.to_string().contains("does not accept WHERE filters"));
}

#[derive(tideorm::Model)]
#[tideorm(table = "query_count_guard_users")]
struct QueryCountGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[test]
fn count_sql_preserves_joins() {
    let (sql, params) = QueryCountGuardUser::query()
        .inner_join("profiles", "query_count_guard_users.id", "profiles.user_id")
        .where_eq("profiles.active", true)
        .build_count_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(params.len(), 1);
    assert!(sql.starts_with("SELECT COUNT(*) AS count FROM (SELECT "));
    assert!(sql.contains(" FROM \"query_count_guard_users\" "));
    assert!(sql.contains(
        "INNER JOIN \"profiles\" ON \"query_count_guard_users\".\"id\" = \"profiles\".\"user_id\""
    ));
    assert!(sql.contains("WHERE \"profiles\".\"active\" = $1"));
    assert!(sql.ends_with(") AS \"tideorm_count_subquery\""));
}

#[test]
fn count_sql_preserves_group_by_and_having() {
    let (sql, params) = QueryCountGuardUser::query()
        .select(vec!["name"])
        .group_by("name")
        .having("COUNT(*) > 1")
        .build_count_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(params.is_empty());
    assert!(sql.starts_with("SELECT COUNT(*) AS count FROM (SELECT "));
    assert!(sql.contains("\"name\" FROM \"query_count_guard_users\""));
    assert!(sql.contains("GROUP BY \"name\""));
    assert!(sql.contains("HAVING COUNT(*) > 1"));
    assert!(sql.ends_with(") AS \"tideorm_count_subquery\""));
}

#[test]
fn count_sql_ignores_order_limit_and_offset() {
    let (sql, params) = QueryCountGuardUser::query()
        .where_eq("name", "alice")
        .order_by("name", crate::query::Order::Asc)
        .limit(10)
        .offset(20)
        .build_count_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(params.len(), 1);
    assert!(!sql.contains("ORDER BY"));
    assert!(!sql.contains("LIMIT 10"));
    assert!(!sql.contains("OFFSET 20"));
    assert!(sql.contains("WHERE \"name\" = $1"));
}