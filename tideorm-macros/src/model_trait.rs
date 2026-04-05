use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::context::BuildContext;
use crate::parse::relation_generic_types;
use crate::relation_gen::generate_with_relations_method;

mod builders;
mod eager_loader;
mod entity_manager_support;
mod helper_methods;
mod internal_model;
mod model_trait_impl;
mod soft_delete;

use builders::*;
use eager_loader::generate_eager_loader_impl;
use entity_manager_support::generate_entity_manager_support_impl;
use helper_methods::generate_helper_methods_impl;
use internal_model::generate_internal_model_impl;
use model_trait_impl::generate_model_trait_impl;
use soft_delete::generate_soft_delete_impl;

pub(crate) fn generate_model_support(ctx: &BuildContext) -> TokenStream2 {
    let internal_model_impl = generate_internal_model_impl(ctx);
    let helper_methods_impl = generate_helper_methods_impl(ctx);
    let model_trait_impl = generate_model_trait_impl(ctx);
    let soft_delete_impl = generate_soft_delete_impl(ctx);
    let eager_loader_impl = generate_eager_loader_impl(ctx);
    let entity_manager_support_impl = generate_entity_manager_support_impl(ctx);
    quote! {
        #internal_model_impl
        #helper_methods_impl
        #model_trait_impl
        #soft_delete_impl
        #eager_loader_impl
        #entity_manager_support_impl
    }
}
