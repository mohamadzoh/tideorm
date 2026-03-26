use super::*;

#[test]
fn test_typed_column_creation() {
    let col: Column<i64> = Column::new("id");
    assert_eq!(col.name(), "id");
}

#[test]
fn test_typed_column_eq() {
    let col: Column<i64> = Column::new("id");
    let cond = col.eq(42);
    assert_eq!(cond.column, "id");
    assert_eq!(cond.operator, ColumnOperator::Eq);
    assert_eq!(cond.value, serde_json::json!(42));
}

#[test]
fn test_typed_column_string_like() {
    let col: Column<String> = Column::new("name");
    let cond = col.contains("test");
    assert_eq!(cond.column, "name");
    assert_eq!(cond.operator, ColumnOperator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!("%test%"));
}

#[test]
fn test_typed_column_string_like_escapes_metacharacters() {
    let col: Column<String> = Column::new("name");
    let cond = col.contains(r"100%_\done");
    assert_eq!(cond.operator, ColumnOperator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!(r"%100\%\_\\done%"));
}

#[test]
fn test_typed_column_nullable() {
    let col: Column<Option<i32>> = Column::new("age");
    let cond = col.is_null();
    assert_eq!(cond.column, "age");
    assert_eq!(cond.operator, ColumnOperator::IsNull);
}

#[test]
fn test_typed_column_between() {
    let col: Column<i32> = Column::new("score");
    let cond = col.between(10, 100);
    assert_eq!(cond.column, "score");
    assert_eq!(cond.operator, ColumnOperator::Between);
    assert_eq!(cond.value, serde_json::json!([10, 100]));
}

#[test]
fn test_typed_column_in() {
    let col: Column<String> = Column::new("status");
    let cond = col.is_in(vec!["active", "pending"]);
    assert_eq!(cond.column, "status");
    assert_eq!(cond.operator, ColumnOperator::In);
}
