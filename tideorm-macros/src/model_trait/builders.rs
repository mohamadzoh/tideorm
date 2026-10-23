use super::*;

use quote::format_ident;
use syn::Ident;

use crate::meta_support::{
    auto_timestamp_value, auto_updated_at_value, clock_binding, is_managed_created_at,
};
use crate::parse::ModelField;

pub(super) fn build_primary_key_value_impl(ctx: &BuildContext) -> TokenStream2 {
    if let [pk_ident] = ctx.pk_idents.as_slice() {
        quote!(self.#pk_ident.clone())
    } else {
        let pk_idents = &ctx.pk_idents;
        quote!((#(self.#pk_idents.clone()),*))
    }
}

/// The key values bind through `bindable_value`, so a `u64` key past `i64::MAX`
/// matches nothing instead of panicking the SQLite and PostgreSQL drivers.
pub(super) fn build_primary_key_condition_impl(ctx: &BuildContext) -> TokenStream2 {
    let internal_entity_mod = &ctx.internal_entity_mod;
    let (bind, components) = ctx.primary_key_components();
    let pk_column_variants = &ctx.pk_column_variants;

    quote! {
        use ::tideorm::orm::ColumnTrait;
        #bind
        ::tideorm::orm::Condition::all()
            #(.add(#internal_entity_mod::Column::#pk_column_variants
                .eq(::tideorm::internal::bindable_value(#components.clone()))))*
    }
}

/// One `match` pattern per persisted field, accepting its field name and —
/// when `#[tideorm(column = "..")]` renames it — its column name, unless
/// another field is named so: a field name wins over a column name spelled
/// alike, as `ModelMeta::canonical_field_name` has it.
pub(super) fn build_name_patterns(ctx: &BuildContext) -> Vec<TokenStream2> {
    ctx.field_names
        .iter()
        .zip(&ctx.column_names)
        .map(|(field_name, column_name)| {
            if field_name == column_name || ctx.field_names.contains(column_name) {
                quote!(#field_name)
            } else {
                quote!(#field_name | #column_name)
            }
        })
        .collect()
}

/// A conversion between the model and its generated entity types.
#[derive(Clone, Copy)]
pub(super) enum Conversion {
    /// The INSERT `ActiveModel`. `encrypt: false` is the infallible
    /// `into_active_model`, which cannot report a cipher failure and so leaves
    /// encrypted columns `NotSet` rather than writing their plaintext.
    Insert { encrypt: bool },
    /// The UPDATE `ActiveModel`.
    Update,
    /// The generated entity `Model` back into the model, decrypting.
    FromEntity,
    /// The model into the generated entity `Model`. `encrypt: false` is the
    /// plaintext rendering in-memory comparisons want: two ciphertexts of a
    /// randomized cipher would differ on every field.
    ToEntity { encrypt: bool },
}

/// What an INSERT or UPDATE conversion's initializers read besides the
/// model, which its bindings provide before any field moves.
#[derive(Default)]
struct Reads {
    /// The write's one clock reading, which every timestamp it stamps holds.
    clock: bool,
    /// Whether the auto-increment key is still the unsaved default.
    key_is_new: bool,
}

/// The variable holding whether an INSERT's auto-increment key is new.
fn key_is_new_ident() -> Ident {
    format_ident!("__tideorm_key_is_new")
}

/// The bindings and the `field: value` initializers of an INSERT or UPDATE
/// conversion, the bindings to place before the initializers: exactly what
/// they read, decided where they read it.
pub(super) fn write_conversion(
    ctx: &BuildContext,
    conversion: Conversion,
) -> (TokenStream2, Vec<TokenStream2>) {
    let mut reads = Reads::default();
    let initializers = initializers(ctx, conversion, &mut reads);
    let clock = reads.clock.then(clock_binding);
    let key_is_new = reads.key_is_new.then(|| {
        let key_is_new = key_is_new_ident();
        quote!(let #key_is_new = <Self as ::tideorm::model::Model>::is_new(&self);)
    });
    (quote!(#clock #key_is_new), initializers)
}

/// The `field: value` initializers of a conversion between the model and its
/// entity `Model`, which read nothing but the model.
pub(super) fn column_initializers(ctx: &BuildContext, conversion: Conversion) -> Vec<TokenStream2> {
    debug_assert!(matches!(
        conversion,
        Conversion::FromEntity | Conversion::ToEntity { .. }
    ));
    initializers(ctx, conversion, &mut Reads::default())
}

/// The `field: value` initializers of one conversion, one per persisted column.
///
/// Every conversion routes encrypted columns through the cipher hooks and the
/// rest straight through; the INSERT and UPDATE setters additionally manage
/// keys and timestamps, and have to agree with each other on both.
fn initializers(
    ctx: &BuildContext,
    conversion: Conversion,
    reads: &mut Reads,
) -> Vec<TokenStream2> {
    ctx.db_fields
        .iter()
        .map(|field| {
            let ident = field.ident();
            let value = column_value(ctx, field, conversion, reads);
            quote!(#ident: #value)
        })
        .collect()
}

fn column_value(
    ctx: &BuildContext,
    field: &ModelField,
    conversion: Conversion,
    reads: &mut Reads,
) -> TokenStream2 {
    let ident = field.ident();
    let encrypted = ctx.is_encrypted(field);

    match conversion {
        Conversion::FromEntity if encrypted => cipher_call(
            ctx,
            field,
            quote!(__decrypt_model_field),
            quote!(model.#ident),
        ),
        Conversion::FromEntity => quote!(model.#ident),
        Conversion::ToEntity { encrypt: true } if encrypted => cipher_call(
            ctx,
            field,
            quote!(__encrypt_model_field),
            quote!(self.#ident.clone()),
        ),
        Conversion::ToEntity { .. } => quote!(self.#ident.clone()),
        Conversion::Insert { encrypt } => {
            // The database numbers a key still at its unsaved default; one the
            // model holds is written, as an upsert writes it.
            if field.primary_key && field.auto_increment {
                reads.key_is_new = true;
                let key_is_new = key_is_new_ident();
                quote!(::tideorm::model::__auto_increment_key(#key_is_new, self.#ident))
            } else if field.primary_key && field.is_uuid() {
                quote!(ActiveValue::Set(::tideorm::model::__uuid_key(self.#ident)))
            } else if let Some(now) = auto_timestamp_value(field) {
                reads.clock = true;
                quote!(ActiveValue::Set(#now))
            } else if encrypted && !encrypt {
                quote!(ActiveValue::NotSet)
            } else {
                set_value(ctx, field, encrypted)
            }
        }
        Conversion::Update => {
            if field.primary_key {
                quote!(ActiveValue::Unchanged(self.#ident))
            } else if let Some(now) = auto_updated_at_value(field) {
                reads.clock = true;
                quote!(ActiveValue::Set(#now))
            } else if is_managed_created_at(field) {
                quote!(ActiveValue::NotSet)
            } else {
                set_value(ctx, field, encrypted)
            }
        }
    }
}

/// `ActiveValue::Set(self.field)`, encrypted first when the column is.
fn set_value(ctx: &BuildContext, field: &ModelField, encrypted: bool) -> TokenStream2 {
    let ident = field.ident();
    let value = if encrypted {
        cipher_call(
            ctx,
            field,
            quote!(__encrypt_model_field),
            quote!(self.#ident),
        )
    } else {
        quote!(self.#ident)
    };
    quote!(ActiveValue::Set(#value))
}

/// A call to one of the runtime's encrypted-field hooks.
fn cipher_call(
    ctx: &BuildContext,
    field: &ModelField,
    hook: TokenStream2,
    value: TokenStream2,
) -> TokenStream2 {
    let table_name = &ctx.table_name;
    let field_name = field.name();
    let column_name = field.column_name();
    quote!(::tideorm::model::#hook(#value, #table_name, #field_name, #column_name)?)
}
