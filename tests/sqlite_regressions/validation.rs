//! Numeric validation bounds compare exactly.

use rust_decimal::Decimal;
use tideorm::prelude::*;

#[tideorm::model(table = "audit_bounds")]
struct Bounds {
    #[tideorm(primary_key)]
    id: i64,
    #[validate(min = 9007199254740993, max = 9007199254740993)]
    exact: i64,
    #[validate(range(9007199254740993, 9007199254740993))]
    range: i64,
    #[validate(max = 1)]
    upper: Decimal,
    #[validate(min = 1)]
    lower: Option<Decimal>,
}

#[test]
fn numeric_validation_preserves_integer_bounds_and_decimal_values() {
    use tideorm::validation::{Validate, ValidationRule};
    let mut row = Bounds {
        id: 1,
        exact: 9007199254740993,
        range: 9007199254740993,
        upper: Decimal::ONE,
        lower: Some(Decimal::ONE),
    };
    row.validate().unwrap();
    row.exact -= 1;
    assert!(row.validate().is_err());
    row.exact += 1;
    row.range += 1;
    assert!(row.validate().is_err());
    row.range -= 1;
    row.upper = "1.0000000000000000001".parse().unwrap();
    assert!(row.validate().is_err());
    row.upper = Decimal::ONE;
    row.lower = Some("0.9999999999999999999".parse().unwrap());
    assert!(row.validate().is_err());
    assert!(
        ValidationRule::Max(1.0)
            .validate(&"1.0000000000000000001".parse::<Decimal>().unwrap())
            .is_err()
    );
    assert!(
        ValidationRule::Min(1.0)
            .validate(&"0.9999999999999999999".parse::<Decimal>().unwrap())
            .is_err()
    );
}

#[tideorm::model(table = "resume_exact_bounds")]
struct ExactBounds {
    #[tideorm(primary_key)]
    id: i64,
    #[validate(min = "9007199254740992.5")]
    lower: i64,
    #[validate(max = "9007199254740992.5")]
    upper: i64,
    #[validate(range("1e-2", "2e0"))]
    scientific: Decimal,
    #[validate(min = "1.00000000000000000000000000001")]
    precise: Decimal,
}

#[test]
fn exact_validation_compares_fractional_scientific_and_over_precision_bounds() {
    use tideorm::validation::{ValidationRule, Validator};
    let valid = ExactBounds {
        id: 1,
        lower: 9007199254740993,
        upper: 9007199254740992,
        scientific: Decimal::ONE,
        precise: Decimal::new(11, 1),
    };
    assert!(valid.validate().is_ok());
    let invalid = ExactBounds {
        lower: 9007199254740992,
        upper: 9007199254740993,
        scientific: Decimal::new(1, 3),
        precise: Decimal::ONE,
        ..valid
    };
    let errors = invalid.validate().unwrap_err();
    for field in ["lower", "upper", "scientific", "precise"] {
        assert!(errors.get(field).is_some(), "{field}");
    }
    // The fields above hold the positive bound; a negative one compares alike.
    let (bound, below, above) = (
        "-9007199254740992.5",
        -9007199254740993_i64,
        -9007199254740992,
    );
    for (value, rule, fails) in [
        (below, ValidationRule::MinExact(bound.into()), true),
        (above, ValidationRule::MinExact(bound.into()), false),
        (below, ValidationRule::MaxExact(bound.into()), false),
        (above, ValidationRule::MaxExact(bound.into()), true),
    ] {
        assert_eq!(
            Validator::validate_rule(&value, &rule, "n").is_some(),
            fails,
            "{value} against {rule:?}"
        );
    }
    assert!(
        Validator::validate_rule(&Decimal::ONE, &ValidationRule::MaxExact("1e-2".into()), "n")
            .is_some()
    );
    assert!(
        Validator::validate_rule(
            &Decimal::ONE,
            &ValidationRule::MaxExact("0.99999999999999999999999999999".into()),
            "n"
        )
        .is_some()
    );
}
