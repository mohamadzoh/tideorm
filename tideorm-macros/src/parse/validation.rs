use super::*;
use syn::{Attribute, Meta};

pub(crate) fn parse_validation_attributes(
    field_name: &str,
    field: &ModelField,
) -> syn::Result<Vec<TokenStream2>> {
    let mut rules = Vec::new();
    for attr in &field.attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }
        if let Meta::List(list) = &attr.meta {
            for part in list.tokens.to_string().split(',').map(str::trim) {
                match part {
                    "required" => {
                        rules.push(quote!(::tideorm::validation::ValidationRule::Required))
                    }
                    "email" => {
                        ensure_validation_compatibility(field_name, field, attr, "email")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Email));
                    }
                    "url" => {
                        ensure_validation_compatibility(field_name, field, attr, "url")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Url));
                    }
                    "alpha" => {
                        ensure_validation_compatibility(field_name, field, attr, "alpha")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Alpha));
                    }
                    "alphanumeric" => {
                        ensure_validation_compatibility(field_name, field, attr, "alphanumeric")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Alphanumeric))
                    }
                    "numeric" => {
                        ensure_validation_compatibility(field_name, field, attr, "numeric")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Numeric))
                    }
                    "uuid" => {
                        ensure_validation_compatibility(field_name, field, attr, "uuid")?;
                        rules.push(quote!(::tideorm::validation::ValidationRule::Uuid));
                    }
                    _ if part.starts_with("min_length") => {
                        ensure_validation_compatibility(field_name, field, attr, "min_length")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "min_length",
                            |n: usize| quote!(::tideorm::validation::ValidationRule::MinLength(#n)),
                        )
                    }
                    _ if part.starts_with("max_length") => {
                        ensure_validation_compatibility(field_name, field, attr, "max_length")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "max_length",
                            |n: usize| quote!(::tideorm::validation::ValidationRule::MaxLength(#n)),
                        )
                    }
                    _ if part.starts_with("length")
                        && !part.contains("min_")
                        && !part.contains("max_") =>
                    {
                        ensure_validation_compatibility(field_name, field, attr, "length")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "length",
                            |n: usize| quote!(::tideorm::validation::ValidationRule::Length(#n)),
                        )
                    }
                    _ if part.starts_with("min") && !part.contains("length") => {
                        ensure_validation_compatibility(field_name, field, attr, "min")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "min",
                            |n: f64| quote!(::tideorm::validation::ValidationRule::Min(#n)),
                        )
                    }
                    _ if part.starts_with("max") && !part.contains("length") => {
                        ensure_validation_compatibility(field_name, field, attr, "max")?;
                        push_parsed_rule(
                            &mut rules,
                            part,
                            "max",
                            |n: f64| quote!(::tideorm::validation::ValidationRule::Max(#n)),
                        )
                    }
                    _ if part.starts_with("range") => {
                        ensure_validation_compatibility(field_name, field, attr, "range")?;
                        if let Some(value) = extract_value(part, "range") {
                            let parts: Vec<_> = value.trim_matches('"').split("..").collect();
                            if parts.len() == 2 {
                                if let (Ok(min), Ok(max)) =
                                    (parts[0].parse::<f64>(), parts[1].parse::<f64>())
                                {
                                    rules.push(quote!(::tideorm::validation::ValidationRule::Range(#min, #max)));
                                }
                            }
                        }
                    }
                    _ if part.starts_with("regex") => {
                        ensure_validation_compatibility(field_name, field, attr, "regex")?;
                        if let Some(value) = extract_value(part, "regex") {
                            let pattern = value.trim_matches('"');
                            rules.push(quote!(::tideorm::validation::ValidationRule::Regex(#pattern.to_string())));
                        }
                    }
                    _ if part.starts_with("custom") => {
                        if let Some(value) = extract_value(part, "custom") {
                            let msg = value.trim_matches('"');
                            rules.push(quote!(::tideorm::validation::ValidationRule::Custom(#msg.to_string())));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(rules)
}

fn ensure_validation_compatibility(
    field_name: &str,
    field: &ModelField,
    attr: &Attribute,
    rule: &str,
) -> syn::Result<()> {
    let expects_string = matches!(
        rule,
        "email"
            | "url"
            | "alpha"
            | "alphanumeric"
            | "numeric"
            | "uuid"
            | "min_length"
            | "max_length"
            | "length"
            | "regex"
    );
    let expects_numeric = matches!(rule, "min" | "max" | "range");

    let compatible = if expects_string {
        field.supports_string_validations()
    } else if expects_numeric {
        field.supports_numeric_validations()
    } else {
        true
    };

    if compatible {
        return Ok(());
    }

    let expected = if expects_string {
        "a string field"
    } else if expects_numeric {
        "a numeric field or string field"
    } else {
        "a compatible field"
    };

    Err(syn::Error::new_spanned(
        attr,
        format!(
            "validation rule '{}' is incompatible with field '{}' of type '{}'; expected {}",
            rule,
            field_name,
            field.validation_base_type(),
            expected
        ),
    ))
}

fn push_parsed_rule<T, F>(rules: &mut Vec<TokenStream2>, input: &str, key: &str, build: F)
where
    T: std::str::FromStr,
    F: FnOnce(T) -> TokenStream2,
{
    if let Some(value) = extract_value(input, key) {
        if let Ok(parsed) = value.parse::<T>() {
            rules.push(build(parsed));
        }
    }
}

pub(crate) fn extract_value(input: &str, key: &str) -> Option<String> {
    let input = input.trim();
    if let Some(pos) = input.find('=') {
        let current = input[..pos].trim();
        if current == key {
            return Some(input[pos + 1..].trim().to_string());
        }
    }
    if let Some(inner) = input
        .strip_prefix(key)
        .map(str::trim_start)
        .and_then(|value| value.strip_prefix('('))
        .and_then(|value| value.strip_suffix(')'))
    {
        return Some(inner.trim().to_string());
    }
    None
}
