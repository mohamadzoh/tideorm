# Tokenization

Tokenization turns a record's primary key into an encrypted, URL-safe token, so URLs and API responses carry `v2.iIBmdKYhJh4_vSKFlBTP..` instead of `42`. A token decodes back to the key only with your application's key, and only for the model that issued it.

Add `tokenize` to the model and set an encryption key at startup:

```rust,ignore
use tideorm::prelude::*;

#[tideorm::model(table = "users", tokenize)]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
    pub name: String,
}

async fn example() -> tideorm::Result<()> {
    let key = std::env::var("ENCRYPTION_KEY").expect("ENCRYPTION_KEY must be set");
    TokenConfig::set_encryption_key(&key);

    let user = User::find(1).await?.unwrap();
    let token = user.to_token()?;           // "v2.iIBmdKYhJh4_vSKFlBTP..."

    let id = User::decode_token(&token)?;   // 1, without a query
    let same_user = User::from_token(&token).await?;
    assert_eq!(user.id, same_user.id);
    Ok(())
}
```

`tokenize` requires exactly one primary key field; on a model with a composite key it is a compile error.

## Setting the Key

Tokens use the encryption key that [encrypted fields](encrypted-fields.md) use. Set it once at startup, through `TideConfig` or directly:

```rust,ignore
let key = std::env::var("ENCRYPTION_KEY").expect("ENCRYPTION_KEY must be set");

TideConfig::init()
    .database("postgres://localhost/mydb")
    .encryption_key(&key)
    .connect()
    .await?;

// or, without TideConfig:
TokenConfig::set_encryption_key(&key);
```

- Without a key, every method that encodes or decodes a default token fails with `Error::Tokenization` ("No encryption key configured"), even the forms that otherwise return `Ok(None)`.
- An empty or blank key is ignored with a warning and leaves no key set. `TokenConfig::has_encryption_key()` tells whether one is set.
- Use a high-entropy secret of 32 or more characters, kept in the environment or a secret manager, never in source.
- Changing the key invalidates every token issued under the old one.

## Methods

A `tokenize` model implements the `Tokenizable` trait, which is in the prelude:

| Method | Returns |
|---|---|
| `user.to_token()` | the record's token. Each call gives a different token, from a fresh random nonce; all of them decode to the same key |
| `User::tokenize_id(42)` | the token for a key, without loading the record |
| `User::decode_token(&token)` | the primary key, without a query. An invalid token is an `Error::InvalidToken` |
| `User::try_decode_token(&token)` | the same, with `Ok(None)` for an invalid token |
| `User::from_token(&token).await` | the record. An invalid token is an `Error::InvalidToken`, and a token whose row is gone an `Error::NotFound` |
| `User::find_by_token(&token).await` | the same, with `Ok(None)` for an invalid token or a missing row |

`from_token` loads the record with `find()`, so on a [soft-delete model](soft-deletes.md) a token for a trashed row is not found.

## Reading Tokens From Requests

A token from a request may be garbled, forged, minted for another model, or name a row that has since been deleted. Use `find_by_token` and `try_decode_token` there: they answer all of those with `Ok(None)`, and only a missing key stays an error.

```rust,ignore
match User::find_by_token(&token).await? {
    Some(user) => respond_with(user),
    None => not_found(),
}

let id: Option<i64> = User::try_decode_token(&token)?;
```

Error messages leave the decoded key out, since hiding it is the point of the token.

## Tokens Belong to One Table

A token is bound to its model's schema and table, so a `User` token does not decode as a `Product`:

```rust,ignore
#[tideorm::model(table = "products", tokenize)]
pub struct Product {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
}

let user_token = User::tokenize_id(1)?;
let product_token = Product::tokenize_id(1)?;
assert_ne!(user_token, product_token);

assert!(User::decode_token(&product_token).is_err());
```

The binding is to the table, not the struct name: two `User` structs in different modules, on different tables, cannot open each other's tokens, while two models over the same table can. Table-bound tokens start with `v2.`; a token issued by TideORM 0.15 or earlier, bound to the struct name, no longer decodes.

## Tokens in API Responses

A model still carries its primary key, so returning it as it is puts the raw id back into the response. Hide the key and send the token in its place:

