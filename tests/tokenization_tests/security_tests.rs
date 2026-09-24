use super::*;

use tideorm::tokenization::default_encode;

#[test]
fn test_token_bits_distribution() {
    init_test_env();

    let mut char_counts = std::collections::HashMap::new();

    for id in 1..=1000 {
        let id = id.to_string();
        let token = default_encode(&id, "User").unwrap();
        for c in token.chars() {
            *char_counts.entry(c).or_insert(0) += 1;
        }
    }

    let base64_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let represented: usize = base64_chars
        .chars()
        .filter(|c| char_counts.contains_key(c))
        .count();

    assert!(
        represented > 50,
        "Only {} of 64 Base64 characters represented",
        represented
    );
}

#[test]
fn test_no_id_leakage() {
    init_test_env();

    let id = "12345";
    let token = default_encode(id, "User").unwrap();

    assert!(!token.contains(id));
    assert!(!token.contains(&format!("{:x}", 12345)));
    assert!(!token.contains(&format!("{:o}", 12345)));
}
