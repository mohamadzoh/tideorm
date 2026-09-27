use super::{
    ConnAcquireErr, DbFailureKind, Error, InternalModel, bindable_value, build_count_select,
    build_exists_any_statement, check_fields_storable, count_to_u64, driver_failure,
    json_to_db_value, mask_url_credentials, one_row_statement, page_statement, push_param,
    reject_encryption_blind_conversion, scoped_find, translate_connect_error, translate_error,
};
use crate::config::DatabaseType;
use crate::internal::{Backend, OrmBackend, OrmError, QueryTrait, Value};
use crate::model::ModelMeta;
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

#[tideorm::model(table = "internal_narrow_integers")]
struct InternalNarrowIntegers {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    count: u32,
    total: Option<u64>,
    level: u8,
}

#[test]
fn driver_limited_fields_lists_the_integer_types_a_driver_cannot_read_back() {
    assert_eq!(
        InternalNarrowIntegers::driver_limited_fields(),
        &[("total", "u64"), ("level", "u8")]
    );
    assert!(InternalCountUser::driver_limited_fields().is_empty());
}

#[test]
fn fields_a_driver_cannot_read_back_are_refused_before_the_write() {
    let error = check_fields_storable::<InternalNarrowIntegers>(Backend::Sqlite)
        .expect_err("SQLite cannot read a u64 back");
    assert!(
        matches!(&error, Error::Conversion { message } if message.contains("total") && message.contains("`i64`")),
        "{error:?}"
    );

    let error = check_fields_storable::<InternalNarrowIntegers>(Backend::Postgres)
        .expect_err("PostgreSQL cannot read a u64 or a u8 back");
    assert!(error.to_string().contains("total"), "{error}");

    assert!(check_fields_storable::<InternalNarrowIntegers>(Backend::MySql).is_ok());
    assert!(check_fields_storable::<InternalCountUser>(Backend::Postgres).is_ok());
}

#[test]
fn json_to_db_value_preserves_large_unsigned_numbers() {
    let unsigned = i64::MAX as u64 + 1;
    let value = serde_json::json!(unsigned);

    // Exact, and not `BigUnsigned`, which the SQLite and PostgreSQL drivers
    // panic converting to their signed integer.
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
fn page_statement_writes_the_limit_and_binds_the_offset_after_other_values() {
    use crate::internal::{ColumnTrait, QueryFilter};

    let name = InternalCountUser::column_from_str("name").unwrap();
    let select = scoped_find::<InternalCountUser>().filter(name.eq("ada"));
    let statement = page_statement(select, OrmBackend::Postgres, 20, 40);

    assert!(
        statement.sql.ends_with(" LIMIT 20 OFFSET $2"),
        "{}",
        statement.sql
    );
    assert_eq!(
        statement.values.unwrap().0,
        vec![
            Value::String(Some("ada".to_string())),
            Value::BigInt(Some(40))
        ]
    );

    let statement = page_statement(
        scoped_find::<InternalCountUser>(),
        OrmBackend::Sqlite,
        20,
        0,
    );
    assert!(
        statement.sql.ends_with(" LIMIT 20 OFFSET ?"),
        "{}",
        statement.sql
    );
    assert_eq!(statement.values.unwrap().0, vec![Value::BigInt(Some(0))]);
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
    let statement = build_count_select::<InternalCountUser>().build(OrmBackend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT COUNT(*) AS \"count\" FROM \"internal_count_users\""
    );
}

#[test]
fn exists_any_probe_reads_at_most_one_row() {
    let statement = build_exists_any_statement::<InternalCountUser>(OrmBackend::Postgres);

    assert_eq!(
        statement.sql,
        "SELECT 1 AS \"tideorm_exists\" FROM \"internal_count_users\" LIMIT 1"
    );
}

#[test]
fn one_row_statement_writes_its_limit_into_the_sql() {
    // A bound `LIMIT ?` makes recent SQLite recompile the statement on every run.
    let statement = one_row_statement(scoped_find::<InternalSoftDeleteUser>(), OrmBackend::Sqlite);

    assert!(
        statement.sql.ends_with(" IS NULL LIMIT 1"),
        "{}",
        statement.sql
    );
    assert!(statement.values.is_none_or(|values| values.0.is_empty()));
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
fn exists_any_probe_preserves_soft_delete_scope() {
    let statement = build_exists_any_statement::<InternalSoftDeleteUser>(OrmBackend::Postgres);

    assert!(
        statement
            .sql
            .contains("WHERE \"internal_soft_delete_users\".\"deleted_at\" IS NULL"),
        "{}",
        statement.sql
    );
}

#[test]
fn translate_error_maps_unset_attributes_to_validation_errors() {
    let err = translate_error(OrmError::AttrNotSet("email".to_string()));

    assert!(
        matches!(&err, crate::error::Error::Validation { field, .. } if field == "email"),
        "err: {err:?}"
    );
}

#[test]
fn translate_error_maps_key_arity_mismatch_to_a_query_error() {
    let err = translate_error(OrmError::KeyArityMismatch {
        expected: 2,
        received: 1,
    });

    assert!(
        matches!(err, crate::error::Error::Query { .. }),
        "key arity mismatch should not collapse into an internal error"
    );
}

#[test]
fn count_to_u64_rejects_negative_counts() {
    let err = count_to_u64(-1, "count(*)").unwrap_err();

    assert!(err.to_string().contains("negative count"));
    assert!(err.to_string().contains("count(*)"));
}

#[test]
fn migration_failures_are_query_errors_like_the_migrator_reports() {
    match translate_error(OrmError::Migration("relation exists".to_string())) {
        Error::Query { message, .. } => assert_eq!(message, "relation exists"),
        other => panic!("unexpected translation: {other:?}"),
    }
}

#[test]
fn access_control_failures_get_their_own_variants() {
    match translate_error(OrmError::AccessDenied {
        permission: "delete".to_string(),
        resource: "users".to_string(),
    }) {
        Error::AccessDenied {
            permission,
            resource,
        } => {
            assert_eq!(permission, "delete");
            assert_eq!(resource, "users");
        }
        other => panic!("unexpected translation: {other:?}"),
    }

    match translate_error(OrmError::RbacError("denied".to_string())) {
        Error::Rbac { message } => assert_eq!(message, "denied"),
        other => panic!("unexpected translation: {other:?}"),
    }
}

#[test]
fn engine_errors_without_a_tideorm_counterpart_stay_internal() {
    for err in [
        OrmError::Custom("boom".to_string()),
        OrmError::MutexPoisonError,
    ] {
        let translated = translate_error(err);
        assert!(
            matches!(translated, Error::Internal { .. }),
            "unexpected translation: {translated:?}"
        );
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
    let translated = translate_error(OrmError::Query(crate::orm::RuntimeErr::SqlxError(
        std::sync::Arc::new(crate::orm::sqlx::Error::Io(broken)),
    )));

    assert_eq!(translated.failure_kind(), DbFailureKind::ConnectionClosed);
    assert!(translated.is_retryable());
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
    assert!(driver_failure(&OrmError::MutexPoisonError).is_none());
    assert!(driver_failure(&OrmError::RecordNotInserted).is_none());
}

#[derive(Clone)]
struct PlainModel;

impl ModelMeta for PlainModel {
    type PrimaryKey = i64;

    fn table_name() -> &'static str {
        "plain_models"
    }

    fn primary_key_names() -> &'static [&'static str] {
        &["id"]
    }

    fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
        primary_key.to_string()
    }

    fn column_names() -> &'static [&'static str] {
        &["id", "name"]
    }

    fn field_names() -> &'static [&'static str] {
        &["id", "name"]
    }
}

