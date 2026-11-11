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

/// The vouchers' table read by code: its tokens open for `Voucher` too.
#[tideorm::model(table = "token_vouchers", tokenize)]
struct VoucherCode {
    #[tideorm(primary_key)]
    code: String,
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

/// A token bound to a struct name, as tokens were before they were bound to
/// tables, opens for no derived model: two models may share that name.
#[test]
fn an_unversioned_token_opens_for_no_derived_model() {
    configure_key();
    let legacy = default_encode("7", "User").unwrap();
    assert!(!legacy.contains('.'), "{legacy}");

    invalid_token(admin::User::decode_token(&legacy));
    invalid_token(shop::User::decode_token(&legacy));
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
    let token = VoucherCode::tokenize_id("s3cret-coupon".to_string()).unwrap();

    let message = invalid_token(Voucher::decode_token(&token));
    assert!(!message.contains("s3cret"), "{message}");
    assert!(message.contains("Voucher"), "{message}");
}

/// `find_by_token` and `try_decode_token` read a token from a request: a
/// garbled, foreign or orphaned one is `None`, and only a missing key is an
/// error.
#[tokio::test]
async fn find_by_token_reads_a_bad_token_as_none() {
    configure_key();
    global_memory_database(
        "CREATE TABLE token_vouchers(id INTEGER PRIMARY KEY); INSERT INTO token_vouchers VALUES(3)",
    )
    .await;

    let token = Voucher::tokenize_id(3).unwrap();
    assert_eq!(Voucher::try_decode_token(&token).unwrap(), Some(3));
    let found = Voucher::find_by_token(&token).await.unwrap();
    assert_eq!(found.map(|voucher| voucher.id), Some(3));

    let orphaned = Voucher::tokenize_id(4).unwrap();
    assert_eq!(Voucher::try_decode_token(&orphaned).unwrap(), Some(4));
    assert!(Voucher::find_by_token(&orphaned).await.unwrap().is_none());
    assert!(matches!(
        Voucher::from_token(&orphaned).await,
        Err(Error::NotFound { .. })
    ));

    let foreign = shop::User::tokenize_id(3).unwrap();
    for bad in [foreign.as_str(), "v2.!!", ""] {
        assert_eq!(Voucher::try_decode_token(bad).unwrap(), None, "{bad:?}");
        assert!(
            Voucher::find_by_token(bad).await.unwrap().is_none(),
            "{bad:?}"
        );
    }

    TokenConfig::reset();
    let decoded = Voucher::try_decode_token(&token);
    let found = Voucher::find_by_token(&token).await;
    configure_key();
    assert!(matches!(decoded, Err(Error::Tokenization { .. })));
    assert!(matches!(found, Err(Error::Tokenization { .. })));
}

/// Global codecs may capture what they need, such as a prefix read at
/// startup.
#[test]
fn global_codecs_may_capture_their_settings() {
    let tenant = String::from("tenant-9");
    let prefix = format!("{tenant}.");
    TokenConfig::set_encoder(move |id, _model| Ok(format!("{tenant}.{id}")));
    TokenConfig::set_decoder(move |token, _model| {
        Ok(token.strip_prefix(&prefix).map(str::to_owned))
    });
    let token = Voucher::tokenize_id(8);
    let decoded = Voucher::decode_token("tenant-9.8");
    TokenConfig::reset();
    configure_key();
    assert_eq!(token.unwrap(), "tenant-9.8");
    assert_eq!(decoded.unwrap(), 8);
}
