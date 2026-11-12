use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;

pub(crate) fn generate_validation_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    if ctx.validation_rules.is_empty() {
        return quote! {
            impl ::tideorm::validation::Validate for #struct_name {
                fn validate(&self) -> ::core::result::Result<(), ::tideorm::validation::ValidationErrors> {
                    Ok(())
                }
            }
        };
    }

    let validation_checks = ctx.validation_rules.iter().flat_map(|(_, checks)| checks);

    quote! {
        impl ::tideorm::validation::Validate for #struct_name {
            fn validate(&self) -> ::core::result::Result<(), ::tideorm::validation::ValidationErrors> {
                let mut errors = ::tideorm::validation::ValidationErrors::new();
                #(#validation_checks)*
                errors.to_result()
            }
        }
    }
}
