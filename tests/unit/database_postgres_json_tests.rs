use super::*;
use crate::internal::driver::postgres::types::{PgDecodedArray, PgMoney};
use serde_json::json;

#[test]
fn driver_values_keep_the_existing_json_policy() {
    use PgDecodedValue as V;
    for (value, expected) in [
        (V::Null, json!(null)),
        (V::Bool(true), json!(true)),
        (V::Integer(42), json!(42)),
        (V::Float(1.5), json!(1.5)),
        (V::Float(f64::NAN), json!("NaN")),
        (V::Float(f64::INFINITY), json!("inf")),
        (
            V::Numeric("123456789012345678901234567890".into()),
            json!("123456789012345678901234567890"),
        ),
        (V::Numeric("NaN".into()), json!("NaN")),
        (V::Text("00123".into()), json!("00123")),
        (V::SpecialMoment("infinity"), json!("infinity")),
        (V::SpecialMoment("24:00:00"), json!("24:00:00")),
        (V::Bytes(vec![0, 255]), json!([0, 255])),
        (V::Money(PgMoney(-1)), json!("-0.01")),
        (V::Json(json!({"x":null})), json!({"x":null})),
    ] {
        assert_eq!(to_json(value), Some(expected));
    }
    let at = chrono::DateTime::parse_from_rfc3339("2020-01-01T10:00:00+00:00").unwrap();
    assert_eq!(
        to_json(V::TimestampWithTimeZone(at.to_utc())),
        Some(json!(at))
    );
    assert_eq!(
        to_json(V::Timestamp(at.naive_utc())),
        Some(json!(at.naive_utc()))
    );
    assert_eq!(to_json(V::Date(at.date_naive())), Some(json!("2020-01-01")));
    assert_eq!(
        to_json(V::Time(
            chrono::NaiveTime::from_hms_milli_opt(0, 0, 1, 500).unwrap()
        )),
        Some(json!("00:00:01.500"))
    );
}

#[test]
fn json_arrays_nest_dimensions_and_preserve_nulls() {
    let array = PgDecodedArray {
        dimensions: vec![2, 2],
        elements: vec![
            PgDecodedValue::Integer(1),
            PgDecodedValue::Null,
            PgDecodedValue::Integer(3),
            PgDecodedValue::Integer(4),
        ],
    };
    assert_eq!(
        to_json(PgDecodedValue::Array(array)),
        Some(json!([[1, null], [3, 4]]))
    );
}
