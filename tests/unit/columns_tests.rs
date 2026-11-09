use super::*;
use crate::DbValue;
use crate::query::{ConditionValue, Operator};

#[test]
fn test_a_column_of_any_type_a_column_holds_compares_and_lists() {
    enum Status {
        Active,
        Banned,
    }
    impl From<Status> for DbValue {
        fn from(status: Status) -> Self {
            match status {
                Status::Active => "active",
                Status::Banned => "banned",
            }
            .into()
        }
    }
    impl crate::orm::sql_builder::Nullable for Status {
        fn null() -> DbValue {
            DbValue::String(None)
        }
    }

    let status: Column<Status> = Column::of("users", "status");
    let cond = status.eq(Status::Active);
    assert_eq!(cond.column, "users.status");
    assert_eq!(cond.operator, Operator::Eq);
    assert_eq!(cond.value, ConditionValue::Single("active".into()));

    let listed = Column::<Status>::of("users", "status").is_in([Status::Active, Status::Banned]);
    assert_eq!(listed.operator, Operator::In);
    assert_eq!(
        listed.value,
        ConditionValue::List(vec!["active".into(), "banned".into()])
    );

    // A nullable column compares with the plain value, and with None.
    let nullable: Column<Option<Status>> = Column::new("previous_status");
    assert_eq!(
        nullable.ne(Status::Banned).value,
        ConditionValue::Single("banned".into())
    );
    // A `None` is SQL NULL, apart from the JSON `null` document.
    let nullable: Column<Option<Status>> = Column::new("previous_status");
    assert_eq!(nullable.eq(None).value, ConditionValue::None);
    let document: Column<serde_json::Value> = Column::new("settings");
    assert_eq!(
        document.eq(serde_json::Value::Null).value,
        ConditionValue::Single(DbValue::Json(Some(Box::new(serde_json::Value::Null))))
    );
    let ids: Column<Option<i64>> = Column::new("owner_id");
    assert_eq!(
        ids.is_in([1, 2]).value,
        ConditionValue::List(vec![1i64.into(), 2i64.into()])
    );
}

#[test]
fn a_value_keeps_its_own_type() {
    let id: Column<uuid::Uuid> = Column::new("id");
    let uuid = uuid::Uuid::nil();
    assert_eq!(id.eq(uuid).value, ConditionValue::Single(uuid.into()));
    let amount: Column<rust_decimal::Decimal> = Column::new("amount");
    let exact = rust_decimal::Decimal::new(1050, 2);
    assert_eq!(amount.gt(exact).value, ConditionValue::Single(exact.into()));
    let big: Column<u64> = Column::new("big");
    assert_eq!(
        big.eq(u64::MAX).value,
        ConditionValue::Single(DbValue::BigUnsigned(Some(u64::MAX)))
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
    assert_eq!(price.eq(1.5).value, ConditionValue::Single(1.5f64.into()));
}
