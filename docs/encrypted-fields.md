# Encrypted Fields

> Requires the `encrypted-fields` feature.

An encrypted field is stored encrypted in its column and holds plaintext in your model. Use it for values the database should not hold in the clear, such as phone numbers or document numbers, when you only read them back by row.

```toml
[dependencies]
tideorm = { version = "0.18.0", features = ["postgres", "encrypted-fields"] }
```

List the fields in the model's `encrypted` option, by field name or column name:

```rust,ignore
use tideorm::prelude::*;

#[tideorm::model(table = "customers", encrypted = "customer_phone_number, backup_phone")]
pub struct Customer {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    #[tideorm(column = "customer_phone_number")]
    pub phone_number: String,
    pub backup_phone: Option<String>,
}

async fn register() -> tideorm::Result<Customer> {
    let customer = Customer::create(Customer {
        name: "Ada".into(),
        phone_number: "+44 20 7946 0000".into(),
        ..Default::default()
    })
    .await?; // the column holds ciphertext

    Ok(Customer::find(customer.id).await?.unwrap()) // phone_number is plaintext again
}
```

Without the feature, `encrypted = ".."` fails to compile, with a note that the item it needs is gated behind `encrypted-fields`.

## Setting the Key

Encryption needs a key, set once at startup. It is the same key [tokenization](tokenization.md) uses:

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

- Without a key, every write of an encrypted field, and every read of a stored payload, fails with `Error::Configuration`; nothing is stored in plaintext.
- An empty or blank key is ignored with a warning and leaves no key set, so an unset environment variable read with `unwrap_or_default()` fails the same way.
- Changing the key makes every value written under the old one unreadable. There is no built-in key rotation.

Load the key from the environment or a secret manager, never from source.

## What Is Encrypted

Every write that stores the model encrypts the field, and every read that builds a model or reads the column decrypts it:

| Encrypted on | Decrypted on |
|---|---|
| `create()`, `save()`, `update()` | `find()`, `all()`, `first()` and the other model reads |
| `insert_all()` | query builder reads (`get()`, `first()`, `paginate()`) |
| upserts (`on_conflict`, `insert_or_update`) | eager-loaded relations |
| nested saves and entity manager flushes | raw SQL decoded into the model |
| batch `update_all().set(..)` and `coalesce(..)` | `pluck`, `value`, `get_json`, `get_as` |

`pluck`, `value`, `get_json` and `get_as` decrypt columns of the query's model and of joined models, and keep decrypting a column read through an alias or a scalar subquery.

A model never holds ciphertext: a row decrypts as it is read, and a write encrypts a copy of the value.

## Field Types and Columns

An encrypted field can be of any type that implements `Serialize` and `Deserialize`: `String`, numbers, `Option`s, your own structs. The column stores the encrypted JSON of the value as text, so declare it `TEXT` whatever the field's type; schema sync and schema export already do.

- `None`, and any value whose JSON is `null`, is stored as `NULL`. An `Option<serde_json::Value>` holding the JSON `null` reads back as `None`.
- In a batch update, `set()` and `coalesce()` take any filter value, such as `serde_json::to_value(&card)?` for a struct field or `None` for an `Option` field, and refuse one the field's type cannot read ("cannot hold a JSON string") before anything is written.
- The other batch setters (`increment`, `multiply`, the array and JSON setters, `set_trusted_raw`) are refused on an encrypted field, since the database cannot compute on ciphertext.

```rust,ignore
Customer::update_all()
    .set(Customer::BACKUP_PHONE, None::<String>)
    .where_eq(Customer::ID, id)
    .execute()
    .await?;
```

## Querying Encrypted Columns

Each write encrypts with a fresh random nonce, so the same value is stored differently every time, and the database cannot compare plaintext:

| Operation | Behaviour |
|---|---|
| `where_eq` and the other filters | compare the stored ciphertext, not the plaintext. A filter on an encrypted column does not find a row by its value |
| `DISTINCT`, `count_distinct`, `GROUP BY`, `ORDER BY` | refused |
| `MIN`, `MAX`, `SUM`, `AVG`, `aggregates()`, HAVING comparisons | refused |
| window functions: `MIN`/`MAX`/`SUM`/`AVG`, `PARTITION BY`, `ORDER BY` | refused |
| `UNION` | refused when a projection is encrypted, since deduplication would compare ciphertext |
| `UNION ALL` | allowed with explicit columns or the default projection, when every operand has the same table and column at each encrypted position; refused for raw SQL or wildcard operands |
| full-text search | refused |

The refusals cover aliases, joined models' columns and scalar subquery outputs too, and fail when the query runs, with an error explaining that randomized ciphertext cannot compare plaintext values. Load the rows and compare or aggregate the values in Rust.

Typed unions also refuse operands whose output types differ at one position, such as a boolean and an integer, since every row is decoded the same way.

To look a row up by a protected value, keep a separate column the application can match, such as a keyed hash of the value, beside the encrypted one.

## What Cannot Be Encrypted

These are compile errors:

- a primary key: its ciphertext changes on every write, so no lookup by the key could match it;
- an `#[index]` or `#[unique_index]` covering an encrypted field: a unique index would never see two equal values;
- a `searchable = ".."` entry naming an encrypted field.

An upsert whose conflict columns include an encrypted field fails when it runs; conflict on a plaintext unique key instead.

## Existing Data

An encrypted column must hold TideORM's encrypted payloads or `NULL`. Loading a row whose column holds plaintext fails with "Encrypted field '..' loaded plaintext data"; a payload written under another key or for another column fails to decrypt. Before listing an existing column in `encrypted`, migrate its data: read the rows with a model that does not encrypt the column, then write each value back through a model that does.

## How It Works

- Values are sealed with XChaCha20-Poly1305 and stored as `enc::` followed by base64url text.
- Each column gets its own key, derived with Argon2id from the configured key, the table name and the column name, and cached after first use. A payload copied into another column or table does not decrypt there.
- Tampering with a stored payload makes its row fail to load.

`ModelMeta::encrypted_fields()` lists a model's encrypted fields as `(field, column)` pairs, and `has_encrypted_fields()` tells whether it has any.

With [project rules](project-rules.md), `sensitive_fields` flags any column whose name looks sensitive and that is neither hidden nor encrypted.

## Password Hashes

Encryption is two-way: the application reads the value back. Store a password one-way instead. `Hashed` (in the prelude, and `tideorm::types::Hashed`) hashes a value with Argon2 under a fresh random salt and checks a candidate against the stored hash:

```rust,ignore
use tideorm::prelude::*;

// Hashed::new does the same and panics if Argon2 fails, which needs a 4 GiB input.
user.password_hash = Hashed::try_new(&password)?.hash().to_owned();
user.save().await?;

let matches = Hashed::from_hash(user.password_hash.clone()).verify(&attempt);
```

`Hashed` has no column mapping, so keep the hash in a `String` field and leave that field out of JSON with `hidden = "password_hash"` (see [Hidden Attributes](models.md#hidden-attributes)). `Hashed` itself never shows the hash: its `Debug`, `Display` and `Serialize` write `***HASHED***`, and deserializing that placeholder fails.
