use crate::config::DatabaseType;
use crate::internal::InternalConnection;
use crate::model::Model;
use crate::query::{
    FrameBound, FrameType, Order, QueryBuilder, WindowFunction, WindowFunctionType,
};
use std::sync::Arc;

#[tideorm::model(table = "cache_key_test_users")]
struct CacheKeyTestUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "cache_key_test_posts")]
struct CacheKeyTestPost {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    title: String,
}

#[tideorm::model(table = "cache_key_test_soft_delete_users", soft_delete)]
struct CacheKeyTestSoftDeleteUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[test]
fn test_generate_cache_key_is_stable_for_equivalent_structured_queries() {
    let build = || {
        CacheKeyTestUser::query()
            .where_in("status", vec!["active", "pending"])
            .where_any(|group| {
                group
                    .where_eq("role", "admin")
                    .nested_and(|inner| inner.where_gt("score", 10).where_lt("score", 20))
            })
            .window(
                WindowFunction::new(
                    WindowFunctionType::Lag("score".to_string(), Some(1), Some("0".to_string())),
                    "previous_score",
                )
                .partition_by("team")
                .order_by("score", Order::Desc)
                .frame(
                    FrameType::Rows,
                    FrameBound::UnboundedPreceding,
                    FrameBound::CurrentRow,
                ),
            )
            .limit(10)
    };

    assert_eq!(build().generate_cache_key(), build().generate_cache_key());
}

#[test]
fn test_generate_cache_key_changes_when_window_definition_changes() {
    let baseline = CacheKeyTestUser::query().window(
        WindowFunction::new(WindowFunctionType::Rank, "rank_alias").order_by("score", Order::Desc),
    );
    let changed = CacheKeyTestUser::query().window(
        WindowFunction::new(WindowFunctionType::DenseRank, "rank_alias")
            .order_by("score", Order::Desc),
    );

    assert_ne!(baseline.generate_cache_key(), changed.generate_cache_key());
}

#[test]
fn test_explicit_cache_key_is_namespaced_per_model() {
    let ttl = std::time::Duration::from_secs(60);
    let user_query = CacheKeyTestUser::query().cache_with_key("recent", ttl);
    let post_query = CacheKeyTestPost::query().cache_with_key("recent", ttl);

    assert_ne!(
        user_query.generate_cache_key(),
        post_query.generate_cache_key()
    );
}

/// A parameterized operand renders the same SQL whatever value it binds, so
/// its values have to reach the cache key, or two queries differing only in
/// one would share a cache entry.
#[test]
fn test_operand_bound_values_participate_in_the_cache_key() {
    let tenant = |id: i64| {
        CacheKeyTestUser::query()
            .select(vec!["id"])
            .where_eq("id", id)
    };

    for (operand, one, two) in [
        (
            "union",
            CacheKeyTestPost::query().union(tenant(1)),
            CacheKeyTestPost::query().union(tenant(2)),
        ),
        (
            "CTE",
            CacheKeyTestPost::query().with_query("tenant_users", tenant(1)),
            CacheKeyTestPost::query().with_query("tenant_users", tenant(2)),
        ),
        (
            "scalar subquery",
            CacheKeyTestPost::query().select_subquery(tenant(1), "owner"),
            CacheKeyTestPost::query().select_subquery(tenant(2), "owner"),
        ),
        (
            "EXISTS subquery",
            CacheKeyTestPost::query().where_exists(tenant(1)),
            CacheKeyTestPost::query().where_exists(tenant(2)),
        ),
    ] {
        assert_eq!(
            one.build_select_sql_with_params_for_db(DatabaseType::Postgres)
                .unwrap()
                .0,
            two.build_select_sql_with_params_for_db(DatabaseType::Postgres)
                .unwrap()
                .0,
            "{operand}"
        );
        assert_ne!(
            one.generate_cache_key(),
            two.generate_cache_key(),
            "{operand}"
        );
    }
}

