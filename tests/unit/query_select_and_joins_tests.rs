use crate::model::Model;

#[tideorm::model(table = "linked_select_users")]
struct LinkedSelectUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    profile_id: i64,
}

const UNSAFE_TABLE: &str = "profiles\" ON 1 = 1; DROP TABLE profiles; --";

#[test]
fn test_join_helpers_validate_their_identifiers() {
    for query in [
        LinkedSelectUser::query().select_with_linked(
            vec!["id"],
            UNSAFE_TABLE,
            "profile_id",
            "id",
            vec!["bio"],
        ),
        LinkedSelectUser::query().select_also_linked(UNSAFE_TABLE, "id", "user_id", vec!["bio"]),
        LinkedSelectUser::query().right_join(UNSAFE_TABLE, "linked_select_users.id", "profiles.id"),
        LinkedSelectUser::query().left_join_as(UNSAFE_TABLE, "p", "linked_select_users.id", "p.id"),
    ] {
        let err = query
            .ensure_query_is_valid()
            .expect_err("an unsafe join table must invalidate the query");
        assert!(err.to_string().contains("unsafe JOIN table"), "{err}");
        assert!(
            !query.known_qualifiers().iter().any(|q| q.contains("DROP")),
            "a rejected join must not whitelist its qualifier"
        );
    }

    let unsafe_alias = LinkedSelectUser::query().right_join_as(
        "profiles",
        "p; --",
        "linked_select_users.id",
        "p.id",
    );
    let err = unsafe_alias
        .ensure_query_is_valid()
        .expect_err("an unsafe join alias must invalidate the query");
    assert!(err.to_string().contains("unsafe JOIN alias"), "{err}");

    let linked = LinkedSelectUser::query().select_with_linked(
        vec!["id"],
        "profiles",
        "profile_id",
        "id",
        vec!["bio"],
    );
    assert!(linked.ensure_query_is_valid().is_ok());
    assert!(linked.known_qualifiers().contains("profiles"));
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

/// A SELECT DISTINCT can only order by a column it selects.
#[test]
fn test_distinct_rejects_order_by_outside_the_projection() {
    let joined = || {
        LinkedSelectUser::query().inner_join(
            "profiles",
            "linked_select_users.profile_id",
            "profiles.id",
        )
    };

    for query in [
        joined().distinct().order_desc("profiles.created_at"),
        // An explicit select() narrows what may be ordered by.
        LinkedSelectUser::query()
            .select(vec!["id"])
            .distinct()
            .order_desc("profile_id"),
    ] {
        let err = query
            .ensure_query_is_valid()
            .expect_err("the ordering column is not selected");
        assert!(
            err.to_string().contains("not part of the distinct()"),
            "{err}"
        );
    }

    for query in [
        joined().distinct().order_desc("linked_select_users.id"),
        // A raw ordering or a raw projection is the caller's to vouch for.
        LinkedSelectUser::query()
            .distinct()
            .order_by_raw("profile_id", crate::query::Order::Desc),
        joined()
            .select_raw("profiles.bio")
            .distinct()
            .order_desc("profiles.bio"),
    ] {
        query
            .ensure_query_is_valid()
            .expect("the ordering column is selected, or raw");
    }
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

#[tideorm::model(table = "window_owners")]
struct WindowOwner {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "window_pets")]
struct WindowPet {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    owner_id: i64,
    name: String,
}

#[test]
fn a_typed_column_of_another_model_in_a_window_keeps_its_table() {
    use crate::config::DatabaseType;
    use crate::query::{Order, WindowFunction, WindowFunctionType};

    let sql = WindowPet::query()
        .inner_join("window_owners", "window_pets.owner_id", "window_owners.id")
        .window(
            WindowFunction::new(WindowFunctionType::RowNumber, "rn")
                .partition_by(WindowOwner::columns.name)
                .order_by(WindowOwner::columns.id, Order::Asc),
        )
        .build_sql_preview_for_db(DatabaseType::Postgres);

    assert!(
        sql.contains(r#"PARTITION BY "window_owners"."name" ORDER BY "window_owners"."id" ASC"#),
        "{sql}"
    );
}

#[tideorm::model(table = "linked_audit_events", schema = "audit")]
struct LinkedAuditEvent {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    token: uuid::Uuid,
}

/// A filter on a joined table binds as the joined model's column type when
/// the join names that model's schema: `linked_audit_events.token` is a UUID,
/// not text, through `audit.linked_audit_events`, but a same-named table in
/// another schema is not that model.
#[test]
fn a_joined_column_binds_as_its_type_only_in_its_models_schema() {
    for (schema, expect_uuid) in [("audit", true), ("other", false)] {
        let (_, params) = LinkedSelectUser::query()
            .inner_join(
                &format!("{schema}.linked_audit_events"),
                "linked_select_users.id",
                "linked_audit_events.id",
            )
            .where_eq("linked_audit_events.token", uuid::Uuid::nil().to_string())
            .build_select_sql_with_params_for_db(crate::config::DatabaseType::Postgres);

        let expected = if expect_uuid {
            crate::internal::Value::from(uuid::Uuid::nil())
        } else {
            crate::internal::Value::from(uuid::Uuid::nil().to_string())
        };
        assert_eq!(params, vec![expected], "{schema}");
    }
}
