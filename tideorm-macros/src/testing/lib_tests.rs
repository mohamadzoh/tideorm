use super::*;
use quote::quote;
use syn::{Type, parse_quote};

use crate::parse::ModelField;

fn normalize_tokens(tokens: &str) -> String {
    tokens.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn field_with_type(ty: Type) -> ModelField {
    ModelField {
        ident: Some(parse_quote!(field)),
        ty,
        attrs: vec![],
        primary_key: false,
        auto_increment: false,
        column: None,
        nullable: false,
        default: None,
        skip: false,
        timestamp: false,
        has_one: None,
        has_many: None,
        belongs_to: None,
        has_many_through: None,
        foreign_key: None,
        owner_key: None,
        local_key: None,
        pivot: None,
        related_key: None,
        morph_name: None,
    }
}

#[test]
fn detects_direct_relation_wrappers() {
    for ty in [
        parse_quote!(HasOne<User>),
        parse_quote!(::tideorm::relations::HasMany<Post>),
        parse_quote!(BelongsTo<Account>),
        parse_quote!(HasManyThrough<Role, UserRole>),
        parse_quote!(MorphTo<Commentable>),
        parse_quote!(MorphOne<Image>),
        parse_quote!(MorphMany<Tag>),
        parse_quote!(SelfRef<Employee>),
        parse_quote!(SelfRefMany<Employee>),
    ] {
        assert!(field_with_type(ty).is_relation_type());
    }
}

#[test]
fn detects_wrapped_relation_wrappers() {
    assert!(field_with_type(parse_quote!(Option<HasOne<User>>)).is_relation_type());
    assert!(field_with_type(parse_quote!(Box<::tideorm::MorphMany<Tag>>)).is_relation_type());
}

#[test]
fn does_not_match_non_relation_names_by_substring() {
    assert!(!field_with_type(parse_quote!(HasOneCount)).is_relation_type());
    assert!(!field_with_type(parse_quote!(MyBelongsToMetadata)).is_relation_type());
    assert!(!field_with_type(parse_quote!(Vec<HasManyLabel>)).is_relation_type());
    assert!(!field_with_type(parse_quote!(String)).is_relation_type());
}

#[test]
fn model_attribute_accepts_inline_table_options() {
    let input: DeriveInput = parse_quote! {
        pub struct User {
            pub id: i64,
        }
    };

    let expanded = expand_model(quote!(table = "users", soft_delete), input)
        .expect("inline model attribute should expand successfully")
        .to_string();
    let normalized = normalize_tokens(&expanded);

    assert!(normalized.contains("#[derive(tideorm::Model)]"));
    assert!(normalized.contains("#[tideorm(table=\"users\",soft_delete)]"));
}

#[test]
fn model_attribute_preserves_stacked_tideorm_attribute() {
    let input: DeriveInput = parse_quote! {
        #[tideorm(table = "users")]
        pub struct User {
            pub id: i64,
        }
    };

    let expanded = expand_model(TokenStream2::new(), input)
        .expect("stacked tideorm syntax should still expand successfully")
        .to_string();
    let normalized = normalize_tokens(&expanded);

    assert!(normalized.contains("#[tideorm(table=\"users\")]"));
}

#[test]
fn model_attribute_rejects_mixed_inline_and_stacked_options() {
    let input: DeriveInput = parse_quote! {
        #[tideorm(table = "users")]
        pub struct User {
            pub id: i64,
        }
    };

    let error = expand_model(quote!(table = "users"), input)
        .expect_err("mixed syntax should be rejected")
        .to_string();

    assert!(error.contains("use either #[tideorm::model(...)] or a separate #[tideorm(...)]"));
}
