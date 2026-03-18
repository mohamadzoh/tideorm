use super::supports_batch_insert_returning;
use crate::config::DatabaseType;
use crate::internal::DbBackend;

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