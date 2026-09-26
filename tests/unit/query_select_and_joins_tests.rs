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

#[test]
fn test_right_and_aliased_joins_render_their_clause_on_every_backend() {
    use crate::config::DatabaseType;

    let query = LinkedSelectUser::query()
        .right_join("profiles", "linked_select_users.profile_id", "profiles.id")
        .left_join_as(
            "linked_select_users",
            "referrer",
            "linked_select_users.id",
            "referrer.profile_id",
        )
        .right_join_as("accounts", "a", "profiles.id", "a.profile_id");
    assert!(query.ensure_query_is_valid().is_ok());

    let postgres = query.build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        postgres.ends_with(
            " FROM \"linked_select_users\" \
             RIGHT JOIN \"profiles\" ON \"linked_select_users\".\"profile_id\" = \"profiles\".\"id\" \
             LEFT JOIN \"linked_select_users\" AS \"referrer\" ON \"linked_select_users\".\"id\" = \"referrer\".\"profile_id\" \
             RIGHT JOIN \"accounts\" AS \"a\" ON \"profiles\".\"id\" = \"a\".\"profile_id\""
        ),
        "{postgres}"
    );

    let mysql = query.build_select_sql_for_db(DatabaseType::MySQL);
    assert!(
        mysql.contains(
            "RIGHT JOIN `profiles` ON `linked_select_users`.`profile_id` = `profiles`.`id`"
        ) && mysql.contains("LEFT JOIN `linked_select_users` AS `referrer` ON")
            && mysql.contains("RIGHT JOIN `accounts` AS `a` ON"),
        "{mysql}"
    );
}

#[test]
fn test_right_and_aliased_joins_validate_their_identifiers() {
    let rejected = [
        LinkedSelectUser::query().right_join(UNSAFE_TABLE, "linked_select_users.id", "profiles.id"),
        LinkedSelectUser::query().right_join_as(
            "profiles",
            "p; --",
            "linked_select_users.id",
            "p.id",
        ),
        LinkedSelectUser::query().left_join_as(UNSAFE_TABLE, "p", "linked_select_users.id", "p.id"),
    ];

    for query in rejected {
        assert!(
            query.ensure_query_is_valid().is_err(),
            "an unsafe join must invalidate the query"
        );
    }
}

#[test]
fn test_a_joined_query_projects_and_filters_its_own_columns_qualified() {
    use crate::config::DatabaseType;

    let sql = LinkedSelectUser::query()
        .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
        .select(vec!["id", "profile_id AS pid"])
        .where_eq("id", 1)
        .order_desc("id")
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        sql.starts_with(
            "SELECT \"linked_select_users\".\"id\", \"linked_select_users\".\"profile_id\" AS \"pid\" FROM"
        ),
        "{sql}"
    );
    assert!(
        sql.contains("WHERE \"linked_select_users\".\"id\" = 1")
            && sql.ends_with("ORDER BY \"linked_select_users\".\"id\" DESC"),
        "{sql}"
    );
}

#[test]
fn test_a_projection_cannot_name_one_column_twice() {
    let err = LinkedSelectUser::query()
        .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
        .select(vec!["linked_select_users.id", "profiles.id"])
        .ensure_query_is_valid()
        .expect_err("two columns named id");
    assert!(err.to_string().contains("'id' twice"), "{err}");

    assert!(
        LinkedSelectUser::query()
            .inner_join("profiles", "linked_select_users.profile_id", "profiles.id")
            .select(vec!["linked_select_users.id", "profiles.id AS profile_key"])
            .ensure_query_is_valid()
            .is_ok()
    );
}

#[test]
fn test_get_refuses_a_projection_that_would_fill_the_model_wrongly() {
    let joined = || {
        LinkedSelectUser::query().inner_join(
            "profiles",
            "linked_select_users.profile_id",
            "profiles.id",
        )
    };

    // distinct() does not vouch for the columns select() left out.
    assert!(
        LinkedSelectUser::query()
            .select(vec!["id"])
            .distinct()
            .ensure_projection_covers_model()
            .is_err()
    );
    // A wildcard over a join reads the joined table's same-named columns too.
    assert!(
        joined()
            .select(vec!["*"])
            .ensure_projection_covers_model()
            .is_err()
    );
    assert!(
        joined()
            .select(vec!["linked_select_users.*", "profiles.*"])
            .ensure_projection_covers_model()
            .is_err()
    );
    assert!(
        joined()
            .select(vec!["linked_select_users.*"])
            .ensure_projection_covers_model()
            .is_ok()
    );
    // Another table's `id` is not the model's.
    assert!(
        joined()
            .select(vec!["profiles.id", "profile_id"])
            .ensure_projection_covers_model()
            .is_err()
    );
    assert!(
        LinkedSelectUser::query()
            .select(vec!["*"])
            .ensure_projection_covers_model()
            .is_ok()
    );
}
