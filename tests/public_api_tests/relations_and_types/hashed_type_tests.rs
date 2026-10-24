use tideorm::types::Hashed;

#[test]
fn new_hashes_are_salted_argon2id_and_verify_only_their_input() {
    let hashes = [
        Hashed::new("secret123"),
        Hashed::try_new("secret123").unwrap(),
        "secret123".into(),
        "secret123".to_string().into(),
    ];

    assert!(
        hashes[0].hash().starts_with("$argon2id$"),
        "{}",
        hashes[0].hash()
    );
    assert_ne!(
        hashes[0].hash(),
        hashes[1].hash(),
        "each hash takes a fresh salt"
    );
    for hashed in &hashes {
        assert!(hashed.verify("secret123"));
        assert!(!hashed.verify("wrong-password"));
    }
}

/// A hash stored by an earlier release, with argon2 0.5, still verifies after
/// a dependency upgrade; a stored value that is no Argon2 hash verifies
/// nothing.
#[test]
fn stored_hashes_verify_across_releases() {
    let stored = Hashed::from_hash(
        "$argon2id$v=19$m=19456,t=2,p=1$Zml4ZWQtc2FsdC1ieXRlcw$AuzHlNYhVa2jgXvOE0GSBh+rjHdv9UiMpC26iZQLBmc"
            .to_string(),
    );
    assert!(stored.verify("hunter42"));
    assert!(!stored.verify("hunter43"));

    let legacy = Hashed::from_hash("legacy-hash-value".to_string());
    assert!(!legacy.verify("legacy-hash-value"));
}

/// The hash is what an offline attack on the password starts from, so it is
/// never shown or serialized.
#[test]
fn debug_display_and_serialize_leave_the_hash_out() {
    let hashed = Hashed::new("password123");

    assert_eq!(
        serde_json::to_value(&hashed).unwrap(),
        serde_json::json!("***HASHED***")
    );
    for rendered in [format!("{hashed:?}"), hashed.to_string()] {
        assert!(!rendered.contains("argon2"), "{rendered}");
        assert!(rendered.contains("***HASHED***"), "{rendered}");
    }
}

/// A stored hash deserializes as itself; the redacted form it serializes to
/// is refused rather than loaded as a hash that verifies nothing.
#[test]
fn deserialize_takes_a_stored_hash_and_refuses_the_redacted_form() {
    let stored: Hashed = serde_json::from_value(serde_json::json!(
        "$argon2id$v=19$m=19456,t=2,p=1$Zml4ZWQtc2FsdC1ieXRlcw$AuzHlNYhVa2jgXvOE0GSBh+rjHdv9UiMpC26iZQLBmc"
    ))
    .unwrap();
    assert!(stored.verify("hunter42"));

    let err = serde_json::from_value::<Hashed>(serde_json::json!("***HASHED***")).unwrap_err();
    assert!(err.to_string().contains("redacted serialization format"));
}