/// Queries of `M` whose filters fold to TRUE, alone or as a branch of an OR.
fn filters_that_fold_to_true<M: Model>() -> Vec<(&'static str, QueryBuilder<M>)> {
    let no_ids: Vec<i64> = Vec::new();
    vec![
        ("no filter", M::query()),
        ("raw TRUE", M::query().where_raw("TRUE")),
        ("raw (1 = 1)", M::query().where_raw("(1 = 1)")),
        ("empty NOT IN", M::query().where_not_in("id", no_ids)),
        (
            "OR with a TRUE branch",
            M::query().or_where_eq("id", 1).or_where_raw("1 = 1"),
        ),
    ]
}

fn guard_refuses<M: Model>(query: &QueryBuilder<M>, trash_scope_counts: bool) -> bool {
    query
        .mutation_where_bound(DatabaseType::Postgres, "delete", trash_scope_counts)
        .is_err_and(|err| {
            err.to_string()
                .contains("unfiltered bulk mutations are blocked")
        })
}

/// The guard folds the condition the filters build: a filter that folds to
/// TRUE, alone or as a branch of an OR, cannot exclude a row. On a
/// soft-delete model the default scope's `deleted_at IS NULL` is no filter of
/// the caller's, and ANDed in it hid the TRUE; `only_trashed()` counts only
/// where the trash may be restored or emptied as a whole (`restore()`,
/// `force_delete()`), and any other mutation in the trash needs a filter.
#[test]
fn test_mutation_where_guard_rejects_conditions_that_fold_to_true() {
    for (label, query) in filters_that_fold_to_true::<CacheKeyTestUser>() {
        assert!(guard_refuses(&query, false), "{label}");
    }
    for (label, query) in filters_that_fold_to_true::<CacheKeyTestSoftDeleteUser>() {
        for trash_scope_counts in [false, true] {
            assert!(guard_refuses(&query, trash_scope_counts), "{label}");
            assert!(
                guard_refuses(&query.clone().with_trashed(), trash_scope_counts),
                "{label}, with trashed rows"
            );
        }
        let trash = query.only_trashed();
        assert!(
            !guard_refuses(&trash, true),
            "{label}, the whole trash restored or emptied"
        );
        assert!(
            guard_refuses(&trash, false),
            "{label}, any other mutation of the whole trash"
        );
    }

    let (sql, params) = CacheKeyTestUser::query()
        .where_eq("id", 1)
        .mutation_where_bound(DatabaseType::Postgres, "delete", false)
        .unwrap()
        .build(DatabaseType::Postgres.dialect());
    assert!(sql.contains("= $1"), "{sql}");
    assert_eq!(params.len(), 1);
}

/// A write to any table a query reads must evict its cached result, so the
/// cache tags cover joined tables and union operands' tables too.
#[test]
fn test_cache_tags_cover_joined_and_union_operand_tables() {
    let posts = "cache_key_test_posts".to_string();
    let users = "cache_key_test_users".to_string();

    let joined = CacheKeyTestPost::query().inner_join(
        "cache_key_test_users",
        "cache_key_test_posts.id",
        "cache_key_test_users.id",
    );
    assert_eq!(joined.cache_tables(), vec![posts.clone(), users.clone()]);

    for composed in [
        CacheKeyTestPost::query().union(CacheKeyTestUser::query()),
        CacheKeyTestPost::query().union_all(CacheKeyTestUser::query()),
    ] {
        assert!(composed.clauses.dependencies.contains(&users));
        assert!(composed.cache_tables().contains(&users));
    }
}

