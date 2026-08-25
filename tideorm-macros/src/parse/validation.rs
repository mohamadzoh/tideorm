use super::*;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Lit, Meta, Token, UnOp};

/// Rule names accepted inside `#[validate(..)]`, reported in diagnostics.
const SUPPORTED_RULES: &str = "required, email, url, alpha, alphanumeric, numeric, uuid, \
     min_length, max_length, length, min, max, range, regex, custom";

pub(crate) fn parse_validation_attributes(
    field_name: &str,
    field: &ModelField,
) -> syn::Result<Vec<TokenStream2>> {
    let mut rules = Vec::new();

    for attr in &field.attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }

        if !matches!(&attr.meta, Meta::List(_)) {
            return Err(syn::Error::new_spanned(
                attr,
                format!(
                    "#[validate(..)] expects a parenthesized rule list; supported rules: {SUPPORTED_RULES}"
                ),
            ));
        }

        attr.parse_nested_meta(|meta| parse_rule(field_name, field, &meta, &mut rules))?;
    }

    Ok(rules)
}

fn parse_rule(
    field_name: &str,
    field: &ModelField,
    meta: &ParseNestedMeta,
    rules: &mut Vec<TokenStream2>,
) -> syn::Result<()> {
    let rule_ident = meta.path.get_ident().cloned().ok_or_else(|| {
        syn::Error::new_spanned(
            &meta.path,
            format!("unknown validation rule; supported rules: {SUPPORTED_RULES}"),
        )
    })?;
    let rule = unraw_ident(&rule_ident);
    ensure_validation_compatibility(field_name, field, &rule_ident, &rule)?;

    let tokens = match rule.as_str() {
        "required" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Required)
        }
        "email" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Email)
        }
        "url" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Url)
        }
        "alpha" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Alpha)
        }
        "alphanumeric" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Alphanumeric)
        }
        "numeric" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Numeric)
        }
        "uuid" => {
            expect_flag(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Uuid)
        }
        "min_length" => {
            let value = parse_usize_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::MinLength(#value))
        }
        "max_length" => {
            let value = parse_usize_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::MaxLength(#value))
        }
        "length" => {
            let value = parse_usize_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Length(#value))
        }
        "min" => {
            let value = parse_f64_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Min(#value))
        }
        "max" => {
            let value = parse_f64_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Max(#value))
        }
        "range" => {
            let (min, max) = parse_range_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Range(#min, #max))
        }
        "regex" => {
            let pattern = parse_string_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Regex(#pattern.to_string()))
        }
        "custom" => {
            let message = parse_string_rule(meta, &rule_ident)?;
            quote!(::tideorm::validation::ValidationRule::Custom(#message.to_string()))
        }
        unknown => {
            return Err(syn::Error::new_spanned(
                &rule_ident,
                format!("unknown validation rule '{unknown}'; supported rules: {SUPPORTED_RULES}"),
            ));
        }
    };

    rules.push(tokens);
    Ok(())
}

fn ensure_validation_compatibility(
    field_name: &str,
    field: &ModelField,
    rule_ident: &Ident,
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
        rule_ident,
        format!(
            "validation rule '{}' is incompatible with field '{}' of type '{}'; expected {}",
            rule,
            field_name,
            field.validation_base_type(),
            expected
        ),
    ))
}

fn expect_flag(meta: &ParseNestedMeta, rule_ident: &Ident) -> syn::Result<()> {
    if meta.input.peek(Token![=]) || meta.input.peek(syn::token::Paren) {
        return Err(syn::Error::new_spanned(
            rule_ident,
            format!("validation rule '{rule_ident}' does not take a value"),
        ));
    }

    Ok(())
}

fn rule_values(meta: &ParseNestedMeta, rule_ident: &Ident) -> syn::Result<Vec<Expr>> {
    if meta.input.peek(Token![=]) {
        let value = meta.value()?;
        return Ok(vec![value.parse::<Expr>()?]);
    }

    if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let values = Punctuated::<Expr, Token![,]>::parse_terminated(&content)?;
        if values.is_empty() {
            return Err(missing_value_error(rule_ident));
        }

        return Ok(values.into_iter().collect());
    }

    Err(missing_value_error(rule_ident))
}

fn missing_value_error(rule_ident: &Ident) -> syn::Error {
    syn::Error::new_spanned(
        rule_ident,
        format!("validation rule '{rule_ident}' requires a value, e.g. `{rule_ident} = ...`"),
    )
}

fn single_value<'a>(values: &'a [Expr], rule_ident: &Ident) -> syn::Result<&'a Expr> {
    match values {
        [value] => Ok(value),
        _ => Err(syn::Error::new_spanned(
            rule_ident,
            format!("validation rule '{rule_ident}' takes exactly one value"),
        )),
    }
}

fn parse_usize_rule(meta: &ParseNestedMeta, rule_ident: &Ident) -> syn::Result<usize> {
    let values = rule_values(meta, rule_ident)?;
    let value = single_value(&values, rule_ident)?;
    expr_to_usize(value).ok_or_else(|| {
        syn::Error::new_spanned(
            value,
            format!(
                "validation rule '{rule_ident}' expects a non-negative integer, e.g. `{rule_ident} = 3`"
            ),
        )
    })
}

fn parse_f64_rule(meta: &ParseNestedMeta, rule_ident: &Ident) -> syn::Result<f64> {
    let values = rule_values(meta, rule_ident)?;
    let value = single_value(&values, rule_ident)?;
    expr_to_f64(value).ok_or_else(|| {
        syn::Error::new_spanned(
            value,
            format!("validation rule '{rule_ident}' expects a number, e.g. `{rule_ident} = 18`"),
        )
    })
}

fn parse_string_rule(meta: &ParseNestedMeta, rule_ident: &Ident) -> syn::Result<String> {
    let values = rule_values(meta, rule_ident)?;
    let value = single_value(&values, rule_ident)?;
    expr_to_string(value).ok_or_else(|| {
        syn::Error::new_spanned(
            value,
            format!(
                "validation rule '{rule_ident}' expects a string literal, e.g. `{rule_ident} = \"...\"`"
            ),
        )
    })
}

fn parse_range_rule(meta: &ParseNestedMeta, rule_ident: &Ident) -> syn::Result<(f64, f64)> {
    let values = rule_values(meta, rule_ident)?;

    // `range(min, max)`
    if let [first, second] = values.as_slice() {
        let bounds = (expr_to_f64(first), expr_to_f64(second));
        return match bounds {
            (Some(min), Some(max)) => Ok((min, max)),
            _ => Err(range_error(first, rule_ident)),
        };
    }

    let value = single_value(&values, rule_ident)?;

    // `range(min..max)`
    if let Expr::Range(range) = value {
        let min = range.start.as_deref().and_then(expr_to_f64);
        let max = range.end.as_deref().and_then(expr_to_f64);
        return match (min, max) {
            (Some(min), Some(max)) => Ok((min, max)),
            _ => Err(range_error(value, rule_ident)),
        };
    }

    // `range = "min..max"`
    let text = expr_to_string(value);
    match text.as_deref().and_then(parse_range_text) {
        Some(bounds) => Ok(bounds),
        None => Err(range_error(value, rule_ident)),
    }
}

fn parse_range_text(text: &str) -> Option<(f64, f64)> {
    let (min, max) = text.split_once("..")?;
    let min = min.trim().parse::<f64>().ok()?;
    let max = max.trim().parse::<f64>().ok()?;
    Some((min, max))
}

fn range_error(value: &Expr, rule_ident: &Ident) -> syn::Error {
    syn::Error::new_spanned(
        value,
        format!(
            "validation rule '{rule_ident}' expects `{rule_ident} = \"min..max\"` or `{rule_ident}(min, max)`"
        ),
    )
}

/// Splits an attribute value into its literal and whether it was negated.
fn expr_literal(expr: &Expr) -> Option<(bool, &Lit)> {
    match expr {
        Expr::Lit(ExprLit { lit, .. }) => Some((false, lit)),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => match unary.expr.as_ref() {
            Expr::Lit(ExprLit { lit, .. }) => Some((true, lit)),
            _ => None,
        },
        _ => None,
    }
}

fn expr_to_usize(expr: &Expr) -> Option<usize> {
    let (negated, literal) = expr_literal(expr)?;
    if negated {
        return None;
    }

    match literal {
        Lit::Int(value) => value.base10_parse::<usize>().ok(),
        Lit::Str(value) => value.value().trim().parse::<usize>().ok(),
        _ => None,
    }
}

fn expr_to_f64(expr: &Expr) -> Option<f64> {
    let (negated, literal) = expr_literal(expr)?;
    let value = match literal {
        Lit::Int(value) => value.base10_parse::<f64>().ok()?,
        Lit::Float(value) => value.base10_parse::<f64>().ok()?,
        Lit::Str(value) => value.value().trim().parse::<f64>().ok()?,
        _ => return None,
    };

    Some(if negated { -value } else { value })
}

fn expr_to_string(expr: &Expr) -> Option<String> {
    match expr_literal(expr)? {
        (false, Lit::Str(value)) => Some(value.value()),
        _ => None,
    }
}
