//! Bulk increments, decrements and scaling stay within the model's integer, float
//! and decimal ranges, or the whole statement is rolled back.

use crate::fixtures::{MoneyRow, memory_database};
use rust_decimal::Decimal;
use serde_json::json;
use tideorm::prelude::*;

#[tokio::test]
async fn arithmetic_rejects_uuid_and_boolean_columns_without_changing_rows() {
    #[tideorm::model(table = "audit_arithmetic_types")]
    struct ArithmeticRow {
        #[tideorm(primary_key)]
        id: i64,
        token: Uuid,
        active: bool,
        amount: i64,
    }
    let db = memory_database("CREATE TABLE audit_arithmetic_types(id INTEGER PRIMARY KEY, token BLOB, active INTEGER, amount INTEGER); INSERT INTO audit_arithmetic_types VALUES(1,X'00112233445566778899AABBCCDDEEFF',1,10)").await;
    for field in ["token", "active"] {
        for operation in 0..4 {
            let update = ArithmeticRow::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .set("amount", 99);
            let update = match operation {
                0 => update.increment(field, 1),
                1 => update.decrement(field, 1),
                2 => update.multiply(field, 2.0),
                _ => update.divide(field, 2.0),
            };
            let error = update.execute().await.unwrap_err();
            assert!(
                error.to_string().contains("requires a numeric column"),
                "{error}"
            );
            let row = ArithmeticRow::find_with(1, &db).await.unwrap().unwrap();
            assert_eq!(
                row.token,
                Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap()
            );
            assert!(row.active);
            assert_eq!(row.amount, 10);
            assert_eq!(
                ArithmeticRow::query_with(&db)
                    .where_eq("active", true)
                    .count()
                    .await
                    .unwrap(),
                1
            );
        }
    }
    for operation in 0..4 {
        let update = ArithmeticRow::query_with(&db)
            .where_eq("id", 1)
            .update_all();
        let update = match operation {
            0 => update.increment("amount", 2),
            1 => update.decrement("amount", 2),
            2 => update.multiply("amount", 2.0),
            _ => update.divide("amount", 2.0),
        };
        assert_eq!(update.execute().await.unwrap(), 1);
    }
    assert_eq!(
        ArithmeticRow::find_with(1, &db)
            .await
            .unwrap()
            .unwrap()
            .amount,
        10
    );
}

#[tideorm::model(table = "float_behavior_rows")]
struct Row {
    #[tideorm(primary_key)]
    id: i64,
    amount: f64,
    narrow: f32,
    optional: Option<f64>,
}

async fn database() -> Database {
    memory_database("CREATE TABLE float_behavior_rows(id INTEGER PRIMARY KEY,amount REAL,narrow REAL,optional REAL); INSERT INTO float_behavior_rows VALUES(1,4.0,4.0,NULL),(2,4.0,4.0,4.0)").await
}

#[tokio::test]
async fn overflow_rejects_the_entire_update_for_both_float_widths() {
    let db = database().await;
    for (column, maximum) in [
        ("amount", f64::MAX),
        ("narrow", f64::from(f32::MAX)),
        ("optional", f64::MAX),
    ] {
        for sign in [-1.0, 1.0] {
            Row::query_with(&db)
                .where_eq("id", 2)
                .update_all()
                .set(column, sign * maximum)
                .execute()
                .await
                .unwrap();
            let before = Row::query_with(&db)
                .order_asc("id")
                .get_json()
                .await
                .unwrap();
            // Row 1 would be changed before the overflowing row 2. The error
            // must roll it back too, not just leave row 2 alone.
            for divide in [false, true] {
                let update = Row::query_with(&db).where_in("id", vec![1, 2]).update_all();
                let update = if divide {
                    update.divide(column, 0.5)
                } else {
                    update.multiply(column, 2.0)
                };
                assert!(
                    update.execute().await.is_err(),
                    "{column}, sign={sign}, divide={divide}"
                );
                assert_eq!(
                    Row::query_with(&db)
                        .order_asc("id")
                        .get_json()
                        .await
                        .unwrap(),
                    before
                );
                assert!(Row::query_with(&db).get().await.is_ok());
                assert!(Row::query_with(&db).pluck::<f64>("amount").await.is_ok());
                assert!(Row::query_with(&db).pluck::<f32>("narrow").await.is_ok());
            }
        }
    }
}