#[test]
fn test_collect_tables_from_sql_reads_every_table_a_statement_names() {
    for (sql, expected) in [
        (
            "SELECT * FROM \"orders\" INNER JOIN `line_items` ON a = b \
             WHERE id IN (SELECT id FROM public.archived_orders)",
            &["orders", "line_items", "archived_orders"][..],
        ),
        // Every table of a FROM list, but not words in a literal.
        (
            "SELECT * FROM a, b AS bee, (SELECT 1 FROM c) d, e \
             WHERE x IN (SELECT y FROM f, g) AND z = 'from h, i'",
            &["a", "b", "c", "e", "f", "g"],
        ),
        (
            "SELECT * FROM ONLY posts WHERE id IN (SELECT post_id FROM ONLY \"comments\")",
            &["posts", "comments"],
        ),
        ("SELECT * FROM (SELECT * FROM \"users\") t", &["users"]),
        // A quoted name is kept whole, and a comment is not read.
        ("SELECT * FROM \"group by\" /* FROM fake */", &["group by"]),
        // A comma after a join continues the FROM list; one inside an
        // expression or an ORDER BY does not.
        (
            "SELECT * FROM a JOIN b ON a.id=b.id, c WHERE coalesce(c.id, 0)>0 ORDER BY a.id, ignored",
            &["a", "b", "c"],
        ),
        (
            "SELECT * FROM a LEFT JOIN b USING(id), c WHERE coalesce(c.id, 0)>0 ORDER BY a.id, ignored",
            &["a", "b", "c"],
        ),
        (
            "SELECT * FROM a CROSS JOIN b, c WHERE coalesce(c.id, 0)>0 ORDER BY a.id, ignored",
            &["a", "b", "c"],
        ),
    ] {
        let mut tables = Vec::new();
        super::collect_tables_from_sql(sql, &mut tables);
        assert_eq!(tables, expected, "{sql}");
    }
}

#[test]
fn test_exists_flag_decoding_reports_undecodable_values() {
    type Decoder = QueryBuilder<CacheKeyTestUser>;

    assert!(Decoder::decode_exists_flag(&serde_json::json!(true)).unwrap());
    assert!(!Decoder::decode_exists_flag(&serde_json::json!(false)).unwrap());
    assert!(Decoder::decode_exists_flag(&serde_json::json!(1)).unwrap());
    assert!(!Decoder::decode_exists_flag(&serde_json::json!(0)).unwrap());

    // An EXISTS query always returns one row, so guessing `true` from "a row
    // came back" would make every undecodable result a false positive.
    let err = Decoder::decode_exists_flag(&serde_json::json!(null)).unwrap_err();
    assert!(
        err.to_string().contains("Unable to decode"),
        "an undecodable EXISTS result must be reported: {err}"
    );
}

#[test]
fn test_count_decoding_reports_missing_and_undecodable_values() {
    type Decoder = QueryBuilder<CacheKeyTestUser>;

    let seven = serde_json::json!(7);
    assert_eq!(
        Decoder::decode_count_value(Some(&seven), "count").unwrap(),
        7
    );

    let missing = Decoder::decode_count_value(None, "count").unwrap_err();
    assert!(
        missing.to_string().contains("no 'count' column"),
        "{missing}"
    );

    let text = serde_json::json!("many");
    let undecodable = Decoder::decode_count_value(Some(&text), "count").unwrap_err();
    assert!(
        undecodable.to_string().contains("Unable to decode"),
        "{undecodable}"
    );

    let negative_value = serde_json::json!(-1);
    let negative = Decoder::decode_count_value(Some(&negative_value), "count").unwrap_err();
    assert!(
        negative.to_string().contains("negative count"),
        "{negative}"
    );
}

/// A lazy pool of the compiled backend that never dials, for identity
/// bookkeeping only.
async fn idle_connection() -> Arc<InternalConnection> {
    let url = if cfg!(feature = "postgres") {
        "postgres://tideorm@127.0.0.1:9/unused"
    } else if cfg!(feature = "mysql") {
        "mysql://tideorm@127.0.0.1:9/unused"
    } else {
        "sqlite::memory:"
    };
    let mut options = crate::internal::ConnectOptions::new(url);
    options.connect_lazy(true);
    let connection = crate::internal::OrmDatabase::connect(options)
        .await
        .expect("a lazy pool opens without dialling");
    Arc::new(InternalConnection::new(connection))
}

#[tokio::test]
async fn test_connection_identity_is_stable_for_one_connection() {
    let connection = idle_connection().await;
    let same_connection = Arc::clone(&connection);

    assert_eq!(
        connection.connection().identity(),
        same_connection.connection().clone().identity(),
        "one connection must keep one identity, or its own cache entries never hit"
    );
}

