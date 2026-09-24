use super::*;
use serde_json::json;

#[tideorm::model(table = "column_value_rows")]
struct ColumnValueRow {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    uid: uuid::Uuid,
    #[tideorm(column = "happened_at")]
    moment: DateTime<Utc>,
    day: NaiveDate,
    price: Decimal,
    title: String,
}

#[test]
fn column_types_resolve_by_field_or_column_name_on_the_models_own_table() {
    assert!(matches!(
        column_type_of::<ColumnValueRow>("uid"),
        Some(ColumnType::Uuid)
    ));
    assert!(matches!(
        column_type_of::<ColumnValueRow>("happened_at"),
        Some(ColumnType::TimestampWithTimeZone)
    ));
    assert!(matches!(
        column_type_of::<ColumnValueRow>("moment"),
        Some(ColumnType::TimestampWithTimeZone)
    ));
    assert!(matches!(
        column_type_of::<ColumnValueRow>("column_value_rows.day"),
        Some(ColumnType::Date)
    ));
    assert!(column_type_of::<ColumnValueRow>("other_table.day").is_none());
    assert!(column_type_of::<ColumnValueRow>("missing").is_none());
}

#[test]
fn strings_bind_as_the_type_of_their_column() {
    let id = uuid::Uuid::new_v4();
    assert_eq!(
        json_to_column_value(&json!(id), Some(&ColumnType::Uuid)),
        Value::from(id)
    );

    let moment = DateTime::parse_from_rfc3339("2024-02-29T23:59:59.123456Z")
        .unwrap()
        .with_timezone(&Utc);
    for text in [
        "2024-02-29T23:59:59.123456Z",
        "2024-02-29 23:59:59.123456+00:00",
        "2024-03-01T01:59:59.123456+02:00",
        "2024-02-29T23:59:59.123456",
    ] {
        assert_eq!(
            json_to_column_value(&json!(text), Some(&ColumnType::TimestampWithTimeZone)),
            Value::from(moment),
            "{text}"
        );
    }

    let naive = moment.naive_utc();
    for text in ["2024-02-29T23:59:59.123456", "2024-02-29 23:59:59.123456"] {
        assert_eq!(
            json_to_column_value(&json!(text), Some(&ColumnType::DateTime)),
            Value::from(naive),
            "{text}"
        );
    }
    let midnight = NaiveDate::from_ymd_opt(2024, 2, 29)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    assert_eq!(
        json_to_column_value(&json!("2024-02-29"), Some(&ColumnType::DateTime)),
        Value::from(midnight)
    );
    assert_eq!(
        json_to_column_value(&json!("2024-02-29"), Some(&ColumnType::Date)),
        Value::from(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
    );
    assert_eq!(
        json_to_column_value(&json!("23:59:59.5"), Some(&ColumnType::Time)),
        Value::from(NaiveTime::from_hms_milli_opt(23, 59, 59, 500).unwrap())
    );
    assert_eq!(
        json_to_column_value(&json!("09:30"), Some(&ColumnType::Time)),
        Value::from(NaiveTime::from_hms_opt(9, 30, 0).unwrap())
    );
}

#[test]
fn numbers_and_text_cross_over_to_the_column_type() {
    let exact: Decimal = "12.50".parse().unwrap();
    assert_eq!(
        json_to_column_value(&json!("12.50"), Some(&ColumnType::Decimal(None))),
        Value::from(exact)
    );
    assert_eq!(
        json_to_column_value(&json!(0.1), Some(&ColumnType::Decimal(None))),
        Value::from("0.1".parse::<Decimal>().unwrap())
    );
    assert_eq!(
        json_to_column_value(&json!("42"), Some(&ColumnType::BigInteger)),
        Value::from(42i64)
    );
    assert_eq!(
        json_to_column_value(&json!("2.5"), Some(&ColumnType::Double)),
        Value::from(2.5f64)
    );
    assert_eq!(
        json_to_column_value(&json!(123), Some(&ColumnType::Text)),
        Value::from("123".to_string())
    );
}

#[test]
fn nulls_are_typed_and_unparseable_values_fall_back_to_generic_binding() {
    let null = serde_json::Value::Null;
    assert_eq!(
        json_to_column_value(&null, Some(&ColumnType::Integer)),
        Value::from(None::<i64>)
    );
    assert_eq!(
        json_to_column_value(&null, Some(&ColumnType::Uuid)),
        Value::from(None::<uuid::Uuid>)
    );
    for (value, column_type) in [
        (json!("not-a-uuid"), ColumnType::Uuid),
        (json!("yesterday"), ColumnType::TimestampWithTimeZone),
        (json!("abc"), ColumnType::BigInteger),
        (json!(true), ColumnType::Uuid),
    ] {
        assert_eq!(
            json_to_column_value(&value, Some(&column_type)),
            json_to_db_value(&value),
            "{value} as {column_type:?}"
        );
    }
    assert_eq!(
        json_to_column_value(&json!("x"), None),
        json_to_db_value(&json!("x"))
    );
}
