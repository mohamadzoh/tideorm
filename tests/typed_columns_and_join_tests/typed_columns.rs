use super::*;
use serde_json::json;

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
        ("id eq", ID.eq(42i64), "id", Operator::Eq, single(json!(42))),
        (
            "id ne",
            ID.ne(99i64),
            "id",
            Operator::NotEq,
            single(json!(99)),
        ),
        ("id gt", ID.gt(10i64), "id", Operator::Gt, single(json!(10))),
        (
            "id gte",
            ID.gte(10i64),
            "id",
            Operator::Gte,
            single(json!(10)),
        ),
        (
            "id lt",
            ID.lt(100i64),
            "id",
            Operator::Lt,
            single(json!(100)),
        ),
        (
            "id lte",
            ID.lte(100i64),
            "id",
            Operator::Lte,
            single(json!(100)),
        ),
        (
            "id between",
            ID.between(10i64, 100i64),
            "id",
            Operator::Between,
            range(json!(10), json!(100)),
        ),
        (
            "id in",
            ID.is_in(vec![1i64, 2, 3, 4, 5]),
            "id",
            Operator::In,
            ConditionValue::List((1..=5).map(|id| json!(id)).collect()),
        ),
        (
            "id not in",
            ID.not_in(vec![1i64, 2]),
            "id",
            Operator::NotIn,
            ConditionValue::List(vec![json!(1), json!(2)]),
        ),
        (
            "name eq",
            NAME.eq("Alice"),
            "name",
            Operator::Eq,
            single(json!("Alice")),
        ),
        (
            "name like",
            NAME.like("%test%"),
            "name",
            Operator::Like,
            single(json!("%test%")),
        ),
        (
            "name not like",
            NAME.not_like("%spam%"),
            "name",
            Operator::NotLike,
            single(json!("%spam%")),
        ),
        (
            "name contains",
            NAME.contains("test"),
            "name",
            Operator::LikeEscaped,
            single(json!("%test%")),
        ),
        (
            "name starts with",
            NAME.starts_with("Mr."),
            "name",
            Operator::LikeEscaped,
            single(json!("Mr.%")),
        ),
        (
            "name ends with",
            NAME.ends_with("son"),
            "name",
            Operator::LikeEscaped,
            single(json!("%son")),
        ),
        (
            "name in",
            NAME.is_in(vec!["Alice", "Bob", "Charlie"]),
            "name",
            Operator::In,
            ConditionValue::List(vec![json!("Alice"), json!("Bob"), json!("Charlie")]),
        ),
        ("age gt", AGE.gt(18), "age", Operator::Gt, single(json!(18))),
        (
            "age between",
            AGE.between(18, 65),
            "age",
            Operator::Between,
            range(json!(18), json!(65)),
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
            single(json!(true)),
        ),
        (
            "score gt",
            SCORE.gt(90.5),
            "score",
            Operator::Gt,
            single(json!(90.5)),
        ),
        (
            "score between",
            SCORE.between(0.0, 100.0),
            "score",
            Operator::Between,
            range(json!(0.0), json!(100.0)),
        ),
    ];

    for (case, condition, column, operator, value) in cases {
        assert_eq!(condition.column, column, "{case}");
        assert_eq!(condition.operator, operator, "{case}");
        assert_eq!(condition.value, value, "{case}");
    }
}
