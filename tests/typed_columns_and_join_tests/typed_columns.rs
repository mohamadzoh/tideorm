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
fn test_integer_column_eq() {
    let cond = user_cols::ID.eq(42i64);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, Operator::Eq);
    assert_eq!(cond.value, serde_json::json!(42));
}

#[test]
fn test_integer_column_ne() {
    let cond = user_cols::ID.ne(99i64);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, Operator::NotEq);
    assert_eq!(cond.value, serde_json::json!(99));
}

#[test]
fn test_integer_column_comparisons() {
    let gt = user_cols::ID.gt(10i64);
    assert_eq!(gt.operator, Operator::Gt);

    let gte = user_cols::ID.gte(10i64);
    assert_eq!(gte.operator, Operator::Gte);

    let lt = user_cols::ID.lt(100i64);
    assert_eq!(lt.operator, Operator::Lt);

    let lte = user_cols::ID.lte(100i64);
    assert_eq!(lte.operator, Operator::Lte);
}

#[test]
fn test_integer_column_between() {
    let cond = user_cols::ID.between(10i64, 100i64);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, Operator::Between);
    assert_eq!(cond.value, serde_json::json!([10, 100]));
}

#[test]
fn test_integer_column_in() {
    let cond = user_cols::ID.is_in(vec![1i64, 2, 3, 4, 5]);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, Operator::In);
    assert_eq!(cond.value, serde_json::json!([1, 2, 3, 4, 5]));
}

#[test]
fn test_integer_column_not_in() {
    let cond = user_cols::ID.not_in(vec![1i64, 2]);
    assert_eq!(cond.operator, Operator::NotIn);
}

#[test]
fn test_string_column_eq() {
    let cond = user_cols::NAME.eq("Alice");
    assert_eq!(cond.column, "name");
    assert_eq!(cond.operator, Operator::Eq);
    assert_eq!(cond.value, serde_json::json!("Alice"));
}

#[test]
fn test_string_column_like() {
    let cond = user_cols::NAME.like("%test%");
    assert_eq!(cond.column, "name");
    assert_eq!(cond.operator, Operator::Like);
    assert_eq!(cond.value, serde_json::json!("%test%"));
}

#[test]
fn test_string_column_not_like() {
    let cond = user_cols::NAME.not_like("%spam%");
    assert_eq!(cond.operator, Operator::NotLike);
}

#[test]
fn test_string_column_contains() {
    let cond = user_cols::NAME.contains("test");
    assert_eq!(cond.operator, Operator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("%test%"));
}

#[test]
fn test_string_column_starts_with() {
    let cond = user_cols::NAME.starts_with("Mr.");
    assert_eq!(cond.operator, Operator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("Mr.%"));
}

#[test]
fn test_string_column_ends_with() {
    let cond = user_cols::NAME.ends_with("son");
    assert_eq!(cond.operator, Operator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("%son"));
}

#[test]
fn test_string_column_in() {
    let cond = user_cols::NAME.is_in(vec!["Alice", "Bob", "Charlie"]);
    assert_eq!(cond.operator, Operator::In);
    assert_eq!(cond.value, serde_json::json!(["Alice", "Bob", "Charlie"]));
}

#[test]
fn test_nullable_column_comparisons() {
    let gt = user_cols::AGE.gt(18);
    assert_eq!(gt.column, "age");
    assert_eq!(gt.operator, Operator::Gt);

    let between = user_cols::AGE.between(18, 65);
    assert_eq!(between.operator, Operator::Between);
}

#[test]
fn test_nullable_column_is_null() {
    let cond = user_cols::AGE.is_null();
    assert_eq!(cond.column, "age");
    assert_eq!(cond.operator, Operator::IsNull);
}

#[test]
fn test_nullable_column_is_not_null() {
    let cond = user_cols::AGE.is_not_null();
    assert_eq!(cond.column, "age");
    assert_eq!(cond.operator, Operator::IsNotNull);
}

#[test]
fn test_bool_column_eq() {
    let cond = user_cols::ACTIVE.eq(true);
    assert_eq!(cond.column, "active");
    assert_eq!(cond.operator, Operator::Eq);
    assert_eq!(cond.value, serde_json::json!(true));
}

#[test]
fn test_float_column_comparisons() {
    let gt = user_cols::SCORE.gt(90.5);
    assert_eq!(gt.column, "score");
    assert_eq!(gt.operator, Operator::Gt);

    let between = user_cols::SCORE.between(0.0, 100.0);
    assert_eq!(between.operator, Operator::Between);
}
