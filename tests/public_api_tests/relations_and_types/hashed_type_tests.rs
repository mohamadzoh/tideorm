use tideorm::types::Hashed;

#[test]
fn test_hashed_uses_argon2_format() {
    let hashed = Hashed::new("secret123");
    assert!(hashed.hash().starts_with("$argon2"));
}

#[test]
fn test_hashed_verify_accepts_matching_password() {
    let hashed = Hashed::new("secret123");
    assert!(hashed.verify("secret123"));
    assert!(!hashed.verify("wrong-password"));
}

#[test]
fn test_hashed_is_salted() {
    let first = Hashed::new("secret123");
    let second = Hashed::new("secret123");

    assert_ne!(first.hash(), second.hash());
    assert!(first.verify("secret123"));
    assert!(second.verify("secret123"));
}

#[test]
fn test_hashed_verify_rejects_non_argon2_hashes() {
    let hashed = Hashed::from_hash("legacy-hash-value".to_string());

    assert!(!hashed.verify("secret123"));
    assert!(!hashed.verify("wrong-password"));
}
