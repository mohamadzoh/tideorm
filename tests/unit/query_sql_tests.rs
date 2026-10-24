use crate::columns::ColumnLike;
use crate::config::DatabaseType;
use crate::internal::Value;
use crate::model::Model as ModelTrait;
use crate::query::OrGroup;

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use crate::Database;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::sync::Arc;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use std::time::Duration;
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use tokio::sync::Mutex;

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

#[test]
fn equality_against_json_null_renders_a_null_check() {
    for (query, expected) in [
        (
            SoftDeleteMutationGuardUser::query()
                .with_trashed()
                .where_eq("deleted_at", serde_json::Value::Null),
            r#""deleted_at" IS NULL"#,
        ),
        (
            SoftDeleteMutationGuardUser::query()
                .with_trashed()
                .where_not("deleted_at", serde_json::Value::Null),
            r#""deleted_at" IS NOT NULL"#,
        ),
    ] {
        let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);
        assert!(sql.ends_with(&format!("WHERE {expected}")), "sql: {sql}");
        assert!(params.is_empty(), "params: {params:?}");
    }
}

#[test]
fn ordering_comparison_against_json_null_is_rejected() {
    let err = SoftDeleteMutationGuardUser::query()
        .with_trashed()
        .where_gt("deleted_at", serde_json::Value::Null)
        .ensure_conditions_are_representable()
        .expect_err("a NULL ordering bound has no null-safe rendering");

    assert!(err.to_string().contains("where_null()"), "err: {err}");
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
async fn setup_query_mutation_cache_test_db() -> Database {
    let db = crate::test_support::install_sqlite_global(&[
        "CREATE TABLE query_mutation_guard_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        "CREATE TABLE query_mutation_guard_soft_delete_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, deleted_at TEXT NULL)",
    ])
    .await;
    crate::test_support::set_query_cache(true);
    db
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn chunk_processes_rows_without_skipping_when_callback_changes_query_scope() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    let _db = setup_query_mutation_cache_test_db().await;

    for index in 1..=7 {
        MutationGuardUser {
            id: 0,
            name: format!("user-{index}"),
        }
        .save()
        .await
        .expect("seed save should succeed");
    }

    let seen_batches = Arc::new(Mutex::new(Vec::<Vec<i64>>::new()));

    MutationGuardUser::query()
        .where_like("name", "user-%")
        .cache_with_key("chunked-mutation-guard-users", Duration::from_secs(60))
        .chunk(2, {
            let seen_batches = Arc::clone(&seen_batches);
            move |batch| {
                let seen_batches = Arc::clone(&seen_batches);
                async move {
                    let ids: Vec<i64> = batch.iter().map(|user| user.id).collect();
                    seen_batches.lock().await.push(ids);

                    for mut user in batch {
                        user.name = format!("done-{}", user.id);
                        user.update().await?;
                    }

                    Ok(())
                }
            }
        })
        .await
        .expect("chunked iteration should succeed");

    assert_eq!(
        *seen_batches.lock().await,
        vec![vec![1, 2], vec![3, 4], vec![5, 6], vec![7]]
    );

    let remaining = MutationGuardUser::query()
        .where_like("name", "user-%")
        .count()
        .await
        .expect("post-chunk count should succeed");
    assert_eq!(remaining, 0);

    let updated = MutationGuardUser::query()
        .order_by("id", crate::query::Order::Asc)
        .get()
        .await
        .expect("final read should succeed");
    assert_eq!(updated.len(), 7);
    assert!(
        updated
            .iter()
            .all(|user| user.name == format!("done-{}", user.id))
    );

    crate::test_support::reset_globals_and_cache();
}

#[test]
fn query_fragment_carries_the_soft_delete_scope() {
    // A fragment's trash scope replaces the builder's.
    let with_trashed = SoftDeleteMutationGuardUser::query().with_trashed();
    let rebuilt = SoftDeleteMutationGuardUser::query()
        .only_trashed()
        .apply(&with_trashed.consolidate())
        .build_sql_preview();
    assert_eq!(rebuilt, with_trashed.build_sql_preview());
    assert!(!rebuilt.contains("WHERE"), "sql: {rebuilt}");

    let only_trashed = SoftDeleteMutationGuardUser::query()
        .apply(
            &SoftDeleteMutationGuardUser::query()
                .only_trashed()
                .consolidate(),
        )
        .build_sql_preview();
    assert!(
        only_trashed.ends_with("WHERE \"deleted_at\" IS NOT NULL"),
        "sql: {only_trashed}"
    );
}

#[test]
fn mutation_guard_rejects_a_query_with_no_restrictive_filter() {
    let mut empty_nested_group = MutationGuardUser::query();
    empty_nested_group
        .clauses
        .or_groups
        .push(OrGroup::new().nested_or(|group| group));

    for (case, result) in [
        (
            "no filter",
            MutationGuardUser::query().ensure_mutation_has_explicit_filters("delete"),
        ),
        (
            "only_trashed() alone",
            SoftDeleteMutationGuardUser::query()
                .only_trashed()
                .ensure_mutation_has_explicit_filters("restore"),
        ),
        (
            "an empty nested OR group",
            empty_nested_group.ensure_mutation_has_explicit_filters("delete"),
        ),
        // An empty candidate set for a negative membership test renders
        // constant-true. Reaching it by accident is easy -- a filter list that
        // came back empty from a form -- and the rendered-SQL check cannot
        // catch it: sea-query emits an empty `NOT IN` as the bound pair
        // `? = ?`, and a soft-delete model appends `deleted_at IS NULL` to
        // whatever the caller declared.
        (
            "an empty NOT IN list",
            MutationGuardUser::query()
                .where_not_in("id", Vec::<i64>::new())
                .ensure_mutation_has_explicit_filters("delete"),
        ),
        (
            "two empty NOT IN lists",
            MutationGuardUser::query()
                .where_not_in("name", Vec::<&str>::new())
                .where_not_in("id", Vec::<i64>::new())
                .ensure_mutation_has_explicit_filters("delete"),
        ),
        // A NOT IN list of NULLs and `where_not(col, None)` both render
        // `IS NOT NULL`, which keeps every row of a `NOT NULL` column.
        (
            "a NOT IN list of NULLs",
            MutationGuardUser::query()
                .where_not_in("id", vec![None::<i64>, None])
                .ensure_mutation_has_explicit_filters("delete"),
        ),
        (
            "where_not(None)",
            MutationGuardUser::query()
                .where_not("name", None::<String>)
                .ensure_mutation_has_explicit_filters("delete"),
        ),
    ] {
        let err = result.expect_err(case);
        assert!(
            err.to_string()
                .contains("requires at least one explicit filter"),
            "{case}: {err}"
        );
    }
}

#[test]
fn mutation_guard_accepts_any_restrictive_filter() {
    // An empty group renders nothing, so it cannot stop an `Or` group beside
    // a filter from restricting: `when(false, ..)` inside one used to make a
    // filtered delete look unfiltered.
    let mut filter_beside_empty_group = MutationGuardUser::query();
    filter_beside_empty_group
        .clauses
        .or_groups
        .push(OrGroup::new().where_eq("id", 1).nested_and(|group| group));

    for (case, query) in [
        (
            "a WHERE clause",
            MutationGuardUser::query().where_eq("id", 1),
        ),
        (
            "an OR group",
            MutationGuardUser::query()
                .begin_or()
                .or_where_eq("name", "alice")
                .end_or(),
        ),
        ("a filter beside an empty group", filter_beside_empty_group),
        // Only queries whose every filter is vacuous are refused.
        (
            "a real filter beside a vacuous one",
            MutationGuardUser::query()
                .where_eq("name", "alice")
                .where_not_in("id", Vec::<i64>::new()),
        ),
        (
            "a non-empty NOT IN list",
            MutationGuardUser::query().where_not_in("id", vec![1i64]),
        ),
        (
            "a NOT IN list holding a value beside a NULL",
            MutationGuardUser::query().where_not_in("id", vec![None, Some(1i64)]),
        ),
        // An empty IN list renders constant-false, which matches nothing and
        // is safe for a mutation.
        (
            "an empty IN list",
            MutationGuardUser::query().where_in("id", Vec::<i64>::new()),
        ),
    ] {
        query
            .ensure_mutation_has_explicit_filters("delete")
            .unwrap_or_else(|err| panic!("{case}: {err}"));
    }
}

