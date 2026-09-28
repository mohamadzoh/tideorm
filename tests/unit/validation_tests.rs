use super::*;

#[test]
fn test_url_validation() {
    assert!(Validator::is_valid_url("http://example.com"));
    assert!(Validator::is_valid_url("https://example.com/path?query=1"));
    assert!(Validator::is_valid_url("http://localhost:8080/path"));
    assert!(!Validator::is_valid_url("example.com"));
    assert!(!Validator::is_valid_url("ftp://example.com"));
    assert!(!Validator::is_valid_url("https://"));
    assert!(!Validator::is_valid_url("https:// ; DROP TABLE users"));
}

#[test]
fn test_length_rules_count_unicode_characters() {
    let value = "مرحبا".to_string();

    assert!(Validator::validate_rule(&value, &ValidationRule::MinLength(5), "name").is_none());
    assert!(Validator::validate_rule(&value, &ValidationRule::MinLength(6), "name").is_some());
    assert!(Validator::validate_rule(&value, &ValidationRule::MaxLength(5), "name").is_none());
    assert!(Validator::validate_rule(&value, &ValidationRule::MaxLength(4), "name").is_some());
    assert!(Validator::validate_rule(&value, &ValidationRule::Length(5), "name").is_none());
    assert!(Validator::validate_rule(&value, &ValidationRule::Length(10), "name").is_some());
}

#[test]
fn numeric_rules_refuse_nan() {
    for rule in [
        ValidationRule::Min(0.0),
        ValidationRule::Max(10.0),
        ValidationRule::Range(0.0, 10.0),
    ] {
        assert!(
            Validator::validate_rule(&f64::NAN, &rule, "score").is_some(),
            "{rule:?}"
        );
        assert!(
            Validator::validate_rule(&5.0, &rule, "score").is_none(),
            "{rule:?}"
        );
    }
}

#[test]
fn test_invalid_regex_rule_fails_with_an_error_naming_the_pattern() {
    let rule = ValidationRule::Regex("[".to_string());

    for value in ["alice", "bob", ""] {
        let error = Validator::validate_rule(&value.to_string(), &rule, "name")
            .expect("an invalid pattern must not accept any value");
        assert!(
            error.contains("`[`") && error.contains("not a valid regular expression"),
            "unexpected error: {error}"
        );
    }

    assert!(
        Validator::validate_rule(&None::<String>, &rule, "name").is_some(),
        "an invalid pattern is reported even when the value is absent"
    );
}

mod derived_model_validation {
    use crate::validation::Validate;

    #[tideorm::model(table = "validation_invalid_pattern_users")]
    struct InvalidPatternUser {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        #[validate(regex = "[")]
        code: String,
    }

    #[test]
    fn an_invalid_regex_attribute_rejects_the_model_instead_of_accepting_it() {
        let user = InvalidPatternUser {
            id: 0,
            code: "anything".to_string(),
        };

        let errors = user
            .validate()
            .expect_err("an invalid pattern must fail validation");
        let messages = errors.field_errors("code");
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("`[`"), "{messages:?}");
    }
}

/// A `skip` field is never stored but its rules still run, so a model that is
/// loaded, and holds `None` there, still passes.
mod skip_field_validation {
    use crate::validation::Validate;

    #[tideorm::model(table = "validation_signups")]
    struct Signup {
        #[tideorm(primary_key, auto_increment)]
        id: i64,
        #[tideorm(skip)]
        #[validate(min_length = 8)]
        password: Option<String>,
    }

    fn signup(password: Option<&str>) -> Signup {
        Signup {
            id: 0,
            password: password.map(str::to_string),
        }
    }

    #[test]
    fn a_rule_on_a_skip_field_runs() {
        let errors = signup(Some("short"))
            .validate()
            .expect_err("a short password fails");
        assert_eq!(errors.field_errors("password").len(), 1, "{errors:?}");
        assert!(signup(Some("long enough")).validate().is_ok());
        assert!(signup(None).validate().is_ok());
    }
}

#[test]
fn test_validation_errors() {
    let mut errors = ValidationErrors::new();
    assert!(errors.is_empty());

    errors.add("email", "Invalid email");
    errors.add("email", "Email already taken");
    errors.add("name", "Name is required");

    assert!(!errors.is_empty());
    assert_eq!(errors.len(), 2);
    assert_eq!(errors.get("email").unwrap().len(), 2);
    assert_eq!(errors.get("name").unwrap().len(), 1);

    let messages = errors.messages();
    assert_eq!(messages.len(), 3);
}

#[test]
fn test_validation_builder() {
    let (field, rules) = ValidationBuilder::new("email")
        .required()
        .email()
        .max_length(255)
        .build();

    assert_eq!(field, "email");
    assert_eq!(rules.len(), 3);
}

/// A module that imports TideORM's one-parameter `Result`, as `use tideorm::*;`
/// does, still compiles the `validate` its models get.
mod models_beside_the_crate_result {
    use crate::Result;
    use crate::validation::Validate;

    #[tideorm::model(table = "result_alias_rows")]
    pub struct ResultAliasRow {
        #[tideorm(primary_key, auto_increment)]
        pub id: i64,
        #[validate(min_length = 2)]
        pub name: String,
    }

    pub fn validates(name: &str) -> Result<bool> {
        let row = ResultAliasRow {
            id: 1,
            name: name.to_string(),
        };
        Ok(row.validate().is_ok())
    }
}

#[test]
fn a_model_beside_the_crate_result_alias_validates() {
    assert!(models_beside_the_crate_result::validates("ok").unwrap());
    assert!(!models_beside_the_crate_result::validates("x").unwrap());
}

