use tideorm::query::{JoinType, Order};

#[test]
fn test_order_as_str() {
    assert_eq!(Order::Asc.as_str(), "ASC");
    assert_eq!(Order::Desc.as_str(), "DESC");
}

#[test]
fn test_join_type_as_sql() {
    assert_eq!(JoinType::Inner.as_sql(), "INNER JOIN");
    assert_eq!(JoinType::Left.as_sql(), "LEFT JOIN");
    assert_eq!(JoinType::Right.as_sql(), "RIGHT JOIN");
}
