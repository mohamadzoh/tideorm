use super::*;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Lit, Meta, Token, UnOp};

/// What a rule takes after its name.
#[derive(Clone, Copy)]
enum Arity {
    /// Nothing: `email`.
    Flag,
    /// A non-negative integer: `min_length = 3`.
    Count,
    /// A finite number: `min = 18`.
    Number,
    /// A string literal: `regex = ".."`.
    Text,
    /// Two finite bounds: `range(1, 10)`.
    Range,
}

/// What kind of field a rule checks.
#[derive(Clone, Copy)]
enum Applies {
    Any,
    Text,
    Number,
}

/// Every rule `#[validate(..)]` accepts: its name, its `ValidationRule`
/// variant, what it takes, and what it checks.
const RULES: &[(&str, &str, Arity, Applies)] = &[
    ("required", "Required", Arity::Flag, Applies::Any),
    ("email", "Email", Arity::Flag, Applies::Text),
    ("url", "Url", Arity::Flag, Applies::Text),
    ("alpha", "Alpha", Arity::Flag, Applies::Text),
    ("alphanumeric", "Alphanumeric", Arity::Flag, Applies::Text),
    ("numeric", "Numeric", Arity::Flag, Applies::Text),
    ("uuid", "Uuid", Arity::Flag, Applies::Text),
    ("min_length", "MinLength", Arity::Count, Applies::Text),
    ("max_length", "MaxLength", Arity::Count, Applies::Text),
    ("length", "Length", Arity::Count, Applies::Text),
    ("min", "Min", Arity::Number, Applies::Number),
    ("max", "Max", Arity::Number, Applies::Number),
    ("range", "Range", Arity::Range, Applies::Number),
    ("regex", "Regex", Arity::Text, Applies::Text),
];

/// The rule names, as diagnostics list them.
fn supported_rules() -> String {
    RULES
        .iter()
        .map(|(name, ..)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn parse_validation_attributes(field: &ModelField) -> syn::Result<Vec<TokenStream2>> {
    let mut rules = Vec::new();

    for attr in &field.attrs {
        if !attr.path().is_ident("validate") {
            continue;
        }

        if field.relation_kind().is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "#[validate(..)] does not apply to a relation field; declare the rules on the \
                 related model's fields",
            ));
        }

        if !matches!(&attr.meta, Meta::List(_)) {
            return Err(syn::Error::new_spanned(
                attr,
                format!(
                    "#[validate(..)] expects a parenthesized rule list; supported rules: {}",
                    supported_rules()
                ),
            ));
        }

        attr.parse_nested_meta(|meta| parse_rule(field, &meta, &mut rules))?;
    }

    Ok(rules)
}

