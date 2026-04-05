use tideorm::query::{AggregateFunction, JoinClause, JoinType};

#[test]
fn test_join_type_as_sql() {
    assert_eq!(JoinType::Inner.as_sql(), "INNER JOIN");
    assert_eq!(JoinType::Left.as_sql(), "LEFT JOIN");
    assert_eq!(JoinType::Right.as_sql(), "RIGHT JOIN");
}

#[test]
fn test_join_type_clone_eq() {
    let jt1 = JoinType::Inner;
    let jt2 = jt1;
    assert_eq!(jt1, jt2);

    assert_ne!(JoinType::Inner, JoinType::Left);
    assert_ne!(JoinType::Left, JoinType::Right);
}

#[test]
fn test_join_type_debug() {
    assert_eq!(format!("{:?}", JoinType::Inner), "Inner");
    assert_eq!(format!("{:?}", JoinType::Left), "Left");
    assert_eq!(format!("{:?}", JoinType::Right), "Right");
}

#[test]
fn test_join_clause_creation() {
    let clause = JoinClause {
        join_type: JoinType::Inner,
        table: "users".to_string(),
        alias: None,
        left_column: "posts.user_id".to_string(),
        right_column: "users.id".to_string(),
    };

    assert_eq!(clause.join_type, JoinType::Inner);
    assert_eq!(clause.table, "users");
    assert!(clause.alias.is_none());
    assert_eq!(clause.left_column, "posts.user_id");
    assert_eq!(clause.right_column, "users.id");
}

#[test]
fn test_join_clause_with_alias() {
    let clause = JoinClause {
        join_type: JoinType::Left,
        table: "users".to_string(),
        alias: Some("u".to_string()),
        left_column: "posts.user_id".to_string(),
        right_column: "u.id".to_string(),
    };

    assert_eq!(clause.alias, Some("u".to_string()));
}

#[test]
fn test_join_clause_clone() {
    let clause = JoinClause {
        join_type: JoinType::Right,
        table: "comments".to_string(),
        alias: Some("c".to_string()),
        left_column: "posts.id".to_string(),
        right_column: "c.post_id".to_string(),
    };

    let cloned = clause.clone();
    assert_eq!(cloned.table, "comments");
    assert_eq!(cloned.alias, Some("c".to_string()));
}

#[test]
fn test_aggregate_function_variants() {
    let agg_count = AggregateFunction::Count;
    let agg_count_distinct = AggregateFunction::CountDistinct("category".to_string());
    let agg_sum = AggregateFunction::Sum("price".to_string());
    let agg_avg = AggregateFunction::Avg("rating".to_string());
    let agg_min = AggregateFunction::Min("created_at".to_string());
    let agg_max = AggregateFunction::Max("updated_at".to_string());

    assert!(matches!(agg_count, AggregateFunction::Count));
    assert!(matches!(
        agg_count_distinct,
        AggregateFunction::CountDistinct(_)
    ));
    assert!(matches!(agg_sum, AggregateFunction::Sum(_)));
    assert!(matches!(agg_avg, AggregateFunction::Avg(_)));
    assert!(matches!(agg_min, AggregateFunction::Min(_)));
    assert!(matches!(agg_max, AggregateFunction::Max(_)));
}

#[test]
fn test_aggregate_function_clone() {
    let agg = AggregateFunction::Sum("amount".to_string());
    let cloned = agg.clone();

    if let AggregateFunction::Sum(col) = cloned {
        assert_eq!(col, "amount");
    } else {
        panic!("Expected Sum variant");
    }
}

#[test]
fn test_aggregate_function_debug() {
    let agg = AggregateFunction::Avg("score".to_string());
    let debug = format!("{:?}", agg);
    assert!(debug.contains("Avg"));
    assert!(debug.contains("score"));
}
