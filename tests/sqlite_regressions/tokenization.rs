//! Record tokens: bound to the model's schema and table, never to the struct
//! name alone, and quiet about the key they hide.

use crate::fixtures::global_memory_database;
use tideorm::Error;
use tideorm::prelude::*;
use tideorm::tokenization::{TokenConfig, default_decode, default_encode};

mod admin {
    #[tideorm::model(table = "token_admin_users", tokenize)]
    pub struct User {
        #[tideorm(primary_key)]
        pub id: i64,
    }
}

mod shop {
    #[tideorm::model(table = "token_shop_users", tokenize)]
    pub struct User {
        #[tideorm(primary_key)]
        pub id: i64,
    }
}

mod archive {
    #[tideorm::model(table = "token_shop_users", schema = "archive", tokenize)]
    pub struct User {
        #[tideorm(primary_key)]
        pub id: i64,
    }
}

#[tideorm::model(table = "token_vouchers", tokenize)]
struct Voucher {
    #[tideorm(primary_key)]
    id: i64,
}

fn configure_key() {
    TokenConfig::set_encryption_key("tokenization-regression-key");
}

fn invalid_token<T: std::fmt::Debug>(result: tideorm::Result<T>) -> String {
    match result.unwrap_err() {
        Error::InvalidToken { message } => message,
        other => panic!("expected an invalid token, got {other:?}"),
    }
}

/// Two `User` models in different modules shared a token binding: the admin
/// token for user 7 loaded the shop's user 7.
#[tokio::test]
async fn a_token_opens_only_for_the_table_it_was_issued_for() {
    configure_key();
    global_memory_database(
        "CREATE TABLE token_admin_users(id INTEGER PRIMARY KEY); \
         CREATE TABLE token_shop_users(id INTEGER PRIMARY KEY); \
         INSERT INTO token_admin_users VALUES(7); INSERT INTO token_shop_users VALUES(7)",
    )
    .await;

    let admin_token = admin::User::tokenize_id(7).unwrap();
    assert!(admin_token.starts_with("v2."), "{admin_token}");
    assert_eq!(admin::User::from_token(&admin_token).await.unwrap().id, 7);
    invalid_token(shop::User::decode_token(&admin_token));
    invalid_token(shop::User::from_token(&admin_token).await);

    // The same table in another schema is another binding too.
    let shop_token = shop::User::tokenize_id(7).unwrap();
    assert_eq!(shop::User::decode_token(&shop_token).unwrap(), 7);
    invalid_token(archive::User::decode_token(&shop_token));
    invalid_token(admin::User::decode_token(&shop_token));
}

/// A token issued before tokens were bound to tables is bound to the struct
/// name, and still decodes for a model of that name only.
#[test]
fn an_unversioned_token_decodes_for_the_struct_name_it_was_bound_to() {
    configure_key();
    let legacy = default_encode("7", "User").unwrap();
    assert!(!legacy.contains('.'), "{legacy}");

    assert_eq!(admin::User::decode_token(&legacy).unwrap(), 7);
    assert_eq!(shop::User::decode_token(&legacy).unwrap(), 7);
    invalid_token(Voucher::decode_token(&legacy));
}

/// A decoder override built on the default decoder, beside the default
/// encoder, reads the tokens the model mints: those are bound to the struct
/// name it receives, not to the table, which it could not open.
#[test]
fn a_decoder_override_reads_the_tokens_the_model_mints() {
    fn wrapping(token: &str, model_name: &str) -> tideorm::Result<Option<String>> {
        default_decode(token, model_name)
    }

    configure_key();
    TokenConfig::set_decoder(wrapping);
    let token = Voucher::tokenize_id(5);
    let decoded = token
        .as_ref()
        .ok()
        .map(|token| Voucher::decode_token(token));
    TokenConfig::reset();
    configure_key();
    let token = token.unwrap();
    assert!(!token.starts_with("v2."), "{token}");
    assert_eq!(decoded.unwrap().unwrap(), 5);
}

/// A `v2.` token stripped of its prefix, an empty or garbled one, and an
/// unknown version stay invalid tokens, and without a key the unprefixed form
/// is a configuration error as a whole token is; `tests/tokenization_tests.rs`
/// covers tampered tokens, and the tokenization unit tests the missing key.
#[test]
fn tampered_tokens_are_invalid_and_a_missing_key_is_a_configuration_error() {
    configure_key();
    let token = Voucher::tokenize_id(42).unwrap();
    let unprefixed = token.trim_start_matches("v2.");
    for bad in [unprefixed, "v2.", "v2.!!", "v3.AAAA"] {
        invalid_token(Voucher::decode_token(bad));
    }

    TokenConfig::reset();
    match Voucher::decode_token(unprefixed).unwrap_err() {
        Error::Tokenization { message } => {
            assert!(
                message.contains("No encryption key configured"),
                "{message}"
            );
        }
        other => panic!("expected a configuration error, got {other:?}"),
    }
    configure_key();
}

/// A payload that opens but is no key of the model used to be quoted in the
/// error, putting what the token hides into logs and responses.
#[test]
fn a_payload_that_is_no_key_stays_out_of_the_error() {
    configure_key();
    let token = default_encode("\"s3cret-coupon\"", "Voucher").unwrap();

    let message = invalid_token(Voucher::decode_token(&token));
    assert!(!message.contains("s3cret"), "{message}");
    assert!(message.contains("Voucher"), "{message}");
}