#[tokio::test]
async fn finite_boundaries_nulls_and_normal_arithmetic_round_trip() {
    let db = database().await;
    for (column, maximum) in [("amount", f64::MAX), ("narrow", f64::from(f32::MAX))] {
        for sign in [-1.0, 1.0] {
            Row::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .set(column, sign * maximum)
                .execute()
                .await
                .unwrap();
            for divide in [false, true] {
                let update = Row::query_with(&db).where_eq("id", 1).update_all();
                let update = if divide {
                    update.divide(column, 1.0)
                } else {
                    update.multiply(column, 1.0)
                };
                assert_eq!(update.execute().await.unwrap(), 1);
                let row = Row::find_with(1, &db).await.unwrap().unwrap();
                let actual = if column == "amount" {
                    row.amount
                } else {
                    f64::from(row.narrow)
                };
                assert_eq!(actual, sign * maximum);
            }
        }
    }
    // Multiple assignments exercise positional parameter order in the guards.
    Row::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .multiply("amount", 2.0)
        .divide("narrow", 2.0)
        .increment("optional", 3)
        .execute()
        .await
        .unwrap();
    let row = Row::find_with(2, &db).await.unwrap().unwrap();
    assert_eq!(
        (row.amount, row.narrow, row.optional),
        (8.0, 2.0, Some(7.0))
    );
    Row::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .decrement("amount", 3)
        .increment("narrow", 2)
        .decrement("optional", 2)
        .execute()
        .await
        .unwrap();
    let row = Row::find_with(2, &db).await.unwrap().unwrap();
    assert_eq!(
        (row.amount, row.narrow, row.optional),
        (5.0, 4.0, Some(5.0))
    );
    for operation in 0..4 {
        let update = Row::query_with(&db).where_eq("id", 1).update_all();
        let update = match operation {
            0 => update.multiply("optional", 2.0),
            1 => update.divide("optional", 2.0),
            2 => update.increment("optional", 2),
            _ => update.decrement("optional", 2),
        };
        update.execute().await.unwrap();
        assert_eq!(
            Row::find_with(1, &db).await.unwrap().unwrap().optional,
            None
        );
    }
}

#[tokio::test]
async fn f32_computed_values_match_rust_rounding_and_equality_filters() {
    let db = database().await;
    let starts = [
        0.7_f32,
        -0.7,
        1.0,
        -1.0,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::from_bits(3),
        -f32::from_bits(3),
        f32::from_bits(0x007fffff),
        f32::from_bits(0x3f800001),
        0.0,
    ];
    for start in starts {
        for operation in 0..6 {
            Row::query_with(&db)
                .where_eq("id", 1)
                .update_all()
                .set("narrow", start)
                .execute()
                .await
                .unwrap();
            let update = Row::query_with(&db).where_eq("id", 1).update_all();
            let (update, expected) = match operation {
                0 => (
                    update.increment("narrow", 1),
                    (f64::from(start) + 1.0) as f32,
                ),
                1 => (
                    update.decrement("narrow", 1),
                    (f64::from(start) - 1.0) as f32,
                ),
                2 => (
                    update.multiply("narrow", 0.3),
                    (f64::from(start) * 0.3) as f32,
                ),
                3 => (
                    update.divide("narrow", 3.0),
                    (f64::from(start) / 3.0) as f32,
                ),
                4 => (
                    update.multiply("narrow", 0.5),
                    (f64::from(start) * 0.5) as f32,
                ),
                _ => (
                    update.multiply("narrow", 1.0 + 2_f64.powi(-24)),
                    (f64::from(start) * (1.0 + 2_f64.powi(-24))) as f32,
                ),
            };
            update.execute().await.unwrap();
            let row = Row::find_with(1, &db).await.unwrap().unwrap();
            assert_eq!(row.narrow, expected, "start={start}, operation={operation}");
            assert_eq!(
                Row::query_with(&db)
                    .where_eq("id", 1)
                    .where_eq("narrow", expected)
                    .count()
                    .await
                    .unwrap(),
                1,
                "start={start}, operation={operation}"
            );
        }
    }
}

