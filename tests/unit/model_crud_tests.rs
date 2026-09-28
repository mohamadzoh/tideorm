use super::*;

#[tideorm::model(table = "crud_pagination_users")]
struct PaginationUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "crud_soft_delete_users", soft_delete)]
struct SoftDeleteUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[tideorm::model(table = "crud_validated_users")]
struct ValidatedUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(email)]
    email: String,
}

#[tokio::test]
async fn insert_all_rejects_an_invalid_model_before_touching_the_database() {
    let batch = vec![
        ValidatedUser {
            id: 0,
            email: "a@example.com".to_string(),
        },
        ValidatedUser {
            id: 0,
            email: "not an email".to_string(),
        },
    ];

    // No connection is configured, so reaching the database would fail with
    // a connection error instead.
    match insert_all::<ValidatedUser>(batch).await {
        Err(Error::Validation { field, message }) => {
            assert_eq!(field, "email");
            assert!(message.contains("model 1 of the batch"), "{message}");
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn paginate_rejects_a_zero_page_number() {
    let error = crate::internal::QueryExecutor::paginate::<PaginationUser>(0, 10)
        .await
        .expect_err("page 0 should be rejected");

    assert!(
        error.to_string().contains("must be at least 1"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn paginate_rejects_a_zero_page_size() {
    let error = crate::internal::QueryExecutor::paginate::<PaginationUser>(1, 0)
        .await
        .expect_err("per_page 0 should be rejected");

    assert!(
        error.to_string().contains("must be greater than 0"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn paginate_reports_an_offset_that_would_overflow() {
    let error = crate::internal::QueryExecutor::paginate::<PaginationUser>(u64::MAX, 4)
        .await
        .expect_err("an overflowing offset should be reported");

    assert!(
        error.to_string().contains("exceeds i64::MAX"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn paginate_rejects_a_page_size_or_offset_past_i64_max() {
    // The drivers take LIMIT and OFFSET as `i64`; SQLite's and PostgreSQL's
    // panic converting a larger `u64`.
    let error = crate::internal::QueryExecutor::paginate::<PaginationUser>(1, i64::MAX as u64 + 1)
        .await
        .expect_err("an oversized page size should be rejected");
    assert!(
        matches!(&error, Error::Validation { field, .. } if field == "per_page"),
        "unexpected error: {error:?}"
    );

    let error = crate::internal::QueryExecutor::paginate::<PaginationUser>(3, 1 << 62)
        .await
        .expect_err("an offset past i64::MAX should be rejected");
    assert!(
        matches!(&error, Error::Validation { field, .. } if field == "page"),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn the_reload_lookup_reports_a_missing_connection_as_a_connection_error() {
    // `find` reports an outage as `Error::Connection`; the unscoped lookup
    // `reload` uses must too, or callers miss every connection-specific branch.
    crate::database::Database::reset_global();

    let error = reload_by_key::<SoftDeleteUser>(1)
        .await
        .expect_err("a missing global connection should be reported");

    assert!(
        matches!(error, Error::Connection { .. }),
        "expected a connection error, got: {error:?}"
    );
}
