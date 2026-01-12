//! # TideORM Validation Example
//!
//! **Category:** Data Validation
//!
//! This example demonstrates TideORM's built-in validation system for models.
//! Validation rules can be applied using attributes or programmatically.
//!
//! ## Run this example
//!
//! ```bash
//! cargo run --example validation_demo
//! ```

use tideorm::prelude::*;
use tideorm::validation::{
    ValidationRule, Validator, ValidationBuilder, ValidationErrors,
    ValidatableValue,
};

// =============================================================================
// MODEL DEFINITION WITH VALIDATION ATTRIBUTES
// =============================================================================

/// User model with validation rules defined via attributes
/// 
/// Note: When using the #[validate(...)] attribute, validation is automatic
/// when calling user.validate() method.
#[tideorm::model]
#[tide(table = "users")]
pub struct User {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    
    // Email validation: required and must be valid email format
    // #[validate(required, email)]
    pub email: String,
    
    // Username: required, 3-20 chars, alphanumeric only
    // #[validate(required, min_length = 3, max_length = 20, alphanumeric)]
    pub username: String,
    
    // Password: required, minimum 8 characters
    // #[validate(required, min_length = 8)]
    pub password: String,
    
    // Age: optional, but if present must be 18-120
    // #[validate(min = 18, max = 120)]
    pub age: Option<i32>,
    
    // Website: optional, but if present must be valid URL
    // #[validate(url)]
    pub website: Option<String>,
    
    // Status: must be one of the allowed values
    // #[validate(in = "active,pending,inactive")]
    pub status: String,
}

// =============================================================================
// BASIC VALIDATION EXAMPLES
// =============================================================================

fn basic_validation_rules() {
    println!("\n=== Basic Validation Rules ===\n");
    
    // Required validation
    let required = ValidationRule::Required;
    println!("Required rule:");
    println!("  'hello' -> {:?}", required.validate(&"hello".to_string()));
    println!("  '' -> {:?}", required.validate(&"".to_string()));
    println!("  '   ' -> {:?}", required.validate(&"   ".to_string()));
    
    // Email validation
    let email = ValidationRule::Email;
    println!("\nEmail rule:");
    println!("  'user@example.com' -> {:?}", email.validate(&"user@example.com".to_string()));
    println!("  'invalid-email' -> {:?}", email.validate(&"invalid-email".to_string()));
    println!("  'user.name+tag@domain.co.uk' -> {:?}", email.validate(&"user.name+tag@domain.co.uk".to_string()));
    
    // URL validation
    let url = ValidationRule::Url;
    println!("\nURL rule:");
    println!("  'https://example.com' -> {:?}", url.validate(&"https://example.com".to_string()));
    println!("  'not-a-url' -> {:?}", url.validate(&"not-a-url".to_string()));
    
    // Length validations
    let min_length = ValidationRule::MinLength(5);
    let max_length = ValidationRule::MaxLength(10);
    println!("\nLength rules (min=5, max=10):");
    println!("  'hi' -> min: {:?}", min_length.validate(&"hi".to_string()));
    println!("  'hello' -> min: {:?}", min_length.validate(&"hello".to_string()));
    println!("  'hello world!!' -> max: {:?}", max_length.validate(&"hello world!!".to_string()));
    
    // Numeric range validations
    let min = ValidationRule::Min(18.0);
    let max = ValidationRule::Max(65.0);
    let range = ValidationRule::Range(18.0, 65.0);
    println!("\nNumeric rules (min=18, max=65):");
    println!("  '17' -> min: {:?}", min.validate(&"17".to_string()));
    println!("  '25' -> min: {:?}", min.validate(&"25".to_string()));
    println!("  '70' -> max: {:?}", max.validate(&"70".to_string()));
    println!("  '30' -> range: {:?}", range.validate(&"30".to_string()));
    
    // Regex validation
    let phone_regex = ValidationRule::Regex(r"^\+?[\d\s-]{10,}$".to_string());
    println!("\nRegex rule (phone number):");
    println!("  '+1-555-123-4567' -> {:?}", phone_regex.validate(&"+1-555-123-4567".to_string()));
    println!("  '123' -> {:?}", phone_regex.validate(&"123".to_string()));
    
    // Character class validations
    let alpha = ValidationRule::Alpha;
    let alphanumeric = ValidationRule::Alphanumeric;
    let numeric = ValidationRule::Numeric;
    println!("\nCharacter class rules:");
    println!("  'Hello' -> alpha: {:?}", alpha.validate(&"Hello".to_string()));
    println!("  'Hello123' -> alpha: {:?}", alpha.validate(&"Hello123".to_string()));
    println!("  'Hello123' -> alphanumeric: {:?}", alphanumeric.validate(&"Hello123".to_string()));
    println!("  '12345' -> numeric: {:?}", numeric.validate(&"12345".to_string()));
    
    // UUID validation
    let uuid = ValidationRule::Uuid;
    println!("\nUUID rule:");
    println!("  '550e8400-e29b-41d4-a716-446655440000' -> {:?}", 
        uuid.validate(&"550e8400-e29b-41d4-a716-446655440000".to_string()));
    println!("  'not-a-uuid' -> {:?}", uuid.validate(&"not-a-uuid".to_string()));
    
    // In/NotIn validations
    let in_list = ValidationRule::In(vec!["red".to_string(), "green".to_string(), "blue".to_string()]);
    let not_in = ValidationRule::NotIn(vec!["admin".to_string(), "root".to_string()]);
    println!("\nIn/NotIn rules:");
    println!("  'red' in [red,green,blue] -> {:?}", in_list.validate(&"red".to_string()));
    println!("  'yellow' in [red,green,blue] -> {:?}", in_list.validate(&"yellow".to_string()));
    println!("  'user' not in [admin,root] -> {:?}", not_in.validate(&"user".to_string()));
    println!("  'admin' not in [admin,root] -> {:?}", not_in.validate(&"admin".to_string()));
}

