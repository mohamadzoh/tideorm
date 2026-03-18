use crate::model::Model;
use crate::query::OrGroup;

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