use super::*;

#[tokio::test]
async fn sqlite_query_with_and_find_with_work_without_global_db() {
    use tideorm::internal::{ActiveModelTrait, InternalModel};

    let db = local_db_with(CI_USERS_DDL).await;
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    let inserted = CiUser {
        id: 0,
        email: "local@example.com".to_string(),
        name: "Local User".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed local user");

    let fetched = CiUser::find_with(inserted.id, &db)
        .await
        .expect("find_with failed")
        .expect("local user should exist");
    assert_eq!(fetched.email, "local@example.com");

    let queried = CiUser::query_with(&db)
        .where_eq("email", "local@example.com")
        .get()
        .await
        .expect("query_with failed");
    assert_eq!(queried.len(), 1);
    assert_eq!(queried[0].name, "Local User");
}

#[tokio::test]
async fn sqlite_query_with_supports_aggregate_queries_without_global_db() {
    use tideorm::internal::{ActiveModelTrait, InternalModel};

    let db = local_db_with(CI_USERS_DDL).await;
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    let first = CiUser {
        id: 0,
        email: "aggregate1@example.com".to_string(),
        name: "Aggregate One".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed first local user");

    let second = CiUser {
        id: 0,
        email: "aggregate2@example.com".to_string(),
        name: "Aggregate Two".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed second local user");

    let third = CiUser {
        id: 0,
        email: "aggregate3@example.com".to_string(),
        name: "Aggregate Three".to_string(),
        active: false,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed third local user");

    let active_sum: i64 = CiUser::query_with(&db)
        .where_eq("active", true)
        .sum("id")
        .await
        .expect("sum with explicit db should succeed");
    assert_eq!(active_sum, first.id + second.id);

    let inactive_distinct = CiUser::query_with(&db)
        .where_eq("active", false)
        .count_distinct("email")
        .await
        .expect("count_distinct with explicit db should succeed");
    assert_eq!(inactive_distinct, 1);

    let total_avg: Option<f64> = CiUser::query_with(&db)
        .avg("id")
        .await
        .expect("avg with explicit db should succeed");
    assert_eq!(
        total_avg,
        Some((first.id + second.id + third.id) as f64 / 3.0)
    );
}

#[tokio::test]
async fn sqlite_query_errors_include_query_builder_context() {
    let db = local_db_with(CI_USERS_DDL).await;

    let err = CiUser::query_with(&db)
        .where_eq("active", true)
        .begin_or()
        .or_where_raw("not_a_real_sql_function() = 1")
        .or_where_eq("email", "broken@example.com")
        .end_or()
        .get()
        .await
        .expect_err("query should fail due to invalid SQL function");

    let ctx = err.context().expect("query errors should include context");
    assert_eq!(ctx.table.as_deref(), Some("ci_users"));
    assert!(
        ctx.conditions
            .iter()
            .any(|condition| condition == "active = true"),
        "expected rendered top-level condition in context: {:?}",
        ctx.conditions
    );
    assert!(
        ctx.conditions.iter().any(
            |condition| condition.contains("not_a_real_sql_function() = 1")
                && condition.contains("OR")
        ),
        "expected rendered OR-group condition in context: {:?}",
        ctx.conditions
    );
    assert!(
        ctx.operator_chain
            .as_deref()
            .is_some_and(|chain| chain.contains("active = true")
                && chain.contains("not_a_real_sql_function() = 1")
                && chain.contains("OR")),
        "expected logical operator chain in context: {:?}",
        ctx.operator_chain
    );
    assert!(
        ctx.query
            .as_deref()
            .is_some_and(|query| query.contains("not_a_real_sql_function() = 1")),
        "expected SQL query preview in context: {:?}",
        ctx.query
    );
}

#[tokio::test]
async fn sqlite_uncached_queries_do_not_touch_global_query_cache() {
    use tideorm::QueryCache;
    use tideorm::internal::{ActiveModelTrait, InternalModel};

    let cache = QueryCache::global();
    cache.clear();
    cache.reset_stats();
    cache.enable();

    let db = local_db_with(CI_USERS_DDL).await;
    let conn = db
        .__internal_connection()
        .expect("local SQLite connection should be available");

    CiUser {
        id: 0,
        email: "uncached@example.com".to_string(),
        name: "Uncached User".to_string(),
        active: true,
    }
    .into_active_model()
    .insert(&conn)
    .await
    .expect("failed to seed uncached user");

    let results = CiUser::query_with(&db)
        .where_eq("email", "uncached@example.com")
        .get()
        .await
        .expect("uncached query should succeed");

    assert_eq!(results.len(), 1);

    let stats = cache.stats();
    assert_eq!(stats.hits, 0, "uncached queries should not read the cache");
    assert_eq!(
        stats.misses, 0,
        "uncached queries should not probe the cache"
    );
    assert_eq!(
        stats.entries, 0,
        "uncached queries should not populate the cache"
    );
    assert!(
        cache.is_empty(),
        "uncached queries should leave the cache empty"
    );

    cache.disable();
    cache.clear();
    cache.reset_stats();
}

#[tokio::test]
async fn sqlite_model_crud_errors_include_model_context() {
    connect_global().await;

    let find_err = CiUser::find(1)
        .await
        .expect_err("find should fail when the table is missing");
    let find_ctx = find_err
        .context()
        .expect("find errors should include model context");
    assert_eq!(find_ctx.table.as_deref(), Some("ci_users"));
    assert_eq!(find_ctx.conditions, vec!["id = 1".to_string()]);
    assert_eq!(find_ctx.operator_chain.as_deref(), Some("id = 1"));
    assert!(
        find_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("find(id = 1)")),
        "expected model operation in context: {:?}",
        find_ctx.query
    );

    let update_err = CiUser {
        id: 7,
        email: "update@example.com".to_string(),
        name: "Update Fail".to_string(),
        active: true,
    }
    .update()
    .await
    .expect_err("update should fail when the table is missing");
    let update_ctx = update_err
        .context()
        .expect("update errors should include model context");
    assert_eq!(update_ctx.table.as_deref(), Some("ci_users"));
    assert_eq!(update_ctx.conditions, vec!["id = 7".to_string()]);
    assert_eq!(update_ctx.operator_chain.as_deref(), Some("id = 7"));
    assert!(
        update_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("update where id = 7")),
        "expected update operation in context: {:?}",
        update_ctx.query
    );
}

#[tokio::test]
async fn sqlite_model_helper_errors_include_context() {
    connect_global().await;

    let all_err = CiUser::all()
        .await
        .expect_err("all should fail when the table is missing");
    let all_ctx = all_err
        .context()
        .expect("all errors should include model context");
    assert_eq!(all_ctx.table.as_deref(), Some("ci_users"));
    assert!(
        all_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("find_all()")),
        "expected helper operation in context: {:?}",
        all_ctx.query
    );

    let count_err = CiUser::count()
        .await
        .expect_err("count should fail when the table is missing");
    let count_ctx = count_err
        .context()
        .expect("count errors should include model context");
    assert_eq!(count_ctx.table.as_deref(), Some("ci_users"));
    assert!(
        count_ctx
            .query
            .as_deref()
            .is_some_and(|query| query.contains("count(*)")),
        "expected helper operation in context: {:?}",
        count_ctx.query
    );
}
