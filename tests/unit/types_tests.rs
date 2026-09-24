#[cfg(feature = "encrypted-fields")]
use super::encrypted::encrypted_field_missing_key_error;

#[cfg(feature = "encrypted-fields")]
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

#[cfg(feature = "encrypted-fields")]
#[test]
fn encrypted_missing_key_error_mentions_deserialization() {
    let err = encrypted_field_missing_key_error("deserialization");
    let message = err.to_string();

    assert!(message.contains("Encrypted field deserialization requires an encryption key"));
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
