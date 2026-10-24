use tideorm::error::Error;
use tideorm::validation::{ValidationErrors, ValidationRule};

/// Each built-in rule applied to text: the values it accepts and the ones it
/// rejects.
#[test]
fn rules_accept_and_reject_text_values() {
    let cases: [(ValidationRule, &[&str], &[&str]); 14] = [
        (ValidationRule::Required, &["hello"], &["", "   "]),
        (
            ValidationRule::Email,
            &["test@example.com", "user.name+tag@domain.co.uk"],
            &["invalid", "@nodomain.com", "noat.com", "test@"],
        ),
        (
            ValidationRule::MinLength(5),
            &["hello", "hello world"],
            &["hi", ""],
        ),
        (
            ValidationRule::MaxLength(10),
            &["hello", ""],
            &["hello world!"],
        ),
        (ValidationRule::Min(18.0), &["18", "25"], &["17", "0"]),
        (ValidationRule::Max(100.0), &["50", "100"], &["101"]),
        (
            ValidationRule::Range(1.0, 100.0),
            &["1", "50", "100"],
            &["0", "101"],
        ),
        (
            ValidationRule::Regex(r"^\d{3}-\d{4}$".to_string()),
            &["123-4567"],
            &["1234567", "abc-defg"],
        ),
        (
            ValidationRule::Alpha,
            &["hello", "HelloWorld"],
            &["hello123", "hello world"],
        ),
        (
            ValidationRule::Alphanumeric,
            &["hello123", "ABC123"],
            &["hello world", "hello-world"],
        ),
        (
            ValidationRule::Numeric,
            &["12345", "0", "12.34", "-123"],
            &["abc", "12abc"],
        ),
        (
            ValidationRule::Uuid,
            &[
                "550e8400-e29b-41d4-a716-446655440000",
                "550E8400-E29B-41D4-A716-446655440000",
            ],
            &["not-a-uuid", "550e8400-e29b-41d4-a716"],
        ),
        (
            ValidationRule::In(vec![
                "red".to_string(),
                "green".to_string(),
                "blue".to_string(),
            ]),
            &["red", "green"],
            &["yellow"],
        ),
        (
            ValidationRule::NotIn(vec!["admin".to_string(), "root".to_string()]),
            &["user", "guest"],
            &["admin", "root"],
        ),
    ];

    for (rule, accepted, rejected) in cases {
        for value in accepted {
            assert!(
                rule.validate(&value.to_string()).is_ok(),
                "{rule:?} should accept {value:?}"
            );
        }
        for value in rejected {
            assert!(
                rule.validate(&value.to_string()).is_err(),
                "{rule:?} should reject {value:?}"
            );
        }
    }
}

#[test]
fn test_validation_error_messages() {
    assert_eq!(
        ValidationRule::MinLength(5).validate(&"hi".to_string()),
        Err("The field must be at least 5 characters".to_string())
    );
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

    assert_eq!(
        errors.to_string(),
        "email: Invalid email; password: Too short"
    );
}

#[test]
fn test_validation_errors_convert_to_validation_error() {
    let mut errors = ValidationErrors::new();
    errors.add("email", "is invalid");

    let error: Error = errors.into();
    assert!(
        matches!(&error, Error::Validation { field, message } if field == "email" && message == "is invalid"),
        "unexpected conversion: {error:?}"
    );
}