#[tokio::test]
async fn test_connection_identity_is_never_inherited_from_a_dropped_pool() {
    let first = idle_connection().await;
    let first_identity = first.connection().identity();
    drop(first);

    // A batch of identically-sized allocations is exactly what lands on a
    // just-freed block, which is how hashing `Arc::as_ptr` served one
    // tenant's cached rows to the next tenant to connect.
    for _ in 0..16 {
        assert_ne!(
            idle_connection().await.connection().identity(),
            first_identity,
            "a new connection must never inherit a dropped connection's cache identity"
        );
    }
}

/// The soft-delete stamp is bound, before the filter's values, so every
/// soft delete of a model runs one statement text and a stamp is stored as
/// any typed write of a `DateTime<Utc>` stores it.
#[test]
fn test_soft_delete_binds_its_stamp() {
    let query = CacheKeyTestSoftDeleteUser::query().where_eq("id", 1);
    for (db_type, stamp_marker, id_marker) in [
        (DatabaseType::Postgres, "= $1", "= $2"),
        (DatabaseType::MySQL, "= ?", "= ?"),
        (DatabaseType::MariaDB, "= ?", "= ?"),
        (DatabaseType::SQLite, "= ?", "= ?"),
    ] {
        let (sql, params) = query
            .build_soft_delete_sql(db_type)
            .expect("a filtered soft delete renders");
        assert!(!sql.contains('\''), "{db_type:?}: {sql}");
        let (set, filter) = sql.split_once(" WHERE ").expect("an UPDATE with a WHERE");
        assert!(set.ends_with(stamp_marker), "{db_type:?}: {sql}");
        assert!(filter.contains(id_marker), "{db_type:?}: {sql}");
        assert!(
            matches!(
                params.first(),
                Some(crate::internal::Value::ChronoDateTimeUtc(Some(_)))
            ),
            "{db_type:?}: {params:?}"
        );
        assert_eq!(params.len(), 2, "{db_type:?}: {params:?}");
    }
}

#[tokio::test]
async fn only_trashed_is_refused_for_a_force_delete_without_soft_delete() {
    // The model has no trash, so the scope would be dropped and the delete
    // would reach the live row with id 5.
    let err = CacheKeyTestUser::query()
        .only_trashed()
        .where_eq("id", 5)
        .force_delete()
        .await
        .unwrap_err();
    assert!(err.to_string().contains("soft delete"), "{err}");
}

#[test]
fn a_distinct_aggregate_runs_over_the_distinct_rows() {
    let (sql, _) = CacheKeyTestUser::query()
        .distinct()
        .select(vec!["name"])
        .build_aggregate_sql_with_params_for_db(DatabaseType::Postgres, "name", "names", |column| {
            format!("COUNT({column})")
        })
        .expect("aggregate sql");
    assert!(sql.contains("FROM (SELECT DISTINCT"), "{sql}");
}

#[test]
fn test_cache_keys_preserve_binding_representations() {
    use crate::internal::Value;
    let key = |value: Value| {
        CacheKeyTestUser::query()
            .where_raw_with("id = ?", vec![value])
            .generate_cache_key()
    };
    let a = Value::from(rust_decimal::Decimal::new(10, 1));
    let b = Value::from(rust_decimal::Decimal::new(100, 2));
    assert_ne!(key(a.clone()), key(b));
    assert_eq!(key(a.clone()), key(a));
    let a = chrono::DateTime::parse_from_rfc3339("2026-11-01T12:00:00+00:00").unwrap();
    let b = chrono::DateTime::parse_from_rfc3339("2026-11-01T14:00:00+02:00").unwrap();
    assert_ne!(key(a.into()), key(b.into()));
    assert_ne!(key(Value::Int(None)), key(Value::BigInt(None)));
    #[cfg(feature = "postgres")]
    {
        use crate::orm::sql_builder::ArrayType;
        assert_ne!(
            key(Value::Array(ArrayType::Int, Some(Box::default()))),
            key(Value::Array(ArrayType::BigInt, Some(Box::default())))
        );
    }
}
