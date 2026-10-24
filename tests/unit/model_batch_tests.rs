use super::BatchUpdateBuilder;
use crate::model::Model as ModelTrait;

#[tideorm::model(table = "batch_update_guard_users")]
struct BatchUpdateGuardUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

/// A batch update needs a filter that can leave a row out, as the
/// `QueryBuilder` mutation terminals do. An empty negative list renders
/// constant-true (`id NOT IN ()` holds for every row), so counting conditions
/// would let a caller whose filter list came back empty rewrite the whole
/// table; an OR group with such a member used to be accepted that way.
#[test]
fn batch_update_guard_requires_a_filter_that_can_exclude_a_row() {
    let update = || BatchUpdateGuardUser::update_all().set("name", "updated");
    let no_ids = Vec::<i64>::new;
    let cases = [
        (
            "no filter",
            update(),
            Some("requires at least one explicit filter"),
        ),
        (
            "a limit alone",
            update().limit(1),
            Some("requires at least one explicit filter"),
        ),
        (
            "an empty negative list",
            update().where_not_in("id", no_ids()),
            Some("unfiltered bulk mutations are blocked"),
        ),
        (
            "an OR group with an empty negative list",
            update()
                .or_where_eq("name", "alice")
                .or_where_not_in("id", no_ids()),
            Some("unfiltered bulk mutations are blocked"),
        ),
        ("a where filter", update().where_eq("id", 1), None),
        ("an OR filter", update().or_where_eq("name", "alice"), None),
        (
            "a real filter beside an empty negative list",
            update()
                .where_eq("name", "alice")
                .where_not_in("id", no_ids()),
            None,
        ),
    ];

    for (case, update, refusal) in cases {
        match (update.ensure_explicit_filters("update"), refusal) {
            (Ok(()), None) => {}
            (Err(error), Some(refusal)) => {
                assert!(error.to_string().contains(refusal), "{case}: {error}")
            }
            (outcome, _) => panic!("{case}: unexpected {outcome:?}"),
        }
    }
}

#[test]
fn batch_update_when_sets_only_when_the_condition_holds() {
    let builder = BatchUpdateBuilder::<BatchUpdateGuardUser>::new()
        .when(true, |update| update.set("name", "updated"));
    assert!(matches!(
        builder.updates.get("name"),
        Some(super::UpdateValue::Value(value)) if *value == serde_json::json!("updated")
    ));

    let builder = BatchUpdateBuilder::<BatchUpdateGuardUser>::new()
        .when(false, |update| update.set("name", "updated"));
    assert!(!builder.updates.contains_key("name"));
}

#[test]
fn batch_update_builder_accepts_typed_columns() {
    let builder = BatchUpdateGuardUser::update_all()
        .set(BatchUpdateGuardUser::columns.name, "updated")
        .increment(BatchUpdateGuardUser::columns.id, 1)
        .where_eq(BatchUpdateGuardUser::columns.id, 1)
        .or_where_eq(BatchUpdateGuardUser::columns.name, "alice");

    assert!(builder.updates.contains_key("name"));
    assert!(builder.updates.contains_key("id"));
    assert_eq!(builder.conditions.len(), 1);
    assert_eq!(builder.conditions[0].column, "id");
    assert_eq!(builder.or_group.conditions.len(), 1);
    assert_eq!(builder.or_group.conditions[0].column, "name");
}

#[test]
fn batch_update_builder_literal_like_helpers_escape_metacharacters() {
    let builder = BatchUpdateGuardUser::update_all()
        .where_contains("name", r"100%_\done")
        .or_where_starts_with("name", r"lead%_")
        .or_where_ends_with("name", r"tail%_");

    assert_eq!(builder.conditions.len(), 1);
    assert!(matches!(
        builder.conditions[0].operator,
        crate::query::Operator::LikeEscaped
    ));
    assert!(matches!(
        &builder.conditions[0].value,
        crate::query::ConditionValue::Single(serde_json::Value::String(value))
            if value == r"%100!%!_\done%"
    ));
    assert_eq!(builder.or_group.conditions.len(), 2);
    assert!(matches!(
        &builder.or_group.conditions[0].value,
        crate::query::ConditionValue::Single(serde_json::Value::String(value))
            if value == r"lead!%!_%"
    ));
    assert!(matches!(
        &builder.or_group.conditions[1].value,
        crate::query::ConditionValue::Single(serde_json::Value::String(value))
            if value == r"%tail!%!_"
    ));
}

#[test]
fn batch_execute_returning_uses_backend_returning_capability() {
    // MariaDB has `UPDATE .. RETURNING` only from 13.0, a version TideORM does
    // not check for.
    for (db_type, name) in [
        (crate::config::DatabaseType::MySQL, "MySQL"),
        (crate::config::DatabaseType::MariaDB, "MariaDB"),
    ] {
        let err =
            BatchUpdateBuilder::<BatchUpdateGuardUser>::ensure_backend_supports_returning(db_type)
                .unwrap_err();

        assert!(
            err.to_string()
                .contains(&format!("execute_returning() is not supported on {name}")),
            "{err}"
        );
        assert!(
            matches!(&err, crate::Error::BackendNotSupported { backend, .. } if backend == name),
            "a backend capability refusal must not be reported as a query error: {err:?}"
        );
    }
    assert!(
        BatchUpdateBuilder::<BatchUpdateGuardUser>::ensure_backend_supports_returning(
            crate::config::DatabaseType::SQLite,
        )
        .is_ok()
    );
}

/// `execute` and `execute_returning` each empty the query cache.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[tokio::test]
async fn batch_updates_invalidate_cached_queries() {
    let _guard = crate::test_support::global_db_lock().lock().await;
    crate::test_support::install_sqlite_global(&[
        "CREATE TABLE batch_update_guard_users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
    ])
    .await;
    crate::test_support::set_query_cache(true);
    let cached_name = || async {
        let rows = crate::test_support::cached_rows::<BatchUpdateGuardUser>().await;
        assert_eq!(crate::test_support::cached_entries(), 1);
        rows[0].name.clone()
    };

    let saved = BatchUpdateGuardUser {
        id: 0,
        name: "Alice".to_string(),
    }
    .save()
    .await
    .expect("seed save should succeed");

    assert_eq!(cached_name().await, "Alice");
    let rows_affected = BatchUpdateGuardUser::update_all()
        .set("name", "Bob")
        .where_eq("id", saved.id)
        .execute()
        .await
        .expect("batch execute should succeed");
    assert_eq!(rows_affected, 1);
    assert_eq!(
        crate::test_support::cached_entries(),
        0,
        "execute should empty the cache"
    );

    assert_eq!(cached_name().await, "Bob");
    let returned = BatchUpdateGuardUser::update_all()
        .set("name", "Carol")
        .where_eq("id", saved.id)
        .execute_returning()
        .await
        .expect("batch execute_returning should succeed");
    assert_eq!(returned.len(), 1);
    assert_eq!(returned[0].name, "Carol");
    assert_eq!(
        crate::test_support::cached_entries(),
        0,
        "execute_returning should empty the cache"
    );
    assert_eq!(cached_name().await, "Carol");

    crate::test_support::reset_globals_and_cache();
}
