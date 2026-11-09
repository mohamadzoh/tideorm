use super::error_translation::driver_failure;
use super::{
    ConnAcquireErr, Error, InternalModel, bindable_value, build_count_select, json_to_db_value,
    mask_url_credentials, push_param, reclassify_error, scoped_find, translate_error,
};
use crate::config::DatabaseType;
use crate::error::DbFailureKind;
use crate::internal::{OrmError, QuerySelect, QueryTrait, SelectExt, Value};
use std::error::Error as StdError;

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
fn json_to_db_value_preserves_large_unsigned_numbers() {
    let unsigned = i64::MAX as u64 + 1;
    let value = serde_json::json!(unsigned);

    // Exact, and not `BigUnsigned`, which the SQLite and PostgreSQL drivers
    // cannot bind as their signed integer.
    match json_to_db_value(&value) {
        Value::Decimal(Some(actual)) => assert_eq!(actual, rust_decimal::Decimal::from(unsigned)),
        other => panic!("expected an exact decimal, got {other:?}"),
    }
}

#[test]
fn bindable_value_only_rewrites_unsigned_values_past_i64_max() {
    assert_eq!(
        bindable_value(u64::MAX),
        Value::Decimal(Some(rust_decimal::Decimal::from(u64::MAX)))
    );
    assert_eq!(bindable_value(7u64), Value::BigUnsigned(Some(7)));
    assert_eq!(bindable_value(-7i64), Value::BigInt(Some(-7)));
    assert_eq!(bindable_value(None::<u64>), Value::BigUnsigned(None));
    assert_eq!(
        bindable_value("key"),
        Value::String(Some("key".to_string()))
    );
}

#[test]
fn a_page_writes_its_limit_and_binds_its_offset_after_other_values() {
    use crate::internal::{ColumnTrait, QueryFilter};

    let name = InternalCountUser::column_from_str("name").unwrap();
    let statement = scoped_find::<InternalCountUser>()
        .filter(name.eq("ada"))
        .limit(20)
        .offset(40)
        .build(DatabaseType::Postgres)
        .unwrap();

    assert!(
        statement.sql.ends_with(" LIMIT 20 OFFSET $2"),
        "{}",
        statement.sql
    );
    assert_eq!(
        statement.values.unwrap().0,
        vec![
            Value::String(Some("ada".to_string())),
            Value::BigUnsigned(Some(40))
        ]
    );
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
    let statement = build_count_select::<InternalCountUser>()
        .build(DatabaseType::Postgres)
        .unwrap();

    assert_eq!(
        statement.sql,
        "SELECT COUNT(*) AS \"count\" FROM \"internal_count_users\""
    );
}

#[test]
fn exists_any_probe_reads_at_most_one_row_and_keeps_the_soft_delete_scope() {
    fn probe<E: crate::internal::EntityTrait>(select: crate::internal::Select<E>) -> String {
        DatabaseType::Postgres
            .build(&select.exists_query())
            .unwrap()
            .sql
    }
    assert_eq!(
        probe(scoped_find::<InternalCountUser>()),
        "SELECT 1 FROM \"internal_count_users\" LIMIT 1"
    );
    assert_eq!(
        probe(scoped_find::<InternalSoftDeleteUser>()),
        "SELECT 1 FROM \"internal_soft_delete_users\" \
         WHERE \"internal_soft_delete_users\".\"deleted_at\" IS NULL LIMIT 1"
    );
}

#[test]
fn a_one_row_read_writes_its_limit_into_the_sql() {
    // A bound `LIMIT ?` makes recent SQLite recompile the statement on every run.
    let statement = scoped_find::<InternalSoftDeleteUser>()
        .limit(1)
        .build(DatabaseType::SQLite)
        .unwrap();

    assert!(
        statement.sql.ends_with(" IS NULL LIMIT 1"),
        "{}",
        statement.sql
    );
    assert!(statement.values.is_none_or(|values| values.0.is_empty()));
}