#[tokio::test]
async fn f32_rounding_preserves_nulls_and_outer_column_references() {
    #[tideorm::model(table = "float_named_values")]
    struct NamedRow {
        #[tideorm(primary_key)]
        id: i64,
        value: Option<f32>,
        quantum: f32,
    }
    let db = memory_database("CREATE TABLE float_named_values(id INTEGER PRIMARY KEY,value REAL,quantum REAL); INSERT INTO float_named_values VALUES(1,NULL,1),(2,NULL,1)").await;
    NamedRow::query_with(&db)
        .where_eq("id", 2)
        .update_all()
        .set("value", 0.7_f32)
        .execute()
        .await
        .unwrap();
    let rows = NamedRow::query_with(&db)
        .where_in("id", vec![1, 2])
        .update_all()
        .increment("value", 1)
        .multiply("quantum", 0.3)
        .execute_returning()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(
            row.value,
            if row.id == 1 {
                None
            } else {
                Some((f64::from(0.7_f32) + 1.0) as f32)
            }
        );
        assert_eq!(row.quantum, 0.3_f32);
        assert_eq!(
            NamedRow::query_with(&db)
                .where_eq("id", row.id)
                .where_eq("value", row.value)
                .where_eq("quantum", row.quantum)
                .count()
                .await
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn decimal_assignments_and_arithmetic_reject_unreadable_rows_atomically() {
    let db = memory_database("CREATE TABLE resume_decimals(id INTEGER PRIMARY KEY,amount REAL,marker INTEGER); INSERT INTO resume_decimals VALUES(1,1.25,0),(2,NULL,0)").await;
    let before = MoneyRow::query_with(&db)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    for invalid in [
        json!("not-a-decimal"),
        json!(true),
        json!([]),
        json!(Decimal::MAX),
        json!(Decimal::MIN),
    ] {
        for coalesce in [false, true] {
            let update = MoneyRow::query_with(&db)
                .where_gte("id", 1)
                .update_all()
                .set("marker", 1);
            let update = if coalesce {
                update.coalesce("amount", invalid.clone())
            } else {
                update.set("amount", invalid.clone())
            };
            assert!(
                update.execute().await.is_err(),
                "{invalid}, coalesce={coalesce}"
            );
            assert_eq!(
                MoneyRow::query_with(&db)
                    .order_asc("id")
                    .get_json()
                    .await
                    .unwrap(),
                before
            );
        }
    }
    for factor in [1e30, -1e30] {
        assert!(
            MoneyRow::query_with(&db)
                .where_gte("id", 1)
                .update_all()
                .multiply("amount", factor)
                .execute()
                .await
                .is_err()
        );
        assert_eq!(
            MoneyRow::query_with(&db)
                .order_asc("id")
                .get_json()
                .await
                .unwrap(),
            before
        );
    }
    // Row 1 can be written, but row 2 overflows: the complete statement rolls back.
    db.exec_raw("UPDATE resume_decimals SET amount=1 WHERE id=1; UPDATE resume_decimals SET amount=7e28 WHERE id=2").await.unwrap();
    let before = MoneyRow::query_with(&db)
        .order_asc("id")
        .get_json()
        .await
        .unwrap();
    assert!(
        MoneyRow::query_with(&db)
            .where_gte("id", 1)
            .update_all()
            .multiply("amount", 2.0)
            .execute()
            .await
            .is_err()
    );
    assert_eq!(
        MoneyRow::query_with(&db)
            .order_asc("id")
            .get_json()
            .await
            .unwrap(),
        before
    );
    for value in [json!("1.25"), json!(1.25), json!("1.25e0")] {
        MoneyRow::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .set("amount", value)
            .execute()
            .await
            .unwrap();
        assert_eq!(
            MoneyRow::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(Decimal::new(125, 2))
        );
    }
    MoneyRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .divide("amount", 0.5)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        MoneyRow::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(Decimal::new(25, 1))
    );
    MoneyRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .set("amount", None::<Decimal>)
        .execute()
        .await
        .unwrap();
    MoneyRow::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .increment("amount", 1)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        MoneyRow::find_with(1, &db).await.unwrap().unwrap().amount,
        None
    );
}

#[tideorm::model(table = "review_arithmetic")]
struct Arithmetic {
    #[tideorm(primary_key)]
    id: i64,
    amount: Option<i64>,
}

async fn arithmetic_database() -> Database {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_arithmetic(id INTEGER PRIMARY KEY, amount INTEGER)")
        .await
        .unwrap();
    db.exec_raw("INSERT INTO review_arithmetic VALUES(1,9007199254740993)")
        .await
        .unwrap();
    db
}

#[tokio::test]
async fn nonfinite_bulk_arithmetic_never_writes() {
    let db = arithmetic_database().await;
    for factor in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for divide in [false, true] {
            let update = Arithmetic::query_with(&db).where_eq("id", 1).update_all();
            let update = if divide {
                update.divide("amount", factor)
            } else {
                update.multiply("amount", factor)
            };
            assert!(update.execute().await.is_err());
            assert_eq!(
                Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
                Some(9007199254740993)
            );
        }
    }
    assert!(
        Arithmetic::query_with(&db)
            .where_eq("id", 1)
            .update_all()
            .divide("amount", 0.0)
            .execute()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sqlite_integer_scale_refuses_inexact_fallback() {
    let db = arithmetic_database().await;
    for divide in [false, true] {
        let update = Arithmetic::query_with(&db).where_eq("id", 1).update_all();
        let update = if divide {
            update.divide("amount", 1.0000000001)
        } else {
            update.multiply("amount", 1.0000000001)
        };
        let error = update.execute().await.unwrap_err();
        assert!(error.to_string().contains("exactly"), "{error}");
        assert_eq!(
            Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(9007199254740993)
        );
    }
    Arithmetic::query_with(&db)
        .where_eq("id", 1)
        .update_all()
        .multiply("amount", 0.5)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(4503599627370497)
    );
}

