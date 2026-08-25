// =============================================================================
// VALIDATION MODULE TESTS
// =============================================================================

use tideorm::validation::{ValidatableValue, ValidationErrors, ValidationRule, Validator};

#[test]
fn test_validation_rule_required() {
    let rule = ValidationRule::Required;
    assert!(rule.validate(&"hello".to_string()).is_ok());
    assert!(rule.validate(&"".to_string()).is_err());
    assert!(rule.validate(&"   ".to_string()).is_err());
}

#[test]
fn test_validation_rule_email() {
    let rule = ValidationRule::Email;
    assert!(rule.validate(&"test@example.com".to_string()).is_ok());
    assert!(
        rule.validate(&"user.name+tag@domain.co.uk".to_string())
            .is_ok()
    );
    assert!(rule.validate(&"invalid".to_string()).is_err());
    assert!(rule.validate(&"@nodomain.com".to_string()).is_err());
    assert!(rule.validate(&"noat.com".to_string()).is_err());
}

#[test]
fn test_validation_rule_url() {
    let rule = ValidationRule::Url;
    assert!(rule.validate(&"https://example.com".to_string()).is_ok());
    assert!(
        rule.validate(&"http://localhost:8080/path".to_string())
            .is_ok()
    );
    assert!(rule.validate(&"not-a-url".to_string()).is_err());
    assert!(rule.validate(&"example.com".to_string()).is_err());
}

#[test]
fn test_validation_rule_min_length() {
    let rule = ValidationRule::MinLength(5);
    assert!(rule.validate(&"hello".to_string()).is_ok());
    assert!(rule.validate(&"hello world".to_string()).is_ok());
    assert!(rule.validate(&"hi".to_string()).is_err());
    assert!(rule.validate(&"".to_string()).is_err());
}

#[test]
fn test_validation_rule_max_length() {
    let rule = ValidationRule::MaxLength(10);
    assert!(rule.validate(&"hello".to_string()).is_ok());
    assert!(rule.validate(&"".to_string()).is_ok());
    assert!(rule.validate(&"hello world!".to_string()).is_err());
}

#[test]
fn test_validation_rule_min() {
    let rule = ValidationRule::Min(18.0);
    assert!(rule.validate(&"18".to_string()).is_ok());
    assert!(rule.validate(&"25".to_string()).is_ok());
    assert!(rule.validate(&"17".to_string()).is_err());
    assert!(rule.validate(&"0".to_string()).is_err());
}

#[test]
fn test_validation_rule_max() {
    let rule = ValidationRule::Max(100.0);
    assert!(rule.validate(&"50".to_string()).is_ok());
    assert!(rule.validate(&"100".to_string()).is_ok());
    assert!(rule.validate(&"101".to_string()).is_err());
}

#[test]
fn test_validation_rule_range() {
    let rule = ValidationRule::Range(1.0, 100.0);
    assert!(rule.validate(&"1".to_string()).is_ok());
    assert!(rule.validate(&"50".to_string()).is_ok());
    assert!(rule.validate(&"100".to_string()).is_ok());
    assert!(rule.validate(&"0".to_string()).is_err());
    assert!(rule.validate(&"101".to_string()).is_err());
}

#[test]
fn test_validation_rule_regex() {
    let rule = ValidationRule::Regex(r"^\d{3}-\d{4}$".to_string());
    assert!(rule.validate(&"123-4567".to_string()).is_ok());
    assert!(rule.validate(&"1234567".to_string()).is_err());
    assert!(rule.validate(&"abc-defg".to_string()).is_err());
}

#[test]
fn test_validation_rule_alpha() {
    let rule = ValidationRule::Alpha;
    assert!(rule.validate(&"hello".to_string()).is_ok());
    assert!(rule.validate(&"HelloWorld".to_string()).is_ok());
    assert!(rule.validate(&"hello123".to_string()).is_err());
    assert!(rule.validate(&"hello world".to_string()).is_err());
}

#[test]
fn test_validation_rule_alphanumeric() {
    let rule = ValidationRule::Alphanumeric;
    assert!(rule.validate(&"hello123".to_string()).is_ok());
    assert!(rule.validate(&"ABC123".to_string()).is_ok());
    assert!(rule.validate(&"hello world".to_string()).is_err());
    assert!(rule.validate(&"hello-world".to_string()).is_err());
}

