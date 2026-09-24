use crate::model::Model;

#[tideorm::model(table = "pagination_users")]
struct PaginationUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[test]
fn test_page_rejects_an_overflowing_offset() {
    let err = PaginationUser::query()
        .page(u64::MAX, 2)
        .ensure_query_is_valid()
        .expect_err("an overflowing offset must invalidate the query");

    assert!(
        err.to_string().contains("overflows the maximum offset"),
        "{err}"
    );
}

#[test]
fn test_page_rejects_zero_page_number() {
    let err = PaginationUser::query()
        .page(0, 10)
        .ensure_query_is_valid()
        .expect_err("page 0 must invalidate the query");

    assert!(err.to_string().contains("at least 1"), "{err}");
}

#[test]
fn test_page_sets_limit_and_offset() {
    let query = PaginationUser::query().page(3, 25);

    assert!(query.ensure_query_is_valid().is_ok());
    assert_eq!(query.limit_value, Some(25));
    assert_eq!(query.offset_value, Some(50));
}
