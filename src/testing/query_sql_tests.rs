use crate::columns::ColumnLike;
use crate::config::DatabaseType;
use crate::model::Model as ModelTrait;
use crate::query::OrGroup;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::{Database, QueryCache, TideConfig};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::{Mutex, OnceLock};
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::time::Duration;

#[tideorm::model(table = "query_mutation_guard_users")]
struct MutationGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "query_mutation_guard_soft_delete_users", soft_delete)]
struct SoftDeleteMutationGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn query_mutation_cache_test_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_query_mutation_cache_test_db() -> Database {
    Database::reset_global();
    TideConfig::reset();
    QueryCache::global().clear();
    QueryCache::global().enable();

    let db = Database::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory connection should succeed for query mutation cache tests");
    Database::set_global(db.clone()).expect("setting global database should succeed");

    db.__execute_with_params(
        "CREATE TABLE query_mutation_guard_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        vec![],
    )
    .await
    .expect("creating mutation guard schema should succeed");
    db.__execute_with_params(
        "CREATE TABLE query_mutation_guard_soft_delete_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, deleted_at TEXT NULL)",
        vec![],
    )
    .await
    .expect("creating soft delete mutation guard schema should succeed");

    db
}

#[test]
fn query_fragment_preserves_soft_delete_scope_semantics() {
    let with_trashed = SoftDeleteMutationGuardUser::query().with_trashed();
    let fragment = with_trashed.consolidate();

    assert!(!fragment.is_empty());

    let rebuilt = SoftDeleteMutationGuardUser::query()
        .only_trashed()
        .apply(&fragment)
        .build_sql_preview();

    assert_eq!(rebuilt, with_trashed.build_sql_preview());
    assert!(
        !rebuilt.contains("deleted_at\" IS NOT NULL"),
        "sql: {rebuilt}"
    );
    assert!(!rebuilt.contains("deleted_at\" IS NULL"), "sql: {rebuilt}");
}

#[test]
fn query_fragment_only_trashed_scope_is_not_empty() {
    let fragment = SoftDeleteMutationGuardUser::query()
        .only_trashed()
        .consolidate();

    assert!(!fragment.is_empty());
}

