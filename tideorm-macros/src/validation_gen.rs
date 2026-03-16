use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::context::BuildContext;

pub(crate) fn generate_validation_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    if !ctx.validation_rules.is_empty() {
        let validation_checks: Vec<_> = ctx
            .validation_rules
            .iter()
            .map(|(field_name, rules)| {
                let field_ident = format_ident!("{}", field_name);
                quote! {
                    {
                        let rules: Vec<::tideorm::validation::ValidationRule> = vec![#(#rules),*];
                        for rule in &rules {
                            if let Some(msg) = ::tideorm::validation::Validator::validate_rule(&self.#field_ident, rule, #field_name) {
                                errors.add(#field_name, msg);
                            }
                        }
                    }
                }
            })
            .collect();

        let rules_list: Vec<_> = ctx
            .validation_rules
            .iter()
            .map(|(field_name, rules)| quote! { (#field_name, vec![#(#rules),*]) })
            .collect();

        quote! {
            impl ::tideorm::validation::Validate for #struct_name {
                fn validation_rules() -> Vec<(&'static str, Vec<::tideorm::validation::ValidationRule>)> {
                    vec![#(#rules_list),*]
                }

                fn validate(&self) -> Result<(), ::tideorm::validation::ValidationErrors> {
                    let mut errors = ::tideorm::validation::ValidationErrors::new();
                    #(#validation_checks)*
                    if let Err(custom_errors) = self.custom_validations() {
                        errors.merge(custom_errors);
                    }
                    errors.to_result()
                }
            }
        }
    } else {
        quote! {
            impl ::tideorm::validation::Validate for #struct_name {
                fn validate(&self) -> Result<(), ::tideorm::validation::ValidationErrors> {
                    self.custom_validations()
                }
            }
        }
    }
}
