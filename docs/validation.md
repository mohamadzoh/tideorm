# Validation

Declare rules on a model's fields with `#[validate(..)]`, and TideORM checks them before every write. A model that breaks a rule is never sent to the database.

```rust
use tideorm::prelude::*;

#[tideorm::model(table = "users")]
pub struct User {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    #[validate(required, email)]
    pub email: String,
    #[validate(min_length = 3, max_length = 20, alphanumeric)]
    pub username: String,
    #[validate(range(18, 120))]
    pub age: i32,
}

fn check() {
    let user = User {
        email: "not-an-email".into(),
        username: "x".into(),
        age: 12,
        ..Default::default()
    };

    if let Err(errors) = user.validate() {
        for (field, message) in errors.errors() {
            println!("{field}: {message}");
        }
    }
}
```

`create()`, `update()`, `save()`, `insert_all()` and upserts check every rule first and fail with `Error::Validation`. For `insert_all()` the error names the failing model's position in the batch. Batch updates (`update_all()`) and query writes do not validate. Call `validate()` yourself to collect every failure at once; `validated()` returns the model itself on success, for call chains.

## Rules

| Rule | Checks | Applies to |
|---|---|---|
| `required` | the value is not empty: not `None`, not blank text, not an empty `Vec`, not JSON `null` | any field, usually an `Option` |
| `email` | an email address | text |
| `url` | an `http` or `https` URL with a host | text |
| `alpha` | letters only | text |
| `alphanumeric` | letters and digits only | text |
| `numeric` | a finite number | text |
| `uuid` | a UUID | text |
| `min_length = n`, `max_length = n`, `length = n` | the length in characters, not bytes | text |
| `min = n`, `max = n` | the lower or upper bound, inclusive | numbers, `Decimal`, numeric text |
| `range(min, max)` | both bounds, inclusive | numbers, `Decimal`, numeric text |
| `regex = "pattern"` | the text matches the pattern | text |
| `custom = "function"` or `custom = function` | your own function, see [Custom Rules](#custom-rules) | any field |

Text means `String`, `&str` and `Text` fields, optional or not. A rule on a field of a type it does not apply to is a compile error, and so is an unknown rule.

Details:

- `range` is also written `range(min..=max)` or `range = "min..max"`; all three include `max`. The half-open `range(min..max)` is refused, since in Rust it would leave `max` out.
- `min`, `max` and `range` take finite bounds only, reject `NaN`, and compare an `f32` at its own precision, so `min = 0.7` accepts `0.7_f32`. An integer is compared exactly, even past 2^53.
- `numeric` refuses `NaN`, `inf` and a value past `f64`.
- `alpha` and `alphanumeric` accept Unicode letters and digits, not only ASCII.
- A `regex` pattern that does not compile fails every value, with an error naming the pattern.
- Text rules pass a value that is not text, such as `None`: only `required` rejects a missing value.
- Errors come out in the order the struct declares its fields.

The default messages read "The email must be a valid email address", "The username must be at least 3 characters", and so on.

### Custom Rules

`custom` calls a function with a reference to the field. Return `Ok(())` to pass, or an error of any type that implements `Display`, which becomes the field's message. It runs beside the field's other rules, wherever they run:

```rust
use tideorm::prelude::*;

fn not_reserved(name: &str) -> Result<(), String> {
    match name {
        "admin" | "root" => Err(format!("'{name}' is reserved")),
        _ => Ok(()),
    }
}

#[tideorm::model(table = "accounts")]
pub struct Account {
    #[tideorm(primary_key, auto_increment)]
    pub id: i64,
    #[validate(custom = "not_reserved", max_length = 32)]
    pub name: String,
}
```

### Rules on Skip Fields

Rules on a `#[tideorm(skip)]` field run too, for a value that is checked but never stored, such as a password confirmation. A skip field holds its `Default` on every model a query returns, so make it an `Option`: `None` passes every rule except `required`.

`#[validate]` on a relation field is a compile error; put the rules on the related model's fields.

## Validation and Callbacks

A single-model write runs `before_validation`, then the rules, then `after_validation`, and only then `before_save`. What `before_save` changes is therefore not validated; normalize input in `before_validation` when the result must pass the rules. `insert_all()` and upserts run no callbacks, so each model is validated as it was built. See [Callbacks and Timestamps](callbacks.md).

## Handling Validation Errors

A failed `create()`, `save()` or `update()` returns `Error::Validation`. Its `validation_errors()` gives back every failure, field by field, to answer a form with:

```rust,ignore
match user.save().await {
    Ok(user) => Ok(user),
    Err(error) => match error.validation_errors() {
        Some(errors) => Err(form_errors(errors.errors())), // Vec<(field, message)>
        None => Err(error.into()),
    },
}
```

`ValidationErrors` is the collection `validate()` returns:

| Method | Returns |
|---|---|
| `errors()` | every failure as `(field, message)` pairs |
| `field_errors(field)` | one field's messages, as a `Vec<String>` |
| `get(field)` | one field's messages, or `None` |
| `first()` | the first `(field, message)` |
| `messages()` | every failure as `"field: message"` text |
| `iter()` | `(field, messages)` pairs, in the order the fields failed |
| `all()` | the underlying map of field to messages |
| `is_empty()`, `len()` | whether there are failures, and how many fields failed |
| `add(field, message)` | records a failure |
| `merge(other)` | appends another collection's failures |
| `to_result()` | `Ok(())` when empty, otherwise `Err(self)` |

Its `Display` joins the failures with `; `. It converts into `tideorm::Error` with `?` or `.into()`; the error's own `field` and `message` are the first failure's, and `validation_errors()` keeps all of them. `Error::validation(field, message)` builds one with a single failure, for instance in a callback.

```rust
use tideorm::prelude::*;

fn collect() -> tideorm::Result<()> {
    let mut errors = ValidationErrors::new();
    errors.add("email", "Email is required");
    errors.add("password", "Password must be at least 8 characters");

    for message in errors.field_errors("email") {
        println!("email: {message}");
    }
    println!("{errors}");
    errors.to_result()?;
    Ok(())
}
```

## Applying Rules by Hand

The same rules are available outside models, through `ValidationRule`, `Validator` and `ValidationBuilder` (all in the prelude, and in `tideorm::validation`).

`ValidationBuilder::new(field)` collects rules for one field. `validate(&value)` checks a value against all of them, and `build()` returns the field name and the list of `ValidationRule`s:

```rust
use tideorm::prelude::*;

fn check_username(username: &str) {
    let username = username.to_string();

    let result = ValidationBuilder::new("username")
        .required()
        .min_length(3)
        .max_length(20)
        .alphanumeric()
        .validate(&username);

    let (field, rules) = ValidationBuilder::new("role")
        .in_list(["admin", "editor", "viewer"])
        .build();
}
```

The builder has a method per rule: `required`, `email`, `url`, `min_length`, `max_length`, `length`, `min`, `max`, `range`, `regex`, `alpha`, `alphanumeric`, `numeric`, `uuid`, plus `in_list(values)` and `not_in(values)` for allow and deny lists. `min`, `max` and `range` keep their bounds as written, so `min(18)` compares an integer exactly and a `"0.10"` bound compares a `Decimal` without rounding.

`Validator::validate_rule(&value, &rule, field)` checks one value against one rule and returns the failure message, or `None`. `rule.validate(&value)` does the same with a `Result<(), String>`, and `rule.message(field)` gives the rule's default message. `Validator::is_valid_email(text)` and `Validator::is_valid_url(text)` are the checks behind `email` and `url`.

```rust
use tideorm::prelude::*;

fn is_email(text: &str) -> bool {
    Validator::validate_rule(&text.to_string(), &ValidationRule::Email, "email").is_none()
}
```

`ValidationRule` lists every rule, including `In(Vec<String>)`, `NotIn(Vec<String>)` and the `MinExact`, `MaxExact` and `RangeExact` forms that keep a bound's exact decimal spelling. It is `#[non_exhaustive]`, so match it with a wildcard arm.

For a type that is not a model, such as a request body, implement `Validate` yourself. `ValidationErrors::check(field, &value, &rules)` applies a field's rules and records each failure under its name:

```rust
use tideorm::prelude::*;

pub struct Signup {
    pub email: String,
    pub age: i32,
}

impl Validate for Signup {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        errors
            .check("email", &self.email, &[ValidationRule::Required, ValidationRule::Email])
            .check("age", &self.age, &[ValidationRule::MinExact("18".into())]);
        errors.to_result()
    }
}
```

Values are checked through the `ValidatableValue` trait, which TideORM implements for strings, numbers, `Decimal`, `Option`s, `Vec`s, JSON values and the other common field types.
