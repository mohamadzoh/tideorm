use crate::config::DatabaseType;
use crate::internal::InternalConnection;
use crate::internal::Value;
use crate::model::Model;
use crate::query::{
    ConditionValue, FrameBound, FrameType, Operator, Order, QueryBuilder, WhereCondition,
    WindowFunction, WindowFunctionType,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
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
    let query_one = CacheKeyTestUser::query()
        .where_in("status", vec!["active", "pending"])
        .or_where(|group| {
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
        .limit(10);

    let query_two = CacheKeyTestUser::query()
        .where_in("status", vec!["active", "pending"])
        .or_where(|group| {
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
        .limit(10);

    assert_eq!(
        query_one.generate_cache_key(),
        query_two.generate_cache_key()
    );
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

#[test]
fn test_rendered_filter_guard_rejects_constant_true_bodies() {
    type Guard = QueryBuilder<CacheKeyTestUser>;

    for body in ["", "TRUE", "true", "1 = 1", "(TRUE)"] {
        let err = Guard::ensure_rendered_filter_is_restrictive("delete", body).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("unfiltered bulk mutations are blocked"),
            "body: {body}"
        );
    }

    let restrictive = Guard::ensure_rendered_filter_is_restrictive("delete", "\"id\" = $1");
    assert!(restrictive.is_ok());
}

#[test]
fn test_empty_negative_list_does_not_count_as_an_explicit_filter() {
    // An empty candidate set for a negative membership test renders
    // constant-true, so it must not satisfy the explicit-filter requirement.
    // Reaching this by accident is easy -- a filter list that came back empty
    // from a form -- and the rendered-SQL check cannot catch it: sea-query
    // emits an empty `NOT IN` as the bound pair `? = ?`, and a soft-delete
    // model appends `deleted_at IS NULL` to whatever the caller declared.
    for query in [
        CacheKeyTestUser::query().where_not_in("id", Vec::<i64>::new()),
        CacheKeyTestUser::query().where_not_in("name", Vec::<&str>::new()),
        CacheKeyTestUser::query()
            .where_not_in("name", Vec::<&str>::new())
            .where_not_in("id", Vec::<i64>::new()),
    ] {
        let err = query
            .ensure_mutation_has_explicit_filters("delete")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("unfiltered bulk mutations are blocked"),
            "vacuous filter was accepted: {err}"
        );
    }
}

#[test]
fn test_a_real_filter_still_counts_alongside_a_vacuous_one() {
    // The guard rejects only queries where *every* declared filter is
    // vacuous; one real predicate is enough, and it still applies.
    assert!(
        CacheKeyTestUser::query()
            .where_eq("name", "alice")
            .where_not_in("id", Vec::<i64>::new())
            .ensure_mutation_has_explicit_filters("delete")
            .is_ok()
    );

    // A non-empty candidate set is a real filter on its own.
    assert!(
        CacheKeyTestUser::query()
            .where_not_in("id", vec![1i64])
            .ensure_mutation_has_explicit_filters("delete")
            .is_ok()
    );

    // The positive duals render constant-FALSE, which matches nothing and is
    // safe for a mutation, so they are deliberately still accepted.
    assert!(
        CacheKeyTestUser::query()
            .where_in("id", Vec::<i64>::new())
            .ensure_mutation_has_explicit_filters("delete")
            .is_ok()
    );
}

#[test]
fn test_cache_tags_cover_joined_tables() {
    let tables = CacheKeyTestPost::query()
        .inner_join(
            "cache_key_test_users",
            "cache_key_test_posts.id",
            "cache_key_test_users.id",
        )
        .cache_tables();

    assert_eq!(
        tables,
        vec![
            "cache_key_test_posts".to_string(),
            "cache_key_test_users".to_string()
        ]
    );
}

#[test]
fn test_write_to_joined_table_evicts_cached_join_result() {
    let cache = crate::cache::QueryCache::new();
    cache.enable();

    let joined = CacheKeyTestPost::query().inner_join(
        "cache_key_test_users",
        "cache_key_test_posts.id",
        "cache_key_test_users.id",
    );
    let joined_key = joined.generate_cache_key();
    cache
        .set_tagged(&joined_key, &["row"], None, &joined.cache_tables())
        .unwrap();

    let posts_only = CacheKeyTestPost::query();
    let posts_only_key = posts_only.generate_cache_key();
    cache
        .set_tagged(&posts_only_key, &["row"], None, &posts_only.cache_tables())
        .unwrap();

    assert!(cache.contains(&joined_key));
    assert!(cache.contains(&posts_only_key));

    cache.invalidate_model("cache_key_test_users");

    assert!(
        !cache.contains(&joined_key),
        "a write to a joined table must evict the cached join result"
    );
    assert!(
        cache.contains(&posts_only_key),
        "invalidation must stay targeted at the tables a query actually reads"
    );
}

#[test]
fn test_cache_tags_cover_union_operand_tables() {
    let tables = CacheKeyTestPost::query()
        .union(CacheKeyTestUser::query())
        .cache_tables();

    assert!(
        tables.contains(&"cache_key_test_users".to_string()),
        "a union operand's table must be tagged for invalidation: {tables:?}"
    );
}

#[test]
fn test_collect_tables_from_sql_reads_from_and_join_targets() {
    let mut tables = Vec::new();
    super::collect_tables_from_sql(
        "SELECT * FROM \"orders\" INNER JOIN `line_items` ON a = b \
         WHERE id IN (SELECT id FROM public.archived_orders)",
        &mut tables,
    );

    assert_eq!(
        tables,
        vec![
            "orders".to_string(),
            "line_items".to_string(),
            "archived_orders".to_string()
        ]
    );
}

#[test]
fn test_collect_tables_from_sql_reads_every_table_of_a_from_list() {
    let mut tables = Vec::new();
    super::collect_tables_from_sql(
        "SELECT * FROM a, b AS bee, (SELECT 1 FROM c) d, e \
         WHERE x IN (SELECT y FROM f, g) AND z = 'from h, i'",
        &mut tables,
    );

    assert_eq!(tables, ["a", "b", "c", "e", "f", "g"]);
}

#[test]
fn test_collect_tables_from_sql_reads_the_table_after_only() {
    let mut tables = Vec::new();
    super::collect_tables_from_sql(
        "SELECT * FROM ONLY posts WHERE id IN (SELECT post_id FROM ONLY \"comments\")",
        &mut tables,
    );

    assert_eq!(tables, ["posts", "comments"]);
}

#[test]
fn test_collect_tables_from_sql_skips_derived_table_keywords() {
    let mut tables = Vec::new();
    super::collect_tables_from_sql("SELECT * FROM (SELECT * FROM \"users\") t", &mut tables);

    assert_eq!(tables, vec!["users".to_string()]);
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

#[test]
fn test_union_bound_values_participate_in_the_cache_key() {
    let tenant_one = CacheKeyTestPost::query().union(CacheKeyTestUser::query().where_eq("id", 1));
    let tenant_two = CacheKeyTestPost::query().union(CacheKeyTestUser::query().where_eq("id", 2));

    assert_eq!(
        tenant_one.clauses.unions[0].query_sql, tenant_two.clauses.unions[0].query_sql,
        "a parameterized union operand renders the same SQL for either bound value"
    );
    assert_ne!(
        tenant_one.generate_cache_key(),
        tenant_two.generate_cache_key(),
        "two unions differing only in a bound value must not share a cache entry"
    );
}

#[test]
fn test_cte_bound_values_participate_in_the_cache_key() {
    let tenant_one = CacheKeyTestPost::query()
        .with_query("tenant_users", CacheKeyTestUser::query().where_eq("id", 1));
    let tenant_two = CacheKeyTestPost::query()
        .with_query("tenant_users", CacheKeyTestUser::query().where_eq("id", 2));

    assert_eq!(
        tenant_one.clauses.ctes[0].query_sql, tenant_two.clauses.ctes[0].query_sql,
        "a parameterized CTE body renders the same SQL for either bound value"
    );
    assert_ne!(
        tenant_one.generate_cache_key(),
        tenant_two.generate_cache_key(),
        "two CTE bodies differing only in a bound value must not share a cache entry"
    );
}

/// A connection that never dialled anything, for identity bookkeeping only.
fn disconnected_connection() -> Arc<InternalConnection> {
    Arc::new(InternalConnection::new(Default::default()))
}

#[test]
fn test_connection_identity_is_stable_for_one_connection() {
    let connection = disconnected_connection();
    let same_connection = Arc::clone(&connection);

    assert_eq!(
        crate::database::connection_identity(&connection),
        crate::database::connection_identity(&same_connection),
        "one connection must keep one identity, or its own cache entries never hit"
    );
}

#[test]
fn test_connection_identity_is_never_inherited_from_a_recycled_address() {
    let first = disconnected_connection();
    let first_address = Arc::as_ptr(&first) as usize;
    let first_identity = crate::database::connection_identity(&first);
    drop(first);

    // A batch of identically-sized allocations is exactly what lands on a
    // just-freed block, which is how hashing `Arc::as_ptr` served one
    // tenant's cached rows to the next tenant to connect.
    let reopened: Vec<_> = (0..16).map(|_| disconnected_connection()).collect();

    for connection in &reopened {
        assert_ne!(
            Arc::as_ptr(connection) as usize,
            first_address,
            "an identity on record must keep its connection's address reserved"
        );
        assert_ne!(
            crate::database::connection_identity(connection),
            first_identity,
            "a new connection must never inherit a dropped connection's cache identity"
        );
    }
}

/// The soft-delete stamp is a literal, so it has to be spelled the way the
/// backend the statement is heading for spells it. Rendering it for the
/// ambient backend instead put an RFC3339 literal — `T` separator, UTC
/// offset — into statements bound for MySQL, which rejects both.
#[test]
fn test_soft_delete_stamp_renders_for_the_statement_backend() {
    // The stamp is the only quoted token in the statement: every filter
    // value travels beside it as a bound parameter.
    fn stamp_of(sql: &str) -> &str {
        sql.split('\'')
            .nth(1)
            .expect("the soft-delete statement embeds a quoted timestamp literal")
    }

    let query = CacheKeyTestSoftDeleteUser::query().where_eq("id", 1);

    let (mysql_sql, _) = query
        .build_soft_delete_sql(DatabaseType::MySQL)
        .expect("a filtered soft delete renders");
    let mysql_stamp = stamp_of(&mysql_sql);
    assert!(!mysql_stamp.contains('T'), "mysql stamp: {mysql_stamp}");
    assert!(!mysql_stamp.contains('+'), "mysql stamp: {mysql_stamp}");

    let (postgres_sql, _) = query
        .build_soft_delete_sql(DatabaseType::Postgres)
        .expect("a filtered soft delete renders");
    let postgres_stamp = stamp_of(&postgres_sql);
    assert!(
        postgres_stamp.contains('T'),
        "postgres stamp: {postgres_stamp}"
    );
    assert!(
        postgres_stamp.ends_with("+00:00"),
        "postgres stamp: {postgres_stamp}"
    );
}

#[test]
fn test_unrepresentable_condition_is_rejected_instead_of_dropped() {
    let mut query = CacheKeyTestUser::query();
    query.clauses.conditions.push(WhereCondition {
        column: "id".to_string(),
        operator: Operator::Between,
        value: ConditionValue::Single(serde_json::json!(1)),
    });

    let err = query.ensure_conditions_are_representable().unwrap_err();
    assert!(err.to_string().contains("cannot be rendered as SQL"));
}

fn condition_hash(condition: &WhereCondition) -> u64 {
    let mut hasher = DefaultHasher::new();
    super::hash_where_condition(condition, &mut hasher);
    hasher.finish()
}

fn bound_subquery_condition(bound: &str) -> WhereCondition {
    WhereCondition {
        column: String::new(),
        operator: Operator::Raw,
        value: ConditionValue::RawExprWithValues {
            sql: "EXISTS (SELECT 1 FROM \"posts\" WHERE \"title\" = $1)".to_string(),
            values: vec![Value::String(Some(bound.to_string()))],
        },
    }
}

#[test]
fn test_bound_subquery_values_participate_in_the_cache_key() {
    assert_ne!(
        condition_hash(&bound_subquery_condition("alice")),
        condition_hash(&bound_subquery_condition("bob")),
        "two subqueries differing only in a bound value render identical SQL and must not share a cache entry"
    );
    assert_eq!(
        condition_hash(&bound_subquery_condition("alice")),
        condition_hash(&bound_subquery_condition("alice"))
    );
}

#[test]
fn test_select_subquery_bound_values_participate_in_the_cache_key() {
    let tenant_one = CacheKeyTestPost::query().select_subquery(
        CacheKeyTestUser::query()
            .select(vec!["id"])
            .where_eq("id", 1),
        "owner",
    );
    let tenant_two = CacheKeyTestPost::query().select_subquery(
        CacheKeyTestUser::query()
            .select(vec!["id"])
            .where_eq("id", 2),
        "owner",
    );

    assert_eq!(
        tenant_one.clauses.subquery_select_expressions[0].query_sql,
        tenant_two.clauses.subquery_select_expressions[0].query_sql,
        "a parameterized scalar subquery renders the same SQL for either bound value"
    );
    assert_ne!(
        tenant_one.generate_cache_key(),
        tenant_two.generate_cache_key(),
        "two scalar subqueries differing only in a bound value must not share a cache entry"
    );
}

#[test]
fn a_negative_list_of_nulls_does_not_count_as_an_explicit_filter() {
    // `NOT IN (NULL)` renders `IS NOT NULL`, which keeps every row of a
    // `NOT NULL` key, so a list that came back as nothing but NULLs must not
    // turn a delete into a whole-table one.
    let err = CacheKeyTestUser::query()
        .where_not_in("id", vec![None::<i64>, None])
        .ensure_mutation_has_explicit_filters("delete")
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("unfiltered bulk mutations are blocked"),
        "{err}"
    );

    assert!(
        CacheKeyTestUser::query()
            .where_not_in("id", vec![None, Some(1i64)])
            .ensure_mutation_has_explicit_filters("delete")
            .is_ok()
    );
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
    assert!(err.to_string().contains("no soft delete"), "{err}");
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
