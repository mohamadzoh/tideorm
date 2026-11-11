use super::*;

fn digest(value: impl Hash) -> String {
    let mut hasher = KeyDigest::new();
    value.hash(&mut hasher);
    hasher.hex()
}

#[test]
fn a_key_digest_is_256_bits_of_what_was_hashed() {
    let key = digest(("users", 42_i64));
    assert_eq!(key.len(), 64);
    assert!(
        key.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(key, digest(("users", 42_i64)));
    assert_ne!(key, digest(("users", 43_i64)));
    // BLAKE2s-256 of nothing, so the digest is the one named
    assert_eq!(
        KeyDigest::new().hex(),
        "69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9"
    );
}