#[test]
fn mutation_guard_rejects_unfiltered_delete() {
    let err = MutationGuardUser::query()
        .ensure_mutation_has_explicit_filters("delete")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn mutation_guard_accepts_basic_where_clause() {
    assert!(
        MutationGuardUser::query()
            .where_eq("id", 1)
            .ensure_mutation_has_explicit_filters("delete")
            .is_ok()
    );
}

#[test]
fn mutation_guard_accepts_non_empty_or_group() {
    assert!(
        MutationGuardUser::query()
            .begin_or()
            .or_where_eq("name", "alice")
            .end_or()
            .ensure_mutation_has_explicit_filters("delete")
            .is_ok()
    );
}

#[test]
fn mutation_guard_rejects_only_trashed_without_user_filters() {
    let err = SoftDeleteMutationGuardUser::query()
        .only_trashed()
        .ensure_mutation_has_explicit_filters("restore")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn mutation_guard_rejects_empty_nested_or_groups() {
    let mut query = MutationGuardUser::query();
    query
        .or_groups
        .push(OrGroup::new().nested_or(|group| group));

    let err = query
        .ensure_mutation_has_explicit_filters("delete")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("requires at least one explicit filter")
    );
}

#[test]
fn delete_all_accepts_unfiltered_queries() {
    assert!(
        MutationGuardUser::query()
            .ensure_mutation_has_no_explicit_filters("delete_all")
            .is_ok()
    );
}

#[test]
fn delete_all_rejects_filtered_queries() {
    let err = MutationGuardUser::query()
        .where_eq("id", 1)
        .ensure_mutation_has_no_explicit_filters("delete_all")
        .unwrap_err();
    assert!(err.to_string().contains("does not accept WHERE filters"));
}

#[tideorm::model(table = "query_count_guard_users")]
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

#[test]
fn exists_sql_uses_select_one_with_limit() {
    let (sql, params) = QueryCountGuardUser::query()
        .where_eq("name", "alice")
        .build_exists_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(params.len(), 1);
    assert!(sql.starts_with("SELECT 1 FROM (SELECT 1 FROM "));
    assert!(sql.contains("FROM \"query_count_guard_users\""));
    assert!(sql.contains("WHERE \"name\" = $1"));
    assert!(sql.ends_with("AS \"tideorm_exists_subquery\" LIMIT 1"));
}

#[test]
fn exists_sql_discards_original_projection_for_non_union_queries() {
    let (sql, params) = QueryCountGuardUser::query()
        .select(vec!["name"])
        .build_exists_sql_with_params_for_db(DatabaseType::Postgres);

    assert!(params.is_empty());
    assert!(sql.starts_with("SELECT 1 FROM (SELECT 1 FROM "));
    assert!(!sql.contains("SELECT \"query_count_guard_users\".\"name\""));
}

#[test]
fn exists_sql_ignores_order_limit_and_offset() {
    let (sql, params) = QueryCountGuardUser::query()
        .where_eq("name", "alice")
        .order_by("name", crate::query::Order::Asc)
        .limit(10)
        .offset(20)
        .build_exists_sql_with_params_for_db(DatabaseType::Postgres);

    assert_eq!(params.len(), 1);
    assert!(!sql.contains("ORDER BY"));
    assert!(!sql.contains("LIMIT 10"));
    assert!(!sql.contains("OFFSET 20"));
    assert!(sql.ends_with("LIMIT 1"));
    assert!(sql.contains("WHERE \"name\" = $1"));
}

#[test]
fn sql_preview_is_labeled_as_non_executable() {
    let preview = QueryCountGuardUser::query()
        .where_eq("name", "alice")
        .build_sql_preview();

    assert!(preview.starts_with("-- DEBUG PREVIEW (not executable, values are approximate)\n"));
    assert!(preview.contains("query_count_guard_users"));
    assert!(preview.contains("WHERE \"name\" = 'alice'"));
}

#[test]
fn sql_preview_marks_escaped_literal_like_helpers() {
    let name = crate::columns::Column::<String>::new("name");
    let preview = QueryCountGuardUser::query()
        .where_col(name.starts_with(r"100%_\done"))
        .build_sql_preview();

    assert!(preview.contains("LIKE '100\\%\\_\\\\done%'"));
    assert!(preview.contains("ESCAPE '\\\\'"));
}

#[test]
fn debug_output_includes_preview_banner_and_parameterized_sql() {
    let debug_info = QueryCountGuardUser::query()
        .where_eq("name", "alice")
        .debug();

    assert!(
        debug_info
            .sql
            .starts_with("-- DEBUG PREVIEW (not executable, values are approximate)\n")
    );
    assert!(debug_info.sql.contains("-- PARAMETERIZED SQL\nSELECT"));
    assert!(debug_info.sql.contains("query_count_guard_users"));
    assert_eq!(debug_info.params.len(), 1);
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn query_delete_invalidates_cached_queries() {
    let _guard = query_mutation_cache_test_guard()
        .lock()
        .expect("query mutation cache test guard should not be poisoned");
    let _db = setup_query_mutation_cache_test_db().await;

    let saved = MutationGuardUser {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = MutationGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before delete should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = MutationGuardUser::query()
        .where_eq("id", saved.id)
        .delete()
        .await
        .expect("query delete should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn query_delete_all_invalidates_cached_queries() {
    let _guard = query_mutation_cache_test_guard()
        .lock()
        .expect("query mutation cache test guard should not be poisoned");
    let _db = setup_query_mutation_cache_test_db().await;

    MutationGuardUser {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before = MutationGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before delete_all should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = MutationGuardUser::query()
        .delete_all()
        .await
        .expect("query delete_all should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn soft_delete_and_restore_invalidate_cached_queries() {
    let _guard = query_mutation_cache_test_guard()
        .lock()
        .expect("query mutation cache test guard should not be poisoned");
    let _db = setup_query_mutation_cache_test_db().await;

    let saved = SoftDeleteMutationGuardUser {
        id: 0,
        name: "Alice".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("seed save should succeed");

    let cached_before_soft_delete = SoftDeleteMutationGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before soft_delete should succeed");
    assert_eq!(cached_before_soft_delete.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let soft_deleted = SoftDeleteMutationGuardUser::query()
        .where_eq("id", saved.id)
        .soft_delete()
        .await
        .expect("soft_delete should succeed");

    assert_eq!(soft_deleted, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);

    let cached_before_restore = SoftDeleteMutationGuardUser::query()
        .with_trashed()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before restore should succeed");
    assert_eq!(cached_before_restore.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let restored = SoftDeleteMutationGuardUser::query()
        .where_eq("id", saved.id)
        .with_trashed()
        .restore()
        .await
        .expect("restore should succeed");

    assert_eq!(restored, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn force_delete_invalidates_cached_queries() {
    let _guard = query_mutation_cache_test_guard()
        .lock()
        .expect("query mutation cache test guard should not be poisoned");
    let _db = setup_query_mutation_cache_test_db().await;

    let saved = SoftDeleteMutationGuardUser {
        id: 0,
        name: "Alice".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("seed save should succeed");

    SoftDeleteMutationGuardUser::query()
        .where_eq("id", saved.id)
        .soft_delete()
        .await
        .expect("soft_delete should succeed");

    let cached_before = SoftDeleteMutationGuardUser::query()
        .with_trashed()
        .order_by("id", crate::query::Order::Asc)
        .cache(Duration::from_secs(60))
        .get()
        .await
        .expect("cached query before force_delete should succeed");
    assert_eq!(cached_before.len(), 1);
    assert_eq!(QueryCache::global().stats().entries, 1);

    let rows_affected = SoftDeleteMutationGuardUser::query()
        .with_trashed()
        .where_eq("id", saved.id)
        .force_delete()
        .await
        .expect("force_delete should succeed");

    assert_eq!(rows_affected, 1);
    assert_eq!(QueryCache::global().stats().entries, 0);
}