// =============================================================================
// VALIDATOR EXAMPLES
// =============================================================================

fn validator_examples() {
    println!("\n=== Validator Examples ===\n");
    
    // Using Validator static methods
    println!("Validator static methods:");
    println!("  is_valid_email('user@example.com'): {}", Validator::is_valid_email("user@example.com"));
    println!("  is_valid_email('invalid'): {}", Validator::is_valid_email("invalid"));
    println!("  is_valid_url('https://example.com'): {}", Validator::is_valid_url("https://example.com"));
    println!("  is_valid_url('not-a-url'): {}", Validator::is_valid_url("not-a-url"));
    
    // Using Validator::validate_rule for custom validation
    println!("\nUsing Validator::validate_rule:");
    
    let email_rule = ValidationRule::Email;
    let result = Validator::validate_rule(&"test@example.com".to_string(), &email_rule, "email");
    println!("  Email 'test@example.com': {:?}", result); // None = no error
    
    let result = Validator::validate_rule(&"invalid".to_string(), &email_rule, "email");
    println!("  Email 'invalid': {:?}", result); // Some(error message)
    
    // Validate multiple fields programmatically
    println!("\nValidating a user form:");
    let fields = vec![
        ("email", "john@example.com".to_string(), ValidationRule::Email),
        ("username", "johndoe".to_string(), ValidationRule::MinLength(3)),
        ("age", "25".to_string(), ValidationRule::Min(18.0)),
    ];
    
    let mut errors = ValidationErrors::new();
    for (field, value, rule) in &fields {
        if let Some(err) = Validator::validate_rule(value, rule, field) {
            errors.add(*field, err);
        }
    }
    
    if errors.is_empty() {
        println!("  ✓ All fields valid!");
    } else {
        println!("  ✗ Errors: {}", errors);
    }
}

// =============================================================================
// VALIDATION BUILDER EXAMPLES
// =============================================================================

fn validation_builder_examples() {
    println!("\n=== ValidationBuilder Examples ===\n");
    
    // Build validation rules for a field using the fluent API
    println!("Building validation rules for 'username':");
    let (field, rules) = ValidationBuilder::new("username")
        .required()
        .min_length(3)
        .max_length(20)
        .alphanumeric()
        .build();
    
    println!("  Field: {}", field);
    println!("  Rules count: {}", rules.len());
    
    // Validate a value against all rules
    let test_values = vec![
        "ab",         // Too short
        "validuser",  // Valid
        "user_name",  // Invalid (underscore)
        "averylongusernamethatexceedslimit", // Too long
    ];
    
    for value in test_values {
        let mut value_errors = ValidationErrors::new();
        for rule in &rules {
            if let Err(err) = rule.validate(&value.to_string()) {
                value_errors.add(&field, err);
            }
        }
        
        if value_errors.is_empty() {
            println!("  '{}' -> ✓ Valid", value);
        } else {
            println!("  '{}' -> ✗ {}", value, value_errors);
        }
    }
    
    // Build rules for multiple fields
    println!("\nBuilding rules for multiple fields:");
    
    let email_rules = ValidationBuilder::new("email")
        .required()
        .email()
        .build();
    println!("  email: {} rules", email_rules.1.len());
    
    let password_rules = ValidationBuilder::new("password")
        .required()
        .min_length(8)
        .build();
    println!("  password: {} rules", password_rules.1.len());
    
    let age_rules = ValidationBuilder::new("age")
        .min(18.0)
        .max(120.0)
        .build();
    println!("  age: {} rules", age_rules.1.len());
}

// =============================================================================
// VALIDATION ERRORS EXAMPLES
// =============================================================================

fn validation_errors_examples() {
    println!("\n=== ValidationErrors Examples ===\n");
    
    // Create and populate validation errors
    let mut errors = ValidationErrors::new();
    
    println!("Initially: is_empty = {}", errors.is_empty());
    
    errors.add("email", "Email is required");
    errors.add("email", "Email format is invalid");
    errors.add("password", "Password must be at least 8 characters");
    errors.add("username", "Username is already taken");
    
    println!("After adding errors: is_empty = {}", errors.is_empty());
    println!("Total errors: {}", errors.errors().len());
    
    // Get errors for specific field
    println!("\nEmail errors:");
    for msg in errors.field_errors("email") {
        println!("  - {}", msg);
    }
    
    println!("\nPassword errors:");
    for msg in errors.field_errors("password") {
        println!("  - {}", msg);
    }
    
    // Display all errors
    println!("\nAll errors (formatted):");
    println!("{}", errors);
    
    // Convert to TideORM Error
    let tide_error: tideorm::error::Error = errors.into();
    println!("\nAs TideORM Error: {}", tide_error);
}

