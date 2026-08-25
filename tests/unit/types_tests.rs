use super::encrypted_field_missing_key_error;

#[test]
fn encrypted_missing_key_error_mentions_startup_configuration_for_serialization() {
    let err = encrypted_field_missing_key_error("serialization");
    let message = err.to_string();

    assert!(message.contains("Encrypted field serialization requires an encryption key"));
    assert!(message.contains("Configure one during startup"));
    assert!(message.contains("TideConfig::init().encryption_key"));
    assert!(message.contains("TokenConfig::set_encryption_key"));
    assert!(message.contains("#[tideorm(encrypted)]"));
}

#[test]
fn encrypted_missing_key_error_mentions_deserialization() {
    let err = encrypted_field_missing_key_error("deserialization");
    let message = err.to_string();

    assert!(message.contains("Encrypted field deserialization requires an encryption key"));
}

#[test]
fn castable_i32_rejects_values_outside_the_i32_range() {
    use super::Castable;

    assert_eq!(
        i32::from_json(&serde_json::json!(i32::MAX)).expect("in-range values still cast"),
        i32::MAX
    );

    let err = i32::from_json(&serde_json::json!(i64::from(i32::MAX) + 1))
        .expect_err("2^31 must not wrap around to -2^31");
    assert!(err.contains("out of range for i32"), "{err}");
}

#[test]
fn cast_value_integer_rejects_unsigned_values_above_i64_max() {
    let err = super::CastValue::cast(&serde_json::json!(u64::MAX), super::CastType::Integer)
        .expect_err("u64::MAX must not saturate to i64::MAX");
    assert!(err.contains("out of range for i64"), "{err}");
}

#[test]
fn cast_value_integer_rejects_floats_outside_the_i64_range() {
    let err = super::CastValue::cast(&serde_json::json!(1.0e30_f64), super::CastType::Integer)
        .expect_err("1e30 must not saturate to i64::MAX");
    assert!(err.contains("out of range for i64"), "{err}");
}

#[test]
fn cast_value_integer_still_truncates_in_range_fractions_toward_zero() {
    assert_eq!(
        super::CastValue::cast(&serde_json::json!(3.9), super::CastType::Integer)
            .expect("in-range fractions still cast"),
        serde_json::json!(3)
    );
    assert_eq!(
        super::CastValue::cast(&serde_json::json!(-3.9), super::CastType::Integer)
            .expect("in-range fractions still cast"),
        serde_json::json!(-3)
    );
}

#[test]
fn cast_value_decimal_keeps_money_strings_exact() {
    assert_eq!(
        super::CastValue::cast(
            &serde_json::json!("12345678901234567890.12"),
            super::CastType::Decimal
        )
        .expect("a decimal string must cast"),
        serde_json::json!("12345678901234567890.12"),
        "money strings must not round-trip through f64"
    );

    assert!(
        super::CastValue::cast(&serde_json::json!("not-a-number"), super::CastType::Decimal)
            .is_err()
    );
}

#[test]
fn cast_value_array_wraps_bare_scalar_strings() {
    assert_eq!(
        super::CastValue::cast(&serde_json::json!("5"), super::CastType::Array)
            .expect("a scalar string must cast"),
        serde_json::json!(["5"]),
        "a bare scalar must not pass through as a number"
    );
    assert_eq!(
        super::CastValue::cast(&serde_json::json!("[1,2]"), super::CastType::Array)
            .expect("a JSON array string must cast"),
        serde_json::json!([1, 2])
    );
    assert_eq!(
        super::CastValue::cast(&serde_json::json!("a, b"), super::CastType::Array)
            .expect("a comma-separated string must cast"),
        serde_json::json!(["a", "b"])
    );
}

#[test]
fn hashed_try_new_returns_a_verifiable_hash_without_unwrapping() {
    let hashed = super::Hashed::try_new("s3cret").expect("hashing a short password must succeed");

    assert!(hashed.verify("s3cret"));
    assert!(!hashed.verify("wrong"));
}

#[test]
fn unix_timestamp_millis_floors_negative_values_to_seconds() {
    let before_epoch = super::UnixTimestampMillis::new(-1_500);

    assert_eq!(before_epoch.as_seconds(), -2);
    assert_eq!(before_epoch.to_unix_timestamp().as_seconds(), -2);
    assert_eq!(
        before_epoch.as_seconds(),
        before_epoch
            .to_datetime()
            .expect("an in-range timestamp converts")
            .timestamp(),
        "as_seconds must agree with the total chrono conversion"
    );
    assert_eq!(super::UnixTimestampMillis::new(1_500).as_seconds(), 1);
}

#[test]
fn unix_timestamp_to_millis_saturates_instead_of_overflowing() {
    assert_eq!(
        super::UnixTimestampMillis::from(super::UnixTimestamp::new(i64::MAX)).as_millis(),
        i64::MAX
    );
    assert_eq!(
        super::UnixTimestampMillis::from(super::UnixTimestamp::new(i64::MIN)).as_millis(),
        i64::MIN
    );
    assert_eq!(
        super::UnixTimestampMillis::from(super::UnixTimestamp::new(-2)).as_millis(),
        -2_000
    );
}