#[derive(Clone)]
struct EncryptedModel;

impl ModelMeta for EncryptedModel {
    type PrimaryKey = i64;

    fn table_name() -> &'static str {
        "encrypted_models"
    }

    fn primary_key_names() -> &'static [&'static str] {
        &["id"]
    }

    fn primary_key_display(primary_key: &Self::PrimaryKey) -> String {
        primary_key.to_string()
    }

    fn column_names() -> &'static [&'static str] {
        &["id", "secret"]
    }

    fn field_names() -> &'static [&'static str] {
        &["id", "secret"]
    }

    fn encrypted_fields() -> Vec<&'static str> {
        vec!["secret"]
    }
}

#[test]
fn models_without_encrypted_fields_keep_the_plaintext_fallback() {
    assert!(reject_encryption_blind_conversion::<PlainModel>("try_into_active_model").is_ok());
}

#[test]
fn an_encrypted_model_never_falls_back_to_the_plaintext_conversion() {
    let error = reject_encryption_blind_conversion::<EncryptedModel>("try_from_entity_model")
        .expect_err("the plaintext fallback cannot decrypt");
    let message = error.to_string();

    assert!(message.contains("try_from_entity_model"), "{message}");
    assert!(message.contains("encrypted_models"), "{message}");
    assert!(message.contains("secret"), "{message}");
}

/// The engine quotes a URL it cannot parse, password included, and a
/// connection error is exactly what gets logged.
#[test]
fn connect_errors_mask_the_url_credentials() {
    let password = format!("pw{}", std::process::id());
    let url = format!("postgres://app:{password}@db.internal:99999/app");
    let error = translate_connect_error(
        OrmError::Conn(crate::orm::RuntimeErr::Internal(format!(
            "The connection string '{url}' cannot be parsed."
        ))),
        &url,
    );
    let shown = format!("{error} {error:?}");

    assert!(matches!(error, Error::Connection { .. }), "{shown}");
    assert!(!shown.contains(&password), "{shown}");
    assert!(
        shown.contains("postgres://***@db.internal:99999/app"),
        "{shown}"
    );
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
}
