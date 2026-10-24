use crate::model::Model;

#[tideorm::model(table = "pagination_users")]
struct PaginationUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[test]
fn test_page_rejects_arguments_that_name_no_page() {
    for (page, per_page, field, message) in [
        (0, 10, "page", "at least 1"),
        (1, 0, "per_page", ""),
        (u64::MAX, 2, "page", "exceeds i64::MAX"),
    ] {
        let err = PaginationUser::query()
            .page(page, per_page)
            .ensure_query_is_valid()
            .expect_err("the page must invalidate the query");

        assert!(
            matches!(&err, crate::error::Error::Validation { field: name, .. } if name == field),
            "page({page}, {per_page}): {err}"
        );
        assert!(
            err.to_string().contains(message),
            "page({page}, {per_page}): {err}"
        );
    }
}

#[test]
fn test_page_sets_limit_and_offset() {
    let query = PaginationUser::query().page(3, 25);

    assert!(query.ensure_query_is_valid().is_ok());
    assert_eq!(query.clauses.limit_value, Some(25));
    assert_eq!(query.clauses.offset_value, Some(50));
}
