use super::*;
use crate::internal::driver::mysql::types::MySqlTimeSign;
use MySqlDecodedValue as V;
use serde_json::json;

#[test]
fn driver_values_map_to_json_without_losing_digits() {
    let digits = "12345678901234567890123456789012345.123456789012345678901234567890";
    let uuid = uuid::Uuid::from_u128(1);
    for (value, expected) in [
        (V::Null, json!(null)),
        (V::Bool(false), json!(false)),
        (V::Integer(-5), json!(-5)),
        (V::Unsigned(u64::MAX), json!(u64::MAX)),
        (V::Float(f64::NAN), json!("NaN")),
        (V::Decimal(digits.into()), json!(digits)),
        (V::Text("00123".into()), json!("00123")),
        (V::Bytes(vec![0, 255]), json!([0, 255])),
        (V::Json(json!({"a": [1]})), json!({"a": [1]})),
        (V::Uuid(uuid), json!(uuid.to_string())),
    ] {
        assert_eq!(to_json(value), Some(expected));
    }
}

#[test]
fn times_are_signed_durations_written_as_a_time_of_day_is() {
    for (sign, hours, micros, expected) in [
        (MySqlTimeSign::Negative, 1, 0, "-01:30:00"),
        (MySqlTimeSign::Positive, 100, 500_000, "100:30:00.500"),
        (MySqlTimeSign::Positive, 0, 1, "00:30:00.000001"),
    ] {
        let time = MySqlTime::new(sign, hours, 30, 0, micros).unwrap();
        assert_eq!(to_json(V::Time(time)), Some(json!(expected)));
    }
}