// =============================================================================
// VALIDATABLE VALUE TRAIT EXAMPLES
// =============================================================================

fn validatable_value_examples() {
    println!("\n=== ValidatableValue Trait Examples ===\n");
    
    // Different types implementing ValidatableValue
    let string_val = "hello".to_string();
    let int_val: i32 = 42;
    let float_val: f64 = 3.14159;
    let option_some: Option<String> = Some("test".to_string());
    let option_none: Option<String> = None;
    
    // String
    println!("String '{}' -> is_empty: {}, as_str: {:?}", 
        string_val, 
        string_val.is_empty_value(),
        string_val.as_str_value());
    
    // Integers
    println!("i32 {} -> is_empty: {}, as_f64: {:?}", 
        int_val, 
        int_val.is_empty_value(),
        int_val.as_f64_value());
    
    // Floats
    println!("f64 {} -> is_empty: {}, as_f64: {:?}", 
        float_val, 
        float_val.is_empty_value(),
        float_val.as_f64_value());
    
    // Options
    println!("Some('test') -> is_empty: {}", option_some.is_empty_value());
    println!("None -> is_empty: {}", option_none.is_empty_value());
    
    // Using ValidatableValue in validation
    println!("\nValidating different types with Min(10) rule:");
    let min_rule = ValidationRule::Min(10.0);
    
    println!("  5 (i32): {:?}", min_rule.validate(&"5".to_string()));
    println!("  15 (i32): {:?}", min_rule.validate(&"15".to_string()));
    println!("  10.5 (f64): {:?}", min_rule.validate(&"10.5".to_string()));
}

// =============================================================================
// PRACTICAL EXAMPLE: FORM VALIDATION
// =============================================================================

fn form_validation_example() {
    println!("\n=== Practical Example: Form Validation ===\n");
    
    // Simulate form submission data
    struct RegistrationForm {
        email: String,
        username: String,
        password: String,
        password_confirmation: String,
        terms_accepted: bool,
    }
    
    fn validate_registration(form: &RegistrationForm) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        
        // Email validation
        if form.email.is_empty() {
            errors.add("email", "Email is required");
        } else if ValidationRule::Email.validate(&form.email).is_err() {
            errors.add("email", "Please enter a valid email address");
        }
        
        // Username validation
        if form.username.is_empty() {
            errors.add("username", "Username is required");
        } else {
            if form.username.len() < 3 {
                errors.add("username", "Username must be at least 3 characters");
            }
            if form.username.len() > 20 {
                errors.add("username", "Username cannot exceed 20 characters");
            }
            if ValidationRule::Alphanumeric.validate(&form.username).is_err() {
                errors.add("username", "Username can only contain letters and numbers");
            }
        }
        
        // Password validation
        if form.password.is_empty() {
            errors.add("password", "Password is required");
        } else if form.password.len() < 8 {
            errors.add("password", "Password must be at least 8 characters");
        }
        
        // Password confirmation
        if form.password != form.password_confirmation {
            errors.add("password_confirmation", "Passwords do not match");
        }
        
        // Terms acceptance
        if !form.terms_accepted {
            errors.add("terms_accepted", "You must accept the terms and conditions");
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    
    // Test with invalid form
    let invalid_form = RegistrationForm {
        email: "invalid".to_string(),
        username: "ab".to_string(),
        password: "123".to_string(),
        password_confirmation: "456".to_string(),
        terms_accepted: false,
    };
    
    println!("Validating invalid registration form:");
    match validate_registration(&invalid_form) {
        Ok(_) => println!("  ✓ Form is valid"),
        Err(errors) => {
            println!("  ✗ Form has errors:");
            for (field, msg) in errors.errors() {
                println!("    {}: {}", field, msg);
            }
        }
    }
    
    // Test with valid form
    let valid_form = RegistrationForm {
        email: "john@example.com".to_string(),
        username: "johndoe123".to_string(),
        password: "SecurePass123".to_string(),
        password_confirmation: "SecurePass123".to_string(),
        terms_accepted: true,
    };
    
    println!("\nValidating valid registration form:");
    match validate_registration(&valid_form) {
        Ok(_) => println!("  ✓ Form is valid! Ready to create user."),
        Err(errors) => {
            println!("  ✗ Form has errors:");
            for (field, msg) in errors.errors() {
                println!("    {}: {}", field, msg);
            }
        }
    }
}

// =============================================================================
// MAIN
// =============================================================================

fn main() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║            TideORM Validation System Demo                      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    
    basic_validation_rules();
    validator_examples();
    validation_builder_examples();
    validation_errors_examples();
    validatable_value_examples();
    form_validation_example();
    
    println!("\n✓ Validation demo complete!");
}
