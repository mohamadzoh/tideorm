//! SQLite has no decimal type: a `Decimal` is read as the value is stored.

use crate::fixtures::{MoneyRow, memory_database};
use rust_decimal::Decimal;
use tideorm::prelude::*;

#[tokio::test]
async fn decimals_read_text_and_integers_exactly() {
    // A column with no declared type keeps each value's storage class.
    let db = memory_database(
        "CREATE TABLE resume_decimals(id INTEGER PRIMARY KEY, amount, marker INTEGER);
         INSERT INTO resume_decimals VALUES
             (1, '12345678901234567890.123456789', 0),
             (2, 9007199254740993, 0),
             (3, 1.5, 0),
             (4, NULL, 0)",
    )
    .await;
    let amounts: Vec<_> = MoneyRow::query_with(&db)
        .order_asc("id")
        .get()
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.amount)
        .collect();
    assert_eq!(
        amounts,
        [
            Some("12345678901234567890.123456789".parse::<Decimal>().unwrap()),
            Some(Decimal::from(9_007_199_254_740_993_i64)),
            Some(Decimal::new(15, 1)),
            None,
        ]
    );
    let exact = MoneyRow::find_with(1, &db).await.unwrap().unwrap().amount;
    assert_eq!(exact.unwrap().to_string(), "12345678901234567890.123456789");
}

/// `MIN`/`MAX` of a decimal column compare with a decimal as numbers: SQLite
/// gives the aggregate no affinity, so a decimal bound as text was always
/// greater.
#[tokio::test]
async fn having_an_extreme_decimal_compares_numbers() {
    let db = memory_database(
        "CREATE TABLE resume_decimals(id INTEGER PRIMARY KEY, amount DECIMAL(10, 2), marker INTEGER);
         INSERT INTO resume_decimals VALUES (1, 1.5, 1), (2, 3.25, 2), (3, 2, 2)",
    )
    .await;
    let markers = |condition: HavingCondition| {
        let db = db.clone();
        async move {
            MoneyRow::query_with(&db)
                .select_raw("marker")
                .group_by("marker")
                .having(condition)
                .get_json()
                .await
                .unwrap()
                .len()
        }
    };
    let bound = Decimal::new(25, 1);
    assert_eq!(markers(Aggregate::max("amount").gt(bound).into()).await, 1);
    assert_eq!(markers(Aggregate::min("amount").lt(bound).into()).await, 2);
    assert_eq!(markers(Aggregate::max("amount").gt("2.5").into()).await, 1);
}