```rust,ignore
use tideorm::prelude::*;

#[tideorm::model(table = "users", tokenize, hidden = "id")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub email: String,
}

async fn show_user(token: &str) -> tideorm::Result<Option<serde_json::Value>> {
    let Some(user) = User::find_by_token(token).await? else {
        return Ok(None);
    };
    let mut body = user.try_to_json(None)?;   // `hidden` drops the raw id
    body["token"] = serde_json::json!(user.to_token()?);
    Ok(Some(body))
}

// GET /api/users/v2.iIBmdKYhJh4_vSKFlBTPgWRlbW8tZW5isZqLo_EU4YI
```

Tokens use the base64url alphabet (`A-Z a-z 0-9 - _`) plus the `v2.` prefix, so they need no escaping in a URL path.

## Custom Token Formats

The default tokens suit most applications. When you need another format, override the encoder and decoder for one model or for all of them. TideORM picks, for each model, the model's own codec first, then the global one, then the default.

An encoder receives the serialized primary key (its JSON text, such as `42`) and the model name, and returns the token. A decoder receives the token and the model name, and returns the serialized key, `Ok(None)` for a token it does not accept, or `Err` when it cannot decode at all. Pair every encoder with a decoder that accepts what it writes: a token the decoder rejects reads as invalid (`Ok(None)` from `find_by_token`), not as a configuration error, so a mismatched pair fails silently.

An override replaces the default encryption: the token's security is then yours. Overriding either codec also drops the table binding: an override receives the model name, and a default codec left on the other side binds to that name.

### For One Model

Implement `Tokenizable` by hand instead of using `tokenize`:

```rust,ignore
use std::sync::Arc;
use tideorm::prelude::*;

#[tideorm::model(table = "documents")]
pub struct Document {
    #[tideorm(primary_key)]
    pub id: i64,
    pub title: String,
}

#[async_trait] // from the prelude
impl Tokenizable for Document {
    type TokenPrimaryKey = i64;

    fn token_model_name() -> &'static str {
        "Document"
    }

    fn token_primary_key(&self) -> i64 {
        self.id
    }

    fn token_encoder() -> Option<TokenEncoder> {
        Some(Arc::new(|id: &str, _model: &str| Ok(format!("DOC-{id}"))))
    }

    fn token_decoder() -> Option<TokenDecoder> {
        Some(Arc::new(|token: &str, _model: &str| {
            Ok(token.strip_prefix("DOC-").map(ToOwned::to_owned))
        }))
    }

    async fn from_token(token: &str) -> tideorm::Result<Self> {
        let id = Self::decode_token(token)?;
        Self::find(id)
            .await?
            .ok_or_else(|| tideorm::Error::not_found("Document not found"))
    }
}
```

`token_model_name`, `token_primary_key` and `from_token` are required; the rest have defaults. `token_table()` returns the schema and table to bind default tokens to: the derive returns the model's own, and a hand-written impl returns `None` by default, which binds them to `token_model_name` instead, so two hand-written impls with one name accept each other's tokens. Name them apart.

### For Every Model

`TokenConfig::set_encoder` and `TokenConfig::set_decoder` install global codecs. They are closures, so they can capture what they need, such as a tenant prefix read at startup:

```rust,ignore
let tenant = std::env::var("TENANT").expect("TENANT must be set");
let prefix = format!("{tenant}-");

TokenConfig::set_encoder(move |id, model| {
    Ok(format!("{tenant}-{}-{}", model.to_lowercase(), id))
});

TokenConfig::set_decoder(move |token, model| {
    let model_prefix = format!("{}-", model.to_lowercase());
    Ok(token
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_prefix(&model_prefix))
        .map(ToOwned::to_owned))
});
```

`TideConfig::token_encoder(..)` and `TideConfig::token_decoder(..)` take the same closures and install them on `connect()` or `apply()`.

Setting a key, encoder or decoder again replaces the previous one. `TokenConfig::reset()` clears the key and both global codecs; `TideConfig::reset()` does the same along with the rest of the configuration. `TokenConfig::get_encoder()` and `get_decoder()` return the global codecs in effect, the default ones when none is set.

To build on the default format, wrap `tideorm::tokenization::default_encode` and `default_decode`, the codecs TideORM uses when nothing is overridden. They bind a token to the model name they are given, not to a table.

## Security

- Default tokens are sealed with XChaCha20-Poly1305, under a key derived from yours with Argon2id.
- The table (or, without one, the model name) is authenticated with the token, so it opens only for that table.
- A modified or truncated token fails authentication and decodes as invalid. A token longer than 4096 characters is refused before it is decoded.
- The output is randomized: one record has many valid tokens, so tokens cannot be compared to find equal keys.
- For high-security applications, plan how you rotate the key: rotation invalidates outstanding tokens.