#[test]
fn delete_all_refuses_where_filters() {
    assert!(
        MutationGuardUser::query()
            .ensure_mutation_has_no_explicit_filters("delete_all")
            .is_ok()
    );

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

/// A query is counted directly, keeping its joins and filters and dropping
/// its ordering and page; one whose rows are groups, or whose raw projection
/// may aggregate, is counted through a derived table.
#[test]
fn count_sql_counts_the_rows_the_query_returns() {
    let users = "\"query_count_guard_users\"";
    let alice = || Value::String(Some("alice".to_string()));

    for (query, expected, params) in [
        (
            QueryCountGuardUser::query()
                .inner_join("profiles", "query_count_guard_users.id", "profiles.user_id")
                .where_eq("profiles.active", true),
            format!(
                "SELECT COUNT(*) AS count FROM {users} INNER JOIN \"profiles\" ON {users}.\"id\" = \"profiles\".\"user_id\" WHERE \"profiles\".\"active\" = $1"
            ),
            vec![Value::Bool(Some(true))],
        ),
        (
            QueryCountGuardUser::query()
                .where_eq("name", "alice")
                .order_by("name", crate::query::Order::Asc)
                .limit(10)
                .offset(20),
            format!("SELECT COUNT(*) AS count FROM {users} WHERE \"name\" = $1"),
            vec![alice()],
        ),
        // One constant row per group: two grouping columns of one name
        // (`orders.status`, `customers.status`) would collide in the derived
        // table, which MySQL refuses.
        (
            QueryCountGuardUser::query().group_by("name"),
            format!(
                "SELECT COUNT(*) AS count FROM (SELECT 1 FROM {users} GROUP BY \"name\") AS \"tideorm_count_subquery\""
            ),
            vec![],
        ),
        (
            QueryCountGuardUser::query()
                .select(vec!["name"])
                .group_by("name")
                .having("COUNT(*) > 1"),
            format!(
                "SELECT COUNT(*) AS count FROM (SELECT {users}.\"name\" FROM {users} GROUP BY \"name\" HAVING COUNT(*) > 1) AS \"tideorm_count_subquery\""
            ),
            vec![],
        ),
        (
            QueryCountGuardUser::query()
                .select(vec!["name"])
                .group_by("name")
                .having(crate::Aggregate::count().gt(1)),
            format!(
                "SELECT COUNT(*) AS count FROM (SELECT {users}.\"name\" FROM {users} GROUP BY \"name\" HAVING COUNT(*) > $1) AS \"tideorm_count_subquery\""
            ),
            vec![Value::BigInt(Some(1))],
        ),
        (
            QueryCountGuardUser::query().select_raw("COUNT(*) AS total_count"),
            format!(
                "SELECT COUNT(*) AS count FROM (SELECT COUNT(*) AS total_count FROM {users}) AS \"tideorm_count_subquery\""
            ),
            vec![],
        ),
    ] {
        let (sql, bound) = query.build_count_sql_with_params_for_db(DatabaseType::Postgres);
        assert_eq!(sql, expected);
        assert_eq!(bound, params, "{sql}");
    }
}

/// `exists()` asks for one row of the query, ordering and page dropped, and
/// selects a constant unless the projection matters: HAVING may name an
/// explicit projection's alias.
#[test]
fn exists_sql_selects_a_constant_unless_having_reads_the_projection() {
    let users = "\"query_count_guard_users\"";

    for (query, inner, params) in [
        (
            QueryCountGuardUser::query()
                .where_eq("name", "alice")
                .order_by("name", crate::query::Order::Asc)
                .limit(10)
                .offset(20),
            format!("SELECT 1 FROM {users} WHERE \"name\" = $1"),
            vec![Value::String(Some("alice".to_string()))],
        ),
        (
            QueryCountGuardUser::query().select(vec!["name"]),
            format!("SELECT 1 FROM {users}"),
            vec![],
        ),
        (
            QueryCountGuardUser::query()
                .group_by("name")
                .having("COUNT(*) > 1"),
            format!("SELECT 1 FROM {users} GROUP BY \"name\" HAVING COUNT(*) > 1"),
            vec![],
        ),
        (
            QueryCountGuardUser::query()
                .select(vec!["name AS label"])
                .group_by("name")
                .having("COUNT(*) > 1"),
            format!(
                "SELECT {users}.\"name\" AS \"label\" FROM {users} GROUP BY \"name\" HAVING COUNT(*) > 1"
            ),
            vec![],
        ),
    ] {
        let (sql, bound) = query.build_exists_sql_with_params_for_db(DatabaseType::Postgres);
        assert_eq!(
            sql,
            format!("SELECT EXISTS({inner} LIMIT 1) AS \"exists_result\"")
        );
        assert_eq!(bound, params, "{sql}");
    }
}

#[test]
fn exists_sql_keeps_potential_aggregates_in_every_dialect() {
    for backend in [
        DatabaseType::Postgres,
        DatabaseType::MySQL,
        DatabaseType::SQLite,
    ] {
        for expression in ["COUNT(*) AS total", "COALESCE(SUM(id), 0) AS total"] {
            let (sql, params) = QueryCountGuardUser::query()
                .select_raw(expression)
                .where_eq("name", "alice")
                .build_exists_sql_with_params_for_db(backend);
            assert!(sql.contains(expression), "{sql}");
            assert_eq!(params.len(), 1);
        }
    }
}

#[test]
fn exists_sql_preserves_subquery_wrapper_for_union_queries() {
    let (sql, params) = QueryCountGuardUser::query()
        .where_eq("name", "alice")
        .union_all(QueryCountGuardUser::query().where_eq("name", "bob"))
        .build_exists_sql_with_params_for_db(DatabaseType::Postgres);

    // The union operand is parameterized like the base select, so it contributes
    // its own bound value instead of an inline literal.
    assert_eq!(params.len(), 2);
    assert!(matches!(params.first(), Some(Value::String(Some(name))) if name == "alice"));
    assert!(matches!(params.get(1), Some(Value::String(Some(name))) if name == "bob"));
    assert!(sql.starts_with("SELECT 1 FROM (SELECT "));
    assert!(sql.contains("UNION ALL (SELECT"));
    assert!(!sql.contains("'bob'"), "sql: {sql}");
    assert!(sql.contains("tideorm_exists_subquery"));
    assert!(sql.ends_with("LIMIT 1"));
}

#[test]
fn sql_preview_escapes_values_the_way_the_backend_reads_them() {
    let name = crate::columns::Column::<String>::new("name");
    let postgres = QueryCountGuardUser::query()
        .where_col(name.starts_with(r"100%_\done"))
        .build_sql_preview_for_db(DatabaseType::Postgres);
    assert!(
        postgres.ends_with(r#"WHERE "name" LIKE E'100!%!_\\done%' ESCAPE '!'"#),
        "{postgres}"
    );

    // MySQL reads `\'` as an escaped quote, so the backslash is doubled.
    let mysql = QueryCountGuardUser::query()
        .where_eq("name", r#"\' OR 1=1 --"#)
        .build_sql_preview_for_db(DatabaseType::MySQL);
    assert!(
        mysql.ends_with(r#"WHERE `name` = '\\\' OR 1=1 --'"#),
        "{mysql}"
    );
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
async fn query_delete_and_delete_all_invalidate_cached_queries() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    let _db = setup_query_mutation_cache_test_db().await;
    let seed = || async {
        MutationGuardUser {
            id: 0,
            name: "Alice".to_string(),
        }
        .save()
        .await
        .expect("seed save should succeed")
    };

    let saved = seed().await;
    assert_eq!(
        crate::test_support::cached_rows::<MutationGuardUser>()
            .await
            .len(),
        1
    );
    assert_eq!(crate::test_support::cached_entries(), 1);
    let deleted = MutationGuardUser::query()
        .where_eq("id", saved.id)
        .delete()
        .await
        .expect("query delete should succeed");
    assert_eq!(deleted, 1);
    assert_eq!(crate::test_support::cached_entries(), 0);

    seed().await;
    assert_eq!(
        crate::test_support::cached_rows::<MutationGuardUser>()
            .await
            .len(),
        1
    );
    assert_eq!(crate::test_support::cached_entries(), 1);
    let deleted = MutationGuardUser::query()
        .delete_all()
        .await
        .expect("query delete_all should succeed");
    assert_eq!(deleted, 1);
    assert_eq!(crate::test_support::cached_entries(), 0);

    crate::test_support::reset_globals_and_cache();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn soft_delete_restore_and_force_delete_invalidate_cached_queries() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    let _db = setup_query_mutation_cache_test_db().await;
    let cache_with_trashed = || async {
        let rows = SoftDeleteMutationGuardUser::query()
            .with_trashed()
            .order_by("id", crate::query::Order::Asc)
            .cache(Duration::from_secs(60))
            .get()
            .await
            .expect("a cached read with trashed rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(crate::test_support::cached_entries(), 1);
    };

    let saved = SoftDeleteMutationGuardUser {
        id: 0,
        name: "Alice".to_string(),
        deleted_at: None,
    }
    .save()
    .await
    .expect("seed save should succeed");
    let by_id = || {
        SoftDeleteMutationGuardUser::query()
            .with_trashed()
            .where_eq("id", saved.id)
    };

    assert_eq!(
        crate::test_support::cached_rows::<SoftDeleteMutationGuardUser>()
            .await
            .len(),
        1
    );
    assert_eq!(crate::test_support::cached_entries(), 1);
    let soft_deleted = SoftDeleteMutationGuardUser::query()
        .where_eq("id", saved.id)
        .soft_delete()
        .await
        .expect("soft_delete");
    assert_eq!(soft_deleted, 1);
    assert_eq!(crate::test_support::cached_entries(), 0);

    cache_with_trashed().await;
    let restored = by_id().restore().await.expect("restore");
    assert_eq!(restored, 1);
    assert_eq!(crate::test_support::cached_entries(), 0);

    cache_with_trashed().await;
    let force_deleted = by_id().force_delete().await.expect("force_delete");
    assert_eq!(force_deleted, 1);
    assert_eq!(crate::test_support::cached_entries(), 0);

    crate::test_support::reset_globals_and_cache();
}

/// A condition whose operator and value do not pair up has no SQL rendering,
/// and every path refuses it: the aggregates' SeaORM path used to drop it from
/// its WHERE clause, so the aggregate silently covered every row.
#[tokio::test]
async fn a_condition_that_cannot_be_rendered_is_refused() {
    let unrenderable = || {
        let mut query = MutationGuardUser::query();
        query.clauses.conditions.push(crate::query::WhereCondition {
            column: "id".to_string(),
            operator: crate::query::Operator::Between,
            value: crate::query::ConditionValue::Single(serde_json::json!(1)),
        });
        query
    };

    let sum = unrenderable().sum::<i64>("id").await.unwrap_err();
    assert!(
        sum.to_string().contains("cannot be rendered as SQL"),
        "{sum}"
    );

    let distinct = unrenderable().count_distinct("name").await.unwrap_err();
    assert!(
        distinct.to_string().contains("cannot be rendered as SQL"),
        "{distinct}"
    );

    let operand = MutationGuardUser::query()
        .where_exists(unrenderable())
        .ensure_query_is_valid()
        .unwrap_err()
        .to_string();
    assert!(
        operand.contains("invalid subquery for where_exists()")
            && operand.contains("cannot be rendered as SQL"),
        "{operand}"
    );
}

#[test]
fn sql_preview_is_the_executed_statement_with_its_values_inlined() {
    let query = SoftDeleteMutationGuardUser::query()
        .where_in("id", vec![1, 2])
        .with_query(
            "recent",
            MutationGuardUser::query().where_eq("name", "carol"),
        )
        .union_all(
            SoftDeleteMutationGuardUser::query()
                .with_trashed()
                .where_eq("name", "bob"),
        );

    let (sql, params) = query.build_select_sql_with_params_for_db(DatabaseType::Postgres);
    let preview = query.build_sql_preview_for_db(DatabaseType::Postgres);
    assert_eq!(
        preview,
        format!(
            "-- DEBUG PREVIEW (not executable, values are approximate)\n{}",
            crate::query::db_sql::inline_parameters(DatabaseType::Postgres, &sql, &params)
        )
    );

    // The preview shows the `IN` list the statement binds.
    assert!(preview.contains("\"id\" IN (1, 2)"), "{preview}");
    // Union and CTE operands used to preview their bare placeholders.
    assert!(
        preview.contains("WITH \"recent\" AS (SELECT") && preview.contains("\"name\" = 'carol'"),
        "{preview}"
    );
    assert!(
        preview.contains("UNION ALL (SELECT") && preview.contains("\"name\" = 'bob'"),
        "{preview}"
    );
    assert!(!preview.contains('$'), "{preview}");
    // The soft-delete scope comes from the executed condition tree too.
    assert!(preview.contains("\"deleted_at\" IS NULL"), "{preview}");
}

#[test]
fn sqlite_json_preview_compares_original_numeric_tokens() {
    let preview = MutationGuardUser::query()
        .where_json_contains("name", 5)
        .build_sql_preview_for_db(DatabaseType::SQLite);

    assert!(
        preview.contains("\"query_mutation_guard_users\".\"name\" -> '$'"),
        "{preview}"
    );
    assert!(
        preview.contains("coefficient = '5'") && preview.contains("scale = 0"),
        "{preview}"
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn aggregates_run_through_the_logged_statement_path() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    let _db = setup_query_mutation_cache_test_db().await;

    for name in ["a", "b", "b"] {
        MutationGuardUser {
            id: 0,
            name: name.to_string(),
        }
        .save()
        .await
        .expect("seed save should succeed");
    }

    crate::logging::QueryLogger::clear_history();
    crate::logging::QueryLogger::global()
        .set_level(crate::logging::LogLevel::Error)
        .set_history_limit(10)
        .enable();

    let distinct = MutationGuardUser::query().count_distinct("name").await;
    let total = MutationGuardUser::query()
        .where_eq("name", "b")
        .sum::<i64>("id")
        .await;
    let empty = MutationGuardUser::query()
        .where_eq("name", "nobody")
        .max::<i64>("id")
        .await;
    let history = crate::logging::QueryLogger::history();

    crate::logging::QueryLogger::clear_history();
    crate::logging::QueryLogger::global()
        .set_history_limit(100)
        .enable();
    crate::logging::QueryLogger::disable();
    crate::test_support::reset_globals_and_cache();

    assert_eq!(distinct.expect("count_distinct should succeed"), 2);
    assert_eq!(total.expect("sum should succeed"), 5);
    assert_eq!(empty.expect("a MAX over no rows is None"), None);
    // The SeaORM path the unjoined aggregates used to take bypassed the query log.
    assert!(
        history
            .iter()
            .any(|entry| entry.sql.contains("COUNT(DISTINCT")),
        "{history:?}"
    );
    assert!(
        history.iter().any(|entry| entry.sql.contains("SUM(")),
        "{history:?}"
    );
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn a_query_renders_for_the_transaction_it_runs_in() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::Database::reset_global();
    let db = crate::Database::connect("sqlite::memory:")
        .await
        .expect("sqlite should connect");

    let rendered = db
        .transaction(|_| Box::pin(async { Ok(MutationGuardUser::query().db_type_for_sql()) }))
        .await
        .expect("transaction should run");

    assert_eq!(rendered, crate::config::DatabaseType::SQLite);
}
