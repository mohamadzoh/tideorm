use super::*;

#[test]
fn test_email_validation() {
    assert!(Validator::is_valid_email("test@example.com"));
    assert!(Validator::is_valid_email("user.name+tag@domain.co.uk"));
    assert!(!Validator::is_valid_email("invalid"));
    assert!(!Validator::is_valid_email("@example.com"));
    assert!(!Validator::is_valid_email("test@"));
}

#[test]
fn test_url_validation() {
    assert!(Validator::is_valid_url("http://example.com"));
    assert!(Validator::is_valid_url("https://example.com/path?query=1"));
    assert!(!Validator::is_valid_url("example.com"));
    assert!(!Validator::is_valid_url("ftp://example.com"));
    assert!(!Validator::is_valid_url("https://"));
    assert!(!Validator::is_valid_url("https:// ; DROP TABLE users"));
}

#[test]
fn test_min_length() {
    let rule = ValidationRule::MinLength(3);
    assert!(Validator::validate_rule(&"ab".to_string(), &rule, "name").is_some());
    assert!(Validator::validate_rule(&"abc".to_string(), &rule, "name").is_none());
    assert!(Validator::validate_rule(&"abcd".to_string(), &rule, "name").is_none());
}

#[test]
fn test_max_length() {
    let rule = ValidationRule::MaxLength(5);
    assert!(Validator::validate_rule(&"abc".to_string(), &rule, "name").is_none());
    assert!(Validator::validate_rule(&"abcde".to_string(), &rule, "name").is_none());
    assert!(Validator::validate_rule(&"abcdef".to_string(), &rule, "name").is_some());
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
fn test_range() {
    let rule = ValidationRule::Range(1.0, 10.0);
    assert!(Validator::validate_rule(&0, &rule, "age").is_some());
    assert!(Validator::validate_rule(&1, &rule, "age").is_none());
    assert!(Validator::validate_rule(&5, &rule, "age").is_none());
    assert!(Validator::validate_rule(&10, &rule, "age").is_none());
    assert!(Validator::validate_rule(&11, &rule, "age").is_some());
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
fn test_regex_validation() {
    let rule = ValidationRule::Regex(r"^[a-z]+$".to_string());

    assert!(Validator::validate_rule(&"alice".to_string(), &rule, "name").is_none());
    assert!(Validator::validate_rule(&"Alice1".to_string(), &rule, "name").is_some());
    assert!(Validator::validate_rule(&"bob".to_string(), &rule, "name").is_none());
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
