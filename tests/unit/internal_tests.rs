use super::{
    build_count_select, build_exists_any_statement, count_to_u64, json_to_db_value, push_param,
    scoped_find, supports_batch_insert_returning,
};
use crate::config::DatabaseType;
use crate::internal::{
    Backend, ColumnTrait, Condition, InternalModel, OrmBackend, QueryTrait, Value,
};

#[tideorm::model(table = "internal_count_users")]
struct InternalCountUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "internal_soft_delete_users", soft_delete)]
struct InternalSoftDeleteUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[test]
fn batch_insert_returning_supports_postgres_backend_without_config() {
    assert!(supports_batch_insert_returning(None, Backend::Postgres));
}

#[test]
fn batch_insert_returning_rejects_mysql_backend_without_mariadb_config() {
    assert!(!supports_batch_insert_returning(None, Backend::MySql));
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::MySQL),
        Backend::MySql,
    ));
}

#[test]
fn batch_insert_returning_accepts_mariadb_config_on_mysql_backend() {
    assert!(supports_batch_insert_returning(
        Some(DatabaseType::MariaDB),
        Backend::MySql,
    ));
}

#[test]
fn batch_insert_returning_rejects_sqlite_even_though_it_supports_returning() {
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::SQLite),
        Backend::Sqlite,
    ));
}

#[test]
fn batch_insert_returning_rejects_config_backend_mismatches() {
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::Postgres),
        Backend::MySql,
    ));
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::MariaDB),
        Backend::Postgres,
    ));
}

#[test]
fn json_to_db_value_preserves_large_unsigned_numbers() {
    let unsigned = i64::MAX as u64 + 1;
    let value = serde_json::json!(unsigned);

    match json_to_db_value(&value) {
        Value::BigUnsigned(Some(actual)) => assert_eq!(actual, unsigned),
        other => panic!("expected BigUnsigned, got {other:?}"),
    }
}

#[test]
fn push_param_uses_backend_specific_placeholders() {
    let mut postgres_params = Vec::new();
    assert_eq!(
        push_param(
            DatabaseType::Postgres,
            &mut postgres_params,
            Value::Bool(Some(true)),
        ),
        "$1"
    );
    assert_eq!(
        push_param(
            DatabaseType::Postgres,
            &mut postgres_params,
            Value::Bool(Some(false)),
        ),
        "$2"
    );
    assert_eq!(postgres_params.len(), 2);

    let mut mysql_params = Vec::new();
    assert_eq!(
        push_param(
            DatabaseType::MySQL,
            &mut mysql_params,
            Value::Bool(Some(true)),
        ),
        "?"
    );
    assert_eq!(mysql_params.len(), 1);
}

#[test]
fn count_select_omits_where_without_condition() {
    let statement = build_count_select::<InternalCountUser>(None).build(OrmBackend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT COUNT(*) AS \"count\" FROM \"internal_count_users\""
    );
}

#[test]
fn exists_any_statement_uses_select_exists_probe() {
    let statement = build_exists_any_statement::<InternalCountUser>(Backend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT EXISTS(SELECT 1 FROM \"internal_count_users\")"
    );
    assert!(statement.values.is_none());
}

#[test]
fn scoped_find_has_no_soft_delete_filter_for_regular_models() {
    let statement = scoped_find::<InternalCountUser>().build(OrmBackend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT \"internal_count_users\".\"id\", \"internal_count_users\".\"name\" FROM \"internal_count_users\""
    );
}

#[test]
fn scoped_find_applies_soft_delete_filter_for_soft_delete_models() {
    let statement = scoped_find::<InternalSoftDeleteUser>().build(OrmBackend::Postgres);

    assert!(
        statement
            .sql
            .contains("FROM \"internal_soft_delete_users\"")
    );
    assert!(
        statement
            .sql
            .contains("WHERE \"internal_soft_delete_users\".\"deleted_at\" IS NULL")
    );
}

#[test]
fn exists_any_statement_preserves_soft_delete_scope() {
    let statement = build_exists_any_statement::<InternalSoftDeleteUser>(Backend::Postgres);

    assert!(
        statement
            .sql
            .contains("SELECT EXISTS(SELECT 1 FROM \"internal_soft_delete_users\"")
    );
    assert!(
        statement
            .sql
            .contains("WHERE \"internal_soft_delete_users\".\"deleted_at\" IS NULL")
    );
    assert!(statement.values.is_none());
}

#[test]
fn count_select_applies_optional_condition() {
    let condition = Condition::all().add(
        InternalCountUser::column_from_str("name")
            .expect("generated model should expose the name column")
            .eq("alice"),
    );
    let statement =
        build_count_select::<InternalCountUser>(Some(condition)).build(OrmBackend::Postgres);

    assert!(
        statement
            .sql
            .contains("WHERE \"internal_count_users\".\"name\" = $1")
    );
    assert!(statement.values.is_some());
}

#[test]
fn count_to_u64_rejects_negative_counts() {
    let err = count_to_u64(-1, "count(*)").unwrap_err();

    assert!(err.to_string().contains("negative count"));
    assert!(err.to_string().contains("count(*)"));
}
