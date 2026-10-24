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
                .0,
            two.build_select_sql_with_params_for_db(DatabaseType::Postgres)
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

/// The soft-delete stamp is a UTC literal rather than the database clock, so
/// it has to be spelled the way the backend the statement is heading for
/// spells it. Rendering it for the ambient backend instead put an RFC3339
/// literal — `T` separator, UTC offset — into statements bound for MySQL,
/// which rejects both.
#[test]
fn test_soft_delete_stamp_renders_for_the_statement_backend() {
    let timestamp = chrono::DateTime::parse_from_rfc3339("2026-07-25T12:34:56.123456Z")
        .expect("fixture is valid rfc3339")
        .with_timezone(&chrono::Utc);
    for (db_type, expected) in [
        (DatabaseType::Postgres, "'2026-07-25T12:34:56.123456+00:00'"),
        (DatabaseType::SQLite, "'2026-07-25T12:34:56.123456+00:00'"),
        (DatabaseType::MySQL, "'2026-07-25 12:34:56.123456'"),
        (DatabaseType::MariaDB, "'2026-07-25 12:34:56.123456'"),
    ] {
        assert_eq!(
            QueryBuilder::<CacheKeyTestSoftDeleteUser>::utc_timestamp_literal(db_type, timestamp),
            expected,
            "{db_type:?}"
        );
    }

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
