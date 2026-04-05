use super::*;

mod user_cols {
    use tideorm::columns::Column;

    pub const ID: Column<i64> = Column::new("id");
    pub const NAME: Column<String> = Column::new("name");
    pub const AGE: Column<Option<i32>> = Column::new("age");
    pub const SCORE: Column<f64> = Column::new("score");
    pub const ACTIVE: Column<bool> = Column::new("active");
}

#[test]
fn test_column_creation() {
    let col: Column<i64> = Column::new("id");
    assert_eq!(col.name(), "id");

    let col: Column<String> = Column::new("name");
    assert_eq!(col.name(), "name");
}

#[test]
fn test_integer_column_eq() {
    let cond = user_cols::ID.eq(42i64);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, ColumnOperator::Eq);
    assert_eq!(cond.value, serde_json::json!(42));
}

#[test]
fn test_integer_column_ne() {
    let cond = user_cols::ID.ne(99i64);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, ColumnOperator::NotEq);
    assert_eq!(cond.value, serde_json::json!(99));
}

#[test]
fn test_integer_column_comparisons() {
    let gt = user_cols::ID.gt(10i64);
    assert_eq!(gt.operator, ColumnOperator::Gt);

    let gte = user_cols::ID.gte(10i64);
    assert_eq!(gte.operator, ColumnOperator::Gte);

    let lt = user_cols::ID.lt(100i64);
    assert_eq!(lt.operator, ColumnOperator::Lt);

    let lte = user_cols::ID.lte(100i64);
    assert_eq!(lte.operator, ColumnOperator::Lte);
}

#[test]
fn test_integer_column_between() {
    let cond = user_cols::ID.between(10i64, 100i64);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, ColumnOperator::Between);
    assert_eq!(cond.value, serde_json::json!([10, 100]));
}

#[test]
fn test_integer_column_in() {
    let cond = user_cols::ID.is_in(vec![1i64, 2, 3, 4, 5]);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, ColumnOperator::In);
    assert_eq!(cond.value, serde_json::json!([1, 2, 3, 4, 5]));
}

#[test]
fn test_integer_column_not_in() {
    let cond = user_cols::ID.not_in(vec![1i64, 2]);
    assert_eq!(cond.operator, ColumnOperator::NotIn);
}

#[test]
fn test_string_column_eq() {
    let cond = user_cols::NAME.eq("Alice");
    assert_eq!(cond.column, "name");
    assert_eq!(cond.operator, ColumnOperator::Eq);
    assert_eq!(cond.value, serde_json::json!("Alice"));
}

#[test]
fn test_string_column_like() {
    let cond = user_cols::NAME.like("%test%");
    assert_eq!(cond.column, "name");
    assert_eq!(cond.operator, ColumnOperator::Like);
    assert_eq!(cond.value, serde_json::json!("%test%"));
}

#[test]
fn test_string_column_not_like() {
    let cond = user_cols::NAME.not_like("%spam%");
    assert_eq!(cond.operator, ColumnOperator::NotLike);
}

#[test]
fn test_string_column_contains() {
    let cond = user_cols::NAME.contains("test");
    assert_eq!(cond.operator, ColumnOperator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("%test%"));
}

#[test]
fn test_string_column_starts_with() {
    let cond = user_cols::NAME.starts_with("Mr.");
    assert_eq!(cond.operator, ColumnOperator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("Mr.%"));
}

#[test]
fn test_string_column_ends_with() {
    let cond = user_cols::NAME.ends_with("son");
    assert_eq!(cond.operator, ColumnOperator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("%son"));
}

#[test]
fn test_string_column_in() {
    let cond = user_cols::NAME.is_in(vec!["Alice", "Bob", "Charlie"]);
    assert_eq!(cond.operator, ColumnOperator::In);
    assert_eq!(cond.value, serde_json::json!(["Alice", "Bob", "Charlie"]));
}

#[test]
fn test_nullable_column_comparisons() {
    let gt = user_cols::AGE.gt(18);
    assert_eq!(gt.column, "age");
    assert_eq!(gt.operator, ColumnOperator::Gt);

    let between = user_cols::AGE.between(18, 65);
    assert_eq!(between.operator, ColumnOperator::Between);
}

#[test]
fn test_nullable_column_is_null() {
    let cond = user_cols::AGE.is_null();
    assert_eq!(cond.column, "age");
    assert_eq!(cond.operator, ColumnOperator::IsNull);
}

#[test]
fn test_nullable_column_is_not_null() {
    let cond = user_cols::AGE.is_not_null();
    assert_eq!(cond.column, "age");
    assert_eq!(cond.operator, ColumnOperator::IsNotNull);
}

#[test]
fn test_bool_column_eq() {
    let cond = user_cols::ACTIVE.eq(true);
    assert_eq!(cond.column, "active");
    assert_eq!(cond.operator, ColumnOperator::Eq);
    assert_eq!(cond.value, serde_json::json!(true));
}

#[test]
fn test_float_column_comparisons() {
    let gt = user_cols::SCORE.gt(90.5);
    assert_eq!(gt.column, "score");
    assert_eq!(gt.operator, ColumnOperator::Gt);

    let between = user_cols::SCORE.between(0.0, 100.0);
    assert_eq!(between.operator, ColumnOperator::Between);
}

#[test]
fn test_column_operator_to_sql() {
    assert_eq!(ColumnOperator::Eq.to_sql(), "=");
    assert_eq!(ColumnOperator::NotEq.to_sql(), "<>");
    assert_eq!(ColumnOperator::Gt.to_sql(), ">");
    assert_eq!(ColumnOperator::Gte.to_sql(), ">=");
    assert_eq!(ColumnOperator::Lt.to_sql(), "<");
    assert_eq!(ColumnOperator::Lte.to_sql(), "<=");
    assert_eq!(ColumnOperator::Like.to_sql(), "LIKE");
    assert_eq!(ColumnOperator::NotLike.to_sql(), "NOT LIKE");
    assert_eq!(ColumnOperator::In.to_sql(), "IN");
    assert_eq!(ColumnOperator::NotIn.to_sql(), "NOT IN");
    assert_eq!(ColumnOperator::IsNull.to_sql(), "IS NULL");
    assert_eq!(ColumnOperator::IsNotNull.to_sql(), "IS NOT NULL");
    assert_eq!(ColumnOperator::Between.to_sql(), "BETWEEN");
}

#[test]
fn test_multiple_conditions_chain() {
    let c1 = user_cols::NAME.eq("Alice");
    let c2 = user_cols::AGE.gt(18);
    let c3 = user_cols::ACTIVE.eq(true);

    assert_eq!(c1.column, "name");
    assert_eq!(c2.column, "age");
    assert_eq!(c3.column, "active");
}
