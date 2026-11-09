use super::*;
use tideorm::DbValue;

fn v(value: impl Into<DbValue>) -> DbValue {
    value.into()
}

mod user_cols {
    use tideorm::columns::Column;

    pub const ID: Column<i64> = Column::new("id");
    pub const NAME: Column<String> = Column::new("name");
    pub const AGE: Column<Option<i32>> = Column::new("age");
    pub const SCORE: Column<f64> = Column::new("score");
    pub const ACTIVE: Column<bool> = Column::new("active");
}

/// Each typed-column operator builds the condition it names on that column,
/// for plain, text, nullable, boolean and float columns alike.
#[test]
fn typed_column_operators_build_their_conditions() {
    use user_cols::{ACTIVE, AGE, ID, NAME, SCORE};

    let single = ConditionValue::Single;
    let range = ConditionValue::Range;
    let cases = [
        ("id eq", ID.eq(42i64), "id", Operator::Eq, single(v(42i64))),
        (
            "id ne",
            ID.ne(99i64),
            "id",
            Operator::NotEq,
            single(v(99i64)),
        ),
        ("id gt", ID.gt(10i64), "id", Operator::Gt, single(v(10i64))),
        (
            "id gte",
            ID.gte(10i64),
            "id",
            Operator::Gte,
            single(v(10i64)),
        ),
        (
            "id lt",
            ID.lt(100i64),
            "id",
            Operator::Lt,
            single(v(100i64)),
        ),
        (
            "id lte",
            ID.lte(100i64),
            "id",
            Operator::Lte,
            single(v(100i64)),
        ),
        (
            "id between",
            ID.between(10i64, 100i64),
            "id",
            Operator::Between,
            range(v(10i64), v(100i64)),
        ),
        (
            "id in",
            ID.is_in(vec![1i64, 2, 3, 4, 5]),
            "id",
            Operator::In,
            ConditionValue::List((1..=5i64).map(v).collect()),
        ),
        (
            "id not in",
            ID.not_in(vec![1i64, 2]),
            "id",
            Operator::NotIn,
            ConditionValue::List(vec![v(1i64), v(2i64)]),
        ),
        (
            "name eq",
            NAME.eq("Alice"),
            "name",
            Operator::Eq,
            single(v("Alice")),
        ),
        (
            "name like",
            NAME.like("%test%"),
            "name",
            Operator::Like,
            single(v("%test%")),
        ),
        (
            "name not like",
            NAME.not_like("%spam%"),
            "name",
            Operator::NotLike,
            single(v("%spam%")),
        ),
        (
            "name contains",
            NAME.contains("test"),
            "name",
            Operator::Contains,
            single(v("test")),
        ),
        (
            "name starts with",
            NAME.starts_with("Mr."),
            "name",
            Operator::StartsWith,
            single(v("Mr.")),
        ),
        (
            "name ends with",
            NAME.ends_with("son"),
            "name",
            Operator::EndsWith,
            single(v("son")),
        ),
        (
            "name in",
            NAME.is_in(vec!["Alice", "Bob", "Charlie"]),
            "name",
            Operator::In,
            ConditionValue::List(vec![v("Alice"), v("Bob"), v("Charlie")]),
        ),
        ("age gt", AGE.gt(18), "age", Operator::Gt, single(v(18))),
        (
            "age between",
            AGE.between(18, 65),
            "age",
            Operator::Between,
            range(v(18), v(65)),
        ),
        (
            "age is null",
            AGE.is_null(),
            "age",
            Operator::IsNull,
            ConditionValue::None,
        ),
        (
            "age is not null",
            AGE.is_not_null(),
            "age",
            Operator::IsNotNull,
            ConditionValue::None,
        ),
        (
            "active eq",
            ACTIVE.eq(true),
            "active",
            Operator::Eq,
            single(v(true)),
        ),
        (
            "score gt",
            SCORE.gt(90.5),
            "score",
            Operator::Gt,
            single(v(90.5)),
        ),
        (
            "score between",
            SCORE.between(0.0, 100.0),
            "score",
            Operator::Between,
            range(v(0.0), v(100.0)),
        ),
    ];

    for (case, condition, column, operator, value) in cases {
        assert_eq!(condition.column, column, "{case}");
        assert_eq!(condition.operator, operator, "{case}");
        assert_eq!(condition.value, value, "{case}");
    }
}