#[test]
fn integer_bounds_are_compared_exactly_past_two_to_the_53() {
    let max = ValidationRule::Max(9_007_199_254_740_992.0);
    let one_past: i64 = 9_007_199_254_740_993;
    assert!(Validator::validate_rule(&one_past, &max, "id").is_some());
    assert!(Validator::validate_rule(&9_007_199_254_740_992_i64, &max, "id").is_none());

    let min = ValidationRule::Min(9_007_199_254_740_992.0);
    assert!(Validator::validate_rule(&9_007_199_254_740_991_i64, &min, "id").is_some());
    assert!(Validator::validate_rule(&Some(u64::MAX), &ValidationRule::Max(1e30), "id").is_none());

    // A fractional bound still admits the integers on its side.
    assert!(Validator::validate_rule(&3_i32, &ValidationRule::Range(2.5, 3.5), "n").is_none());
    assert!(Validator::validate_rule(&4_i32, &ValidationRule::Range(2.5, 3.5), "n").is_some());
    assert!(Validator::validate_rule(&f64::NAN, &ValidationRule::Min(0.0), "n").is_some());
}

#[test]
fn f32_bounds_compare_at_the_precision_the_field_holds() {
    // `0.7_f32` is `0.699999988` as an `f64`, below a bound of `0.7`.
    assert!(Validator::validate_rule(&0.7_f32, &ValidationRule::Min(0.7), "score").is_none());
    assert!(Validator::validate_rule(&0.1_f32, &ValidationRule::Max(0.1), "score").is_none());
    assert!(
        Validator::validate_rule(&Some(0.7_f32), &ValidationRule::Range(0.7, 0.9), "score")
            .is_none()
    );
    assert!(Validator::validate_rule(&0.69_f32, &ValidationRule::Min(0.7), "score").is_some());
}

#[test]
fn numeric_refuses_text_that_parses_as_no_finite_number() {
    for text in ["NaN", "inf", "-Infinity", "1e999"] {
        assert!(
            Validator::validate_rule(&text.to_string(), &ValidationRule::Numeric, "amount")
                .is_some(),
            "{text}"
        );
    }
    assert!(
        Validator::validate_rule(&"1e3".to_string(), &ValidationRule::Numeric, "amount").is_none()
    );
}

#[test]
fn errors_come_out_in_the_order_the_fields_failed() {
    let mut errors = ValidationErrors::new();
    for field in ["zeta", "alpha", "mid", "alpha"] {
        errors.add(field, format!("{field} is wrong"));
    }
    let fields: Vec<&str> = errors.iter().map(|(field, _)| field.as_str()).collect();
    assert_eq!(fields, ["zeta", "alpha", "mid"]);
    assert_eq!(
        errors.first().map(|(field, _)| field.as_str()),
        Some("zeta")
    );
    assert_eq!(errors.messages()[0], "zeta: zeta is wrong");

    let mut merged = ValidationErrors::new();
    merged.add("beta", "b");
    merged.merge(errors);
    let fields: Vec<&str> = merged.iter().map(|(field, _)| field.as_str()).collect();
    assert_eq!(fields, ["beta", "zeta", "alpha", "mid"]);
}

#[test]
fn required_applies_to_the_common_field_types() {
    let rule = ValidationRule::Required;
    assert!(Validator::validate_rule(&None::<uuid::Uuid>, &rule, "owner").is_some());
    assert!(Validator::validate_rule(&Some(uuid::Uuid::nil()), &rule, "owner").is_none());
    assert!(Validator::validate_rule(&Vec::<i32>::new(), &rule, "tags").is_some());
    assert!(Validator::validate_rule(&serde_json::json!({}), &rule, "meta").is_some());
    assert!(Validator::validate_rule(&serde_json::json!({ "a": 1 }), &rule, "meta").is_none());
    assert!(
        Validator::validate_rule(&None::<chrono::DateTime<chrono::Utc>>, &rule, "at").is_some()
    );
    assert!(Validator::validate_rule(&Some(true), &rule, "flag").is_none());

    let price = rust_decimal::Decimal::new(150, 2);
    assert!(Validator::validate_rule(&price, &ValidationRule::Min(1.5), "price").is_none());
    assert!(Validator::validate_rule(&price, &ValidationRule::Min(2.0), "price").is_some());
}

/// Rules on `Text` and `Decimal` fields and a required `Option<Uuid>` compile,
/// and `min..=max` includes its upper bound.
#[tideorm::model(table = "validated_documents")]
struct ValidatedDocument {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(max_length = 5)]
    body: crate::types::Text,
    #[validate(min = 0)]
    price: rust_decimal::Decimal,
    #[validate(required)]
    owner: Option<uuid::Uuid>,
    #[validate(range(1..=5))]
    stars: i32,
}

#[test]
fn rules_on_text_decimal_and_uuid_fields_check_them() {
    let document = ValidatedDocument {
        id: 0,
        body: "short".into(),
        price: rust_decimal::Decimal::ZERO,
        owner: Some(uuid::Uuid::new_v4()),
        stars: 5,
    };
    assert!(document.validate().is_ok());

    let document = ValidatedDocument {
        body: "too long".into(),
        price: rust_decimal::Decimal::new(-1, 0),
        owner: None,
        stars: 6,
        ..document
    };
    let errors = document.validate().expect_err("every rule fails");
    let fields: Vec<&str> = errors.iter().map(|(field, _)| field.as_str()).collect();
    assert_eq!(fields, ["body", "price", "owner", "stars"]);
}
