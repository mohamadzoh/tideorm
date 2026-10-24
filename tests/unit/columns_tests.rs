use super::*;
use crate::query::{ConditionValue, Operator};
use serde_json::json;

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
    assert_eq!(cond.value, ConditionValue::Single(json!("active")));

    let listed = Column::<Status>::of("users", "status").is_in([Status::Active, Status::Banned]);
    assert_eq!(listed.operator, Operator::In);
    assert_eq!(
        listed.value,
        ConditionValue::List(vec![json!("active"), json!("banned")])
    );

    // A nullable column compares with the plain value, and with None.
    let nullable: Column<Option<Status>> = Column::new("previous_status");
    assert_eq!(
        nullable.ne(Status::Banned).value,
        ConditionValue::Single(json!("banned"))
    );
    // A `None` is SQL NULL, apart from any value that serializes to `null`.
    let nullable: Column<Option<Status>> = Column::new("previous_status");
    assert_eq!(nullable.eq(None).value, ConditionValue::None);
    let ids: Column<Option<i64>> = Column::new("owner_id");
    assert_eq!(
        ids.is_in([1, 2]).value,
        ConditionValue::List(vec![json!(1), json!(2)])
    );
}

#[test]
fn a_nan_or_infinite_float_is_refused_rather_than_read_as_null() {
    // JSON writes a NaN as `null`, which a filter would read as `IS NULL`.
    let price: Column<f64> = Column::new("price");
    assert!(matches!(
        price.eq(f64::NAN).value,
        ConditionValue::Invalid(_)
    ));
    assert!(matches!(
        price.between(0.0, f64::INFINITY).value,
        ConditionValue::Invalid(_)
    ));
    assert!(matches!(
        price.is_in([1.0, f64::NEG_INFINITY]).value,
        ConditionValue::Invalid(_)
    ));
    // A None still means NULL, and a finite float binds.
    let nullable_price: Column<Option<f64>> = Column::new("price");
    assert_eq!(nullable_price.eq(None).value, ConditionValue::None);
    assert_eq!(price.eq(1.5).value, ConditionValue::Single(json!(1.5)));
}