#[test]
fn test_validation_rule_numeric() {
    let rule = ValidationRule::Numeric;
    assert!(rule.validate(&"12345".to_string()).is_ok());
    assert!(rule.validate(&"0".to_string()).is_ok());
    assert!(rule.validate(&"12.34".to_string()).is_ok());
    assert!(rule.validate(&"-123".to_string()).is_ok());
    assert!(rule.validate(&"abc".to_string()).is_err());
    assert!(rule.validate(&"12abc".to_string()).is_err());
}

#[test]
fn test_validation_rule_uuid() {
    let rule = ValidationRule::Uuid;
    assert!(
        rule.validate(&"550e8400-e29b-41d4-a716-446655440000".to_string())
            .is_ok()
    );
    assert!(
        rule.validate(&"550E8400-E29B-41D4-A716-446655440000".to_string())
            .is_ok()
    );
    assert!(rule.validate(&"not-a-uuid".to_string()).is_err());
    assert!(
        rule.validate(&"550e8400-e29b-41d4-a716".to_string())
            .is_err()
    );
}

#[test]
fn test_validation_rule_in() {
    let rule = ValidationRule::In(vec![
        "red".to_string(),
        "green".to_string(),
        "blue".to_string(),
    ]);
    assert!(rule.validate(&"red".to_string()).is_ok());
    assert!(rule.validate(&"green".to_string()).is_ok());
    assert!(rule.validate(&"yellow".to_string()).is_err());
}

#[test]
fn test_validation_rule_not_in() {
    let rule = ValidationRule::NotIn(vec!["admin".to_string(), "root".to_string()]);
    assert!(rule.validate(&"user".to_string()).is_ok());
    assert!(rule.validate(&"guest".to_string()).is_ok());
    assert!(rule.validate(&"admin".to_string()).is_err());
    assert!(rule.validate(&"root".to_string()).is_err());
}

#[test]
fn test_validation_errors_collection() {
    let mut errors = ValidationErrors::new();
    assert!(errors.is_empty());

    errors.add("email", "Invalid email format");
    assert!(!errors.is_empty());
    assert!(errors.has_errors());

    errors.add("email", "Email already taken");
    errors.add("password", "Too short");

    let all_errors = errors.errors();
    assert_eq!(all_errors.len(), 3);
}

#[test]
fn test_validation_errors_field_errors() {
    let mut errors = ValidationErrors::new();
    errors.add("email", "Invalid format");
    errors.add("email", "Already taken");
    errors.add("name", "Required");

    let email_errors = errors.field_errors("email");
    assert_eq!(email_errors.len(), 2);

    let name_errors = errors.field_errors("name");
    assert_eq!(name_errors.len(), 1);

    let missing_errors = errors.field_errors("missing");
    assert_eq!(missing_errors.len(), 0);
}

#[test]
fn test_validation_errors_display() {
    let mut errors = ValidationErrors::new();
    errors.add("email", "Invalid email");
    errors.add("password", "Too short");

    let display = format!("{}", errors);
    assert!(
        display.contains("email")
            || display.contains("Invalid email")
            || display.contains("password")
            || display.contains("Too short")
    );
}

#[test]
fn test_validator_validate_rule() {
    let rule = ValidationRule::Email;
    let result = Validator::validate_rule(&"test@example.com".to_string(), &rule, "email");
    assert!(result.is_none());

    let result = Validator::validate_rule(&"invalid".to_string(), &rule, "email");
    assert!(result.is_some());
}

#[test]
fn test_validatable_value_string() {
    let value = "hello".to_string();
    assert!(!value.is_empty_value());
    assert_eq!(value.as_str_value(), Some("hello"));

    let empty = "".to_string();
    assert!(empty.is_empty_value());
}

#[test]
fn test_validatable_value_option() {
    let some_value: Option<String> = Some("test".to_string());
    assert!(!some_value.is_empty_value());

    let none_value: Option<String> = None;
    assert!(none_value.is_empty_value());
}

#[test]
fn test_validatable_value_numbers() {
    let int_val: i32 = 42;
    assert!(!int_val.is_empty_value());
    assert_eq!(int_val.as_f64_value(), Some(42.0));

    let float_val: f64 = 3.14;
    assert_eq!(float_val.as_f64_value(), Some(3.14));
}

#[test]
fn test_validation_error_messages() {
    let rule = ValidationRule::MinLength(5);
    let result = rule.validate(&"hi".to_string());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("5"));
}

#[test]
fn test_validation_errors_into_error() {
    let mut errors = ValidationErrors::new();
    errors.add("field1", "error1");
    errors.add("field2", "error2");

    let error: tideorm::error::Error = errors.into();
    assert!(error.is_validation_error());
}
