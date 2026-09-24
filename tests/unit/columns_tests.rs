use super::*;
use crate::query::Operator;

#[test]
fn test_typed_column_string_like_escapes_metacharacters() {
    let col: Column<String> = Column::new("name");
    let cond = col.contains(r"100%_\done");
    assert_eq!(cond.operator, Operator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!(r"%100!%!_\done%"));
}
