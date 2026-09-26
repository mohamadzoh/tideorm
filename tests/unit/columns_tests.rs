use super::*;
use crate::query::Operator;

#[test]
fn test_typed_column_string_like_escapes_metacharacters() {
    let col: Column<String> = Column::new("name");
    let cond = col.contains(r"100%_\done");
    assert_eq!(cond.operator, Operator::LikeEscaped);
    assert_eq!(cond.value, serde_json::json!(r"%100!%!_\done%"));
}

#[test]
fn test_a_column_of_any_serializable_type_compares_and_lists() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "snake_case")]
    enum Status {
        Active,
        Banned,
    }

    let status: Column<Status> = Column::of("users", "status");
    let cond = status.eq(Status::Active);
    assert_eq!(cond.column, "users.status");
    assert_eq!(cond.operator, Operator::Eq);
    assert_eq!(cond.value, serde_json::json!("active"));

    let listed = Column::<Status>::of("users", "status").is_in([Status::Active, Status::Banned]);
    assert_eq!(listed.operator, Operator::In);
    assert_eq!(listed.value, serde_json::json!(["active", "banned"]));

    // A nullable column compares with the plain value, and with None.
    let nullable: Column<Option<Status>> = Column::new("previous_status");
    assert_eq!(
        nullable.ne(Status::Banned).value,
        serde_json::json!("banned")
    );
    let nullable: Column<Option<Status>> = Column::new("previous_status");
    assert_eq!(nullable.eq(None).value, serde_json::Value::Null);
    let ids: Column<Option<i64>> = Column::new("owner_id");
    assert_eq!(ids.is_in([1, 2]).value, serde_json::json!([1, 2]));
}
