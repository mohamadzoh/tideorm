use super::*;

use tideorm::tokenization::default_encode;

#[test]
fn test_tokens_not_predictable() {
    init_test_env();

    let token1 = default_encode("1", "User").unwrap();
    let token2 = default_encode("2", "User").unwrap();
    let token3 = default_encode("3", "User").unwrap();

    let common_prefix = common_prefix_len(&token1, &token2);
    let common_prefix2 = common_prefix_len(&token2, &token3);

    assert!(
        common_prefix < token1.len(),
        "Tokens share too much common prefix"
    );
    assert!(
        common_prefix2 < token2.len(),
        "Tokens share too much common prefix"
    );
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(ca, cb)| ca == cb)
        .count()
}

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

#[test]
fn test_model_name_binding() {
    init_test_env();

    let id = "42";
    let token_user = default_encode(id, "User").unwrap();
    let token_admin = default_encode(id, "Admin").unwrap();

    let common = common_prefix_len(&token_user, &token_admin);
    assert!(
        common < 10,
        "Model-specific tokens share too much in common"
    );
}