#[test]
fn scoped_find_leaves_trashed_rows_out_only_for_soft_delete_models() {
    assert_eq!(
        scoped_find::<InternalCountUser>()
            .build(DatabaseType::Postgres)
            .unwrap()
            .sql,
        "SELECT \"internal_count_users\".\"id\", \"internal_count_users\".\"name\" FROM \"internal_count_users\""
    );
    assert_eq!(
        scoped_find::<InternalSoftDeleteUser>()
            .build(DatabaseType::Postgres)
            .unwrap()
            .sql,
        "SELECT \"internal_soft_delete_users\".\"id\", \"internal_soft_delete_users\".\"name\", \
         \"internal_soft_delete_users\".\"deleted_at\" FROM \"internal_soft_delete_users\" \
         WHERE \"internal_soft_delete_users\".\"deleted_at\" IS NULL"
    );
}

#[test]
fn translate_error_maps_each_engine_error_to_its_variant() {
    type IsExpected = fn(&Error) -> bool;
    let cases: Vec<(OrmError, IsExpected)> = vec![
        (
            OrmError::AttrNotSet("email".to_string()),
            |error| matches!(error, Error::Validation { field, .. } if field == "email"),
        ),
        // Not collapsed into an internal error.
        (
            OrmError::KeyArityMismatch {
                expected: 2,
                received: 1,
            },
            |error| matches!(error, Error::Query { .. }),
        ),
        // A statement refused on an aborted transaction.
        (
            OrmError::TransactionAborted {
                reason: "it deadlocked".to_string(),
                cause: None,
            },
            |error| matches!(error, Error::Transaction { message, .. } if message == "it deadlocked"),
        ),
        // Engine errors without a TideORM counterpart stay internal.
        (OrmError::Custom("boom".to_string()), |error| {
            matches!(error, Error::Internal { .. })
        }),
    ];
    for (engine_error, expected) in cases {
        let described = format!("{engine_error:?}");
        let translated = translate_error(engine_error);
        assert!(expected(&translated), "{described} became {translated:?}");
    }
}

#[test]
fn pool_acquire_failures_keep_a_retryable_classification() {
    let translated = translate_error(OrmError::ConnectionAcquire(ConnAcquireErr::Timeout));

    assert!(matches!(translated, Error::Connection { .. }));
    assert_eq!(translated.failure_kind(), DbFailureKind::ConnectionTimeout);
    assert!(translated.is_retryable());
}

#[test]
fn a_connection_that_breaks_under_a_statement_is_retryable() {
    // MySQL's KILL and a server restart close the socket without an error
    // packet, so the driver reports I/O, not a database error.
    let broken = std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "aborted");
    let translated = translate_error(OrmError::Query(crate::orm::RuntimeErr::DriverError(
        std::sync::Arc::new(crate::orm::driver::Error::Io(broken)),
    )));

    assert_eq!(translated.failure_kind(), DbFailureKind::ConnectionClosed);
    assert!(translated.is_retryable());
}

/// A driver failure with no database error behind it is classified by what
/// went wrong: a refused connection, TLS, or the connection settings.
#[test]
fn connection_failures_without_a_database_error_keep_their_class() {
    use crate::orm::driver::Error as DriverError;

    let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
    for (error, kind) in [
        (DriverError::Io(refused), DbFailureKind::ConnectionRefused),
        (
            DriverError::Tls("bad certificate".into()),
            DbFailureKind::TlsFailed,
        ),
        (
            DriverError::Configuration("bad port".into()),
            DbFailureKind::InvalidConfiguration,
        ),
    ] {
        let translated = translate_error(OrmError::Conn(crate::orm::RuntimeErr::DriverError(
            std::sync::Arc::new(error),
        )));
        assert!(
            matches!(translated, Error::Connection { .. }),
            "{translated:?}"
        );
        assert_eq!(translated.failure_kind(), kind);
    }
}

