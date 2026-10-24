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

    // The table exists nowhere, so reaching the database would fail with a
    // connection or query error instead.
    match insert_all::<ValidatedUser>(batch).await {
        Err(Error::Validation { field, message }) => {
            assert_eq!(field, "email");
            assert!(message.contains("model at index 1"), "{message}");
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

/// `paginate` refuses a page before it reads: a zero page or page size, or a
/// page size or offset past `i64::MAX`. The drivers take LIMIT and OFFSET as
/// `i64`, and SQLite's and PostgreSQL's panic converting a larger `u64`.
#[tokio::test]
async fn paginate_rejects_a_page_no_backend_can_read() {
    for (page, per_page, field, message) in [
        (0, 10, "page", "must be at least 1"),
        (1, 0, "per_page", "must be greater than 0"),
        (
            1,
            i64::MAX as u64 + 1,
            "per_page",
            "must be at most i64::MAX",
        ),
        (u64::MAX, 4, "page", "exceeds i64::MAX"),
        (3, 1 << 62, "page", "exceeds i64::MAX"),
    ] {
        let error = crate::internal::QueryExecutor::paginate::<PaginationUser>(page, per_page)
            .await
            .expect_err("the page should be rejected");
        assert!(
            matches!(
                &error,
                Error::Validation { field: refused, message: why }
                    if refused == field && why.contains(message)
            ),
            "page {page} of {per_page}: {error:?}"
        );
    }
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