fn parse_rule(
    field: &ModelField,
    meta: &ParseNestedMeta,
    rules: &mut Vec<TokenStream2>,
) -> syn::Result<()> {
    let rule_ident = meta.path.get_ident().cloned().ok_or_else(|| {
        syn::Error::new_spanned(
            &meta.path,
            format!(
                "unknown validation rule; supported rules: {}",
                supported_rules()
            ),
        )
    })?;
    let rule = unraw_ident(&rule_ident);
    let Some(&(_, variant, arity, applies)) = RULES.iter().find(|(name, ..)| *name == rule) else {
        // A field rule cannot run model code, so `custom` used to compile to a
        // marker nothing evaluated and silently accepted every value.
        if rule == "custom" {
            return Err(syn::Error::new_spanned(
                &rule_ident,
                "`#[validate(custom = ..)]` is not supported: implement \
                 `tideorm::Callbacks::after_validation` (or `before_validation`) on the model \
                 and return `Err(tideorm::Error::validation(field, message))` from it",
            ));
        }
        return Err(syn::Error::new_spanned(
            &rule_ident,
            format!(
                "unknown validation rule '{rule}'; supported rules: {}",
                supported_rules()
            ),
        ));
    };
    ensure_validation_compatibility(field, &rule_ident, &rule, applies)?;

    let variant = format_ident!("{}", variant);
    let rule_path = quote!(::tideorm::validation::ValidationRule::#variant);
    let tokens = match arity {
        Arity::Flag => {
            expect_flag(meta, &rule_ident)?;
            rule_path
        }
        Arity::Count => {
            let value = parse_single(
                meta,
                &rule_ident,
                expr_to_usize,
                "a non-negative integer",
                "3",
            )?;
            quote!(#rule_path(#value))
        }
        Arity::Number => {
            let value = parse_single(meta, &rule_ident, expr_to_f64, "a finite number", "18")?;
            quote!(#rule_path(#value))
        }
        Arity::Text => {
            let value = parse_single(
                meta,
                &rule_ident,
                expr_to_string,
                "a string literal",
                "\"...\"",
            )?;
            quote!(#rule_path(#value.to_string()))
        }
        Arity::Range => {
            let (min, max) = parse_range_rule(meta, &rule_ident)?;
            quote!(#rule_path(#min, #max))
        }
    };

    rules.push(tokens);
    Ok(())
}

fn ensure_validation_compatibility(
    field: &ModelField,
    rule_ident: &Ident,
    rule: &str,
    applies: Applies,
) -> syn::Result<()> {
    let (compatible, expected) = match applies {
        Applies::Any => return Ok(()),
        Applies::Text => (field.supports_string_validations(), "a string field"),
        Applies::Number => (
            field.supports_numeric_validations(),
            "a numeric field or string field",
        ),
    };
    if compatible {
        return Ok(());
    }

    Err(syn::Error::new_spanned(
        rule_ident,
        format!(
            "validation rule '{}' is incompatible with field '{}' of type '{}'; expected {}",
            rule,
            field.name(),
            type_string(field.validation_base_type()),
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

/// The one value a rule takes, read by `convert`; `expected` and `example`
/// describe it when it does not read.
fn parse_single<T>(
    meta: &ParseNestedMeta,
    rule_ident: &Ident,
    convert: fn(&Expr) -> Option<T>,
    expected: &str,
    example: &str,
) -> syn::Result<T> {
    let values = rule_values(meta, rule_ident)?;
    let value = single_value(&values, rule_ident)?;
    convert(value).ok_or_else(|| {
        syn::Error::new_spanned(
            value,
            format!(
                "validation rule '{rule_ident}' expects {expected}, e.g. `{rule_ident} = {example}`"
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

    // `range(min..=max)`. The rule includes its upper bound, which `min..max`
    // leaves out in Rust, so that spelling is refused rather than read as
    // including it.
    if let Expr::Range(range) = value {
        if matches!(range.limits, syn::RangeLimits::HalfOpen(_)) {
            return Err(syn::Error::new_spanned(
                value,
                format!(
                    "validation rule '{rule_ident}' includes its upper bound; write \
                     `{rule_ident}(min..=max)` or `{rule_ident}(min, max)`"
                ),
            ));
        }
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
    let max = max.strip_prefix('=').unwrap_or(max);
    let min = min
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|min| min.is_finite())?;
    let max = max
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|max| max.is_finite())?;
    Some((min, max))
}

fn range_error(value: &Expr, rule_ident: &Ident) -> syn::Error {
    syn::Error::new_spanned(
        value,
        format!(
            "validation rule '{rule_ident}' expects `{rule_ident} = \"min..max\"` or `{rule_ident}(min, max)` with finite bounds"
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

    // A non-finite bound has no literal to emit, and every value fails
    // `min = inf` or passes `max = inf`.
    Some(if negated { -value } else { value }).filter(|value| value.is_finite())
}

fn expr_to_string(expr: &Expr) -> Option<String> {
    match expr_literal(expr)? {
        (false, Lit::Str(value)) => Some(value.value()),
        _ => None,
    }
}