#[tokio::test]
async fn sqlite_scaling_overflow_aborts_the_whole_update() {
    let db = arithmetic_database().await;
    for (value, factor, divide) in [
        (4611686018427387904_i64, 2.0, false),
        (-4611686018427387905, 2.0, false),
        (i64::MIN, -1.0, false),
        (i64::MIN, -1.0, true),
        (4611686018427387904, 0.5, true),
    ] {
        db.exec_raw("DELETE FROM review_arithmetic").await.unwrap();
        db.exec_raw_with_params(
            "INSERT INTO review_arithmetic VALUES(1,2),(2,?)",
            vec![value.into()],
        )
        .await
        .unwrap();
        let update = Arithmetic::query_with(&db).where_gte("id", 1).update_all();
        let result = if divide {
            update.divide("amount", factor)
        } else {
            update.multiply("amount", factor)
        }
        .execute()
        .await;
        assert!(
            result.is_err(),
            "overflow unexpectedly succeeded: {value}, {factor}, {divide}"
        );
        assert_eq!(
            Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(2)
        );
        assert_eq!(
            Arithmetic::find_with(2, &db).await.unwrap().unwrap().amount,
            Some(value)
        );
    }
    db.exec_raw("UPDATE review_arithmetic SET amount=NULL WHERE id=2")
        .await
        .unwrap();
    Arithmetic::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .multiply("amount", 2.0)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
        Some(4)
    );
    assert_eq!(
        Arithmetic::find_with(2, &db).await.unwrap().unwrap().amount,
        None
    );
}

#[tokio::test]
async fn integer_addition_and_subtraction_overflow_roll_back_all_rows() {
    let db = arithmetic_database().await;
    for (amount, increment, operand) in [
        (i64::MAX, true, 1),
        (i64::MIN, false, 1),
        (i64::MAX, false, -1),
        (i64::MIN, true, -1),
    ] {
        db.exec_raw("DELETE FROM review_arithmetic").await.unwrap();
        db.exec_raw_with_params(
            "INSERT INTO review_arithmetic(id,amount) VALUES(1,2),(2,?)",
            vec![amount.into()],
        )
        .await
        .unwrap();
        let query = Arithmetic::query_with(&db).where_gte("id", 1).update_all();
        assert!(
            if increment {
                query.increment("amount", operand)
            } else {
                query.decrement("amount", operand)
            }
            .execute()
            .await
            .is_err()
        );
        assert_eq!(
            Arithmetic::find_with(1, &db).await.unwrap().unwrap().amount,
            Some(2)
        );
        assert_eq!(
            Arithmetic::find_with(2, &db).await.unwrap().unwrap().amount,
            Some(amount)
        );
    }
}

#[tideorm::model(table = "review_narrow_arithmetic")]
struct NarrowArithmetic {
    #[tideorm(primary_key)]
    id: i64,
    signed: Option<i32>,
    unsigned: u32,
}

#[tokio::test]
async fn sqlite_arithmetic_respects_model_integer_widths() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE review_narrow_arithmetic(id INTEGER PRIMARY KEY, signed INTEGER, unsigned INTEGER)").await.unwrap();
    db.exec_raw("INSERT INTO review_narrow_arithmetic VALUES(1, 2, 2), (2, 2147483647, 0)")
        .await
        .unwrap();
    for operation in 0..4 {
        let query = NarrowArithmetic::query_with(&db)
            .where_gte("id", 1)
            .update_all();
        let query = match operation {
            0 => query.increment("signed", 1),
            1 => query.decrement("unsigned", 1),
            2 => query.multiply("signed", 2.0),
            _ => query.divide("signed", 0.5),
        };
        assert!(query.execute().await.is_err(), "operation {operation}");
        assert_eq!(
            NarrowArithmetic::find_with(1, &db)
                .await
                .unwrap()
                .unwrap()
                .signed,
            Some(2)
        );
        let row = NarrowArithmetic::find_with(2, &db).await.unwrap().unwrap();
        assert_eq!(row.signed, Some(i32::MAX));
        assert_eq!(row.unsigned, 0);
    }
    db.exec_raw("UPDATE review_narrow_arithmetic SET signed=NULL WHERE id=2")
        .await
        .unwrap();
    NarrowArithmetic::query_with(&db)
        .where_gte("id", 1)
        .update_all()
        .increment("signed", 1)
        .execute()
        .await
        .unwrap();
    assert_eq!(
        NarrowArithmetic::find_with(2, &db)
            .await
            .unwrap()
            .unwrap()
            .signed,
        None
    );
}
