use crate::model::Model;

#[tideorm::model(table = "linked_select_users")]
struct LinkedSelectUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    profile_id: i64,
}

const UNSAFE_TABLE: &str = "profiles\" ON 1 = 1; DROP TABLE profiles; --";

#[test]
fn test_select_with_linked_validates_its_join_table() {
    let query = LinkedSelectUser::query().select_with_linked(
        vec!["id"],
        UNSAFE_TABLE,
        "profile_id",
        "id",
        vec!["bio"],
    );

    let err = query
        .ensure_query_is_valid()
        .expect_err("an unsafe linked table must invalidate the query");
    assert!(err.to_string().contains("unsafe JOIN table"), "{err}");
    assert!(
        !query.known_qualifiers().iter().any(|q| q.contains("DROP")),
        "a rejected join must not whitelist its qualifier"
    );
}

#[test]
fn test_select_also_linked_validates_its_join_table() {
    let query =
        LinkedSelectUser::query().select_also_linked(UNSAFE_TABLE, "id", "user_id", vec!["bio"]);

    let err = query
        .ensure_query_is_valid()
        .expect_err("an unsafe linked table must invalidate the query");
    assert!(err.to_string().contains("unsafe JOIN table"), "{err}");
    assert!(
        !query.known_qualifiers().iter().any(|q| q.contains("DROP")),
        "a rejected join must not whitelist its qualifier"
    );
}

#[test]
fn test_distinct_is_idempotent() {
    let sql = LinkedSelectUser::query()
        .distinct()
        .distinct()
        .build_select_sql_for_db(crate::config::DatabaseType::Postgres);

    assert_eq!(
        sql,
        "SELECT DISTINCT \"linked_select_users\".\"id\", \"linked_select_users\".\"profile_id\" FROM \"linked_select_users\""
    );
}

#[test]
fn test_distinct_rejects_order_by_outside_the_projection() {
    let err = LinkedSelectUser::query()
        .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
        .distinct()
        .order_desc("profiles.created_at")
        .ensure_query_is_valid()
        .expect_err("a SELECT DISTINCT cannot order by a column it does not select");

    assert!(
        err.to_string().contains("not part of the distinct()"),
        "{err}"
    );
}

#[test]
fn test_distinct_allows_order_by_a_projected_column() {
    let query = LinkedSelectUser::query()
        .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
        .distinct()
        .order_desc("linked_select_users.id");

    assert!(query.ensure_query_is_valid().is_ok());
}

#[test]
fn test_distinct_narrows_the_allowed_order_by_to_an_explicit_select() {
    let err = LinkedSelectUser::query()
        .select(vec!["id"])
        .distinct()
        .order_desc("profile_id")
        .ensure_query_is_valid()
        .expect_err("an explicit select() narrows what a SELECT DISTINCT can order by");

    assert!(
        err.to_string().contains("not part of the distinct()"),
        "{err}"
    );
}

#[test]
fn test_distinct_leaves_raw_order_by_and_raw_projections_to_the_caller() {
    let raw_order = LinkedSelectUser::query()
        .distinct()
        .order_by_raw("profile_id", crate::query::Order::Desc);
    assert!(raw_order.ensure_query_is_valid().is_ok());

    let raw_projection = LinkedSelectUser::query()
        .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
        .select_raw("profiles.bio")
        .distinct()
        .order_desc("profiles.bio");
    assert!(raw_projection.ensure_query_is_valid().is_ok());
}

#[test]
fn test_select_with_linked_still_registers_a_valid_join() {
    let query = LinkedSelectUser::query().select_with_linked(
        vec!["id"],
        "profiles",
        "profile_id",
        "id",
        vec!["bio"],
    );

    assert!(query.ensure_query_is_valid().is_ok());
    assert!(query.known_qualifiers().contains("profiles"));
}
