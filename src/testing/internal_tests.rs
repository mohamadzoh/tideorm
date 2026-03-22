use super::{
    build_count_select, build_exists_statement, count_to_u64, supports_batch_insert_returning,
};
use crate::config::DatabaseType;
use crate::internal::{ColumnTrait, Condition, DbBackend, InternalModel, QueryTrait};

#[tideorm::model(table = "internal_count_users")]
struct InternalCountUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[test]
fn batch_insert_returning_supports_postgres_backend_without_config() {
    assert!(supports_batch_insert_returning(None, DbBackend::Postgres));
}

#[test]
fn batch_insert_returning_rejects_mysql_backend_without_mariadb_config() {
    assert!(!supports_batch_insert_returning(None, DbBackend::MySql));
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::MySQL),
        DbBackend::MySql,
    ));
}

#[test]
fn batch_insert_returning_accepts_mariadb_config_on_mysql_backend() {
    assert!(supports_batch_insert_returning(
        Some(DatabaseType::MariaDB),
        DbBackend::MySql,
    ));
}

#[test]
fn batch_insert_returning_rejects_sqlite_even_though_it_supports_returning() {
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::SQLite),
        DbBackend::Sqlite,
    ));
}

#[test]
fn batch_insert_returning_rejects_config_backend_mismatches() {
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::Postgres),
        DbBackend::MySql,
    ));
    assert!(!supports_batch_insert_returning(
        Some(DatabaseType::MariaDB),
        DbBackend::Postgres,
    ));
}

#[test]
fn count_select_omits_where_without_condition() {
    let statement = build_count_select::<InternalCountUser>(None).build(DbBackend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT COUNT(*) AS \"count\" FROM \"internal_count_users\""
    );
}

#[test]
fn exists_select_uses_exists_subquery_with_limit() {
    let statement = build_exists_statement::<InternalCountUser>(DbBackend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT EXISTS(SELECT $1 FROM \"internal_count_users\" LIMIT $2) AS \"exists\""
    );
    assert!(statement.values.is_some());
}

#[test]
fn count_select_applies_optional_condition() {
    let condition = Condition::all().add(
        InternalCountUser::column_from_str("name")
            .expect("generated model should expose the name column")
            .eq("alice"),
    );
    let statement =
        build_count_select::<InternalCountUser>(Some(condition)).build(DbBackend::Postgres);

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