/// A statement refused on an aborted transaction fails as the one that
/// aborted it did, so it is retryable exactly when that one was.
#[test]
fn a_refused_statement_carries_the_failure_that_aborted_its_transaction() {
    let refused = translate_error(OrmError::TransactionAborted {
        reason: "it failed".to_string(),
        cause: Some(std::sync::Arc::new(OrmError::ConnectionAcquire(
            ConnAcquireErr::ConnectionClosed,
        ))),
    });
    assert!(matches!(refused, Error::Transaction { .. }), "{refused:?}");
    assert_eq!(refused.failure_kind(), DbFailureKind::ConnectionClosed);
    assert!(refused.is_retryable());
}

#[test]
fn the_engine_error_survives_as_the_source_of_the_translated_error() {
    let translated = translate_error(OrmError::ConnectionAcquire(
        ConnAcquireErr::ConnectionClosed,
    ));

    // Error -> DbFailure -> engine error. The last hop is what a `{:#}`
    // chain or an `anyhow` report needs to reach the driver.
    let failure = StdError::source(&translated).expect("driver failure is the source");
    assert!(
        failure.source().is_some(),
        "the engine error must stay reachable through the failure"
    );
}

#[test]
fn engine_bookkeeping_errors_carry_no_driver_failure() {
    assert!(driver_failure(&OrmError::UnpackInsertId).is_none());
    assert!(driver_failure(&OrmError::RecordNotInserted).is_none());
}

/// The engine quotes a URL it cannot parse, with its credentials masked, and
/// a connection error is exactly what gets logged.
#[tokio::test]
async fn connect_errors_mask_the_url_credentials() {
    let password = format!("pw{}", std::process::id());
    let url = format!("postgres://app:{password}@db.internal:99999/app");
    let error = crate::Database::builder()
        .url(url.as_str())
        .build()
        .await
        .expect_err("port 99999 does not parse");
    let shown = format!("{error} {error:?}");

    assert!(matches!(error, Error::Connection { .. }), "{shown}");
    assert!(!shown.contains(&password), "{shown}");
    assert!(
        shown.contains("postgres://***@db.internal:99999/app"),
        "{shown}"
    );
}

/// A reclassified error keeps the message TideORM translated, not the
/// engine's own text.
#[test]
fn reclassified_errors_keep_the_translated_message() {
    let aborted = OrmError::TransactionAborted {
        reason: "it deadlocked".to_string(),
        cause: None,
    };
    let error = reclassify_error(aborted, |message, source| Error::Transaction {
        message,
        source,
    });
    assert_eq!(error.to_string(), "Transaction error: it deadlocked");
}

#[test]
fn masking_covers_the_whole_userinfo_and_nothing_else() {
    let password = format!("p@ss{}", std::process::id());
    assert_eq!(
        mask_url_credentials(&format!("mysql://app:{password}@db:3306/app")),
        "mysql://***@db:3306/app"
    );
    assert_eq!(
        mask_url_credentials("postgres://db/app"),
        "postgres://db/app"
    );
    assert_eq!(mask_url_credentials("sqlite::memory:"), "sqlite::memory:");
    // A password holding an unencoded `/` ends at the last `@` all the same.
    assert_eq!(
        mask_url_credentials("postgres://user:pa/ss@localhost/db"),
        "postgres://***@localhost/db"
    );

    // The PostgreSQL driver also reads a password from the query string, and
    // an `@` there is not the end of the userinfo.
    assert_eq!(
        mask_url_credentials("postgres://db/app?user=app&password=s3cret&sslmode=require"),
        "postgres://db/app?user=app&password=***&sslmode=require"
    );
    assert_eq!(
        mask_url_credentials("mysql://app:pw@db/app?ssl-mode=REQUIRED&tag=a@b"),
        "mysql://***@db/app?ssl-mode=REQUIRED&tag=a@b"
    );
    assert_eq!(
        mask_url_credentials("postgres://db/app?tag=a@b"),
        "postgres://db/app?tag=a@b"
    );

    // An unencoded `?` or `#` in a password ends the authority early; the
    // password is masked all the same, never printed as a query.
    for password in ["p?ss", "p#ss", "p?a@ss", "p?ss&password=x"] {
        let shown = mask_url_credentials(&format!("mysql://app:{password}@db:3306/app?a=1"));
        assert_eq!(shown, "mysql://***@db:3306/app?a=1", "{password}");
    }
}
