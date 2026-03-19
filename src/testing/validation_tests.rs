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
fn test_range() {
    let rule = ValidationRule::Range(1.0, 10.0);
    assert!(Validator::validate_rule(&0, &rule, "age").is_some());
    assert!(Validator::validate_rule(&1, &rule, "age").is_none());
    assert!(Validator::validate_rule(&5, &rule, "age").is_none());
    assert!(Validator::validate_rule(&10, &rule, "age").is_none());
    assert!(Validator::validate_rule(&11, &rule, "age").is_some());
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
