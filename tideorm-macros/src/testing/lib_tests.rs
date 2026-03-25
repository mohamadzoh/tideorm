use super::*;
use darling::FromDeriveInput;
use quote::quote;
use syn::{DeriveInput, Type, parse_quote};

use crate::context::BuildContext;
use crate::meta_support::detect_existing_derives;
use crate::parse::ModelInput;
use crate::serde_gen::generate_trait_impls;

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
fn column_type_expr_emits_compile_error_for_unknown_types() {
    let tokens = field_with_type(parse_quote!(CustomEnum))
        .column_type_expr()
        .to_string();
    let normalized = normalize_tokens(&tokens);

    assert!(normalized.contains("::core::compile_error!"));
    assert!(normalized.contains("unsupportedTideORMcolumntype'CustomEnum'inschemageneration"));
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
fn model_attribute_preserves_user_derives() {
    let input: DeriveInput = parse_quote! {
        #[derive(PartialEq, Eq, Hash)]
        pub struct User {
            pub id: i64,
        }
    };

    let expanded = expand_model(quote!(table = "users"), input)
        .expect("user derives should be preserved")
        .to_string();
    let normalized = normalize_tokens(&expanded);

    assert!(normalized.contains("#[derive(tideorm::Model)]"));
    assert!(normalized.contains("#[derive(PartialEq,Eq,Hash)]"));
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

#[test]
fn field_level_timestamp_attribute_is_rejected() {
    let input: DeriveInput = parse_quote! {
        struct User {
            #[tideorm(timestamp)]
            created_at: chrono::NaiveDateTime,
        }
    };

    let error = ModelInput::from_derive_input(&input)
        .expect_err("field-level timestamp attribute should be rejected")
        .to_string();

    assert!(error.contains("timestamp"));
}

#[test]
fn deserialize_impl_requires_missing_non_optional_fields() {
    let input: DeriveInput = parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            name: String,
            nickname: Option<String>,
        }
    };

    let existing_derives = detect_existing_derives(&input.attrs);
    let model_input = ModelInput::from_derive_input(&input).expect("model input should parse");
    let ctx = BuildContext::new(&model_input, vec![], vec![], &existing_derives)
        .expect("build context should be constructed");
    let generated = generate_trait_impls(&ctx).to_string();
    let normalized = normalize_tokens(&generated);

    assert!(normalized.contains("id:__field_id.unwrap_or_default()"));
    assert!(normalized.contains("nickname:__field_nickname.unwrap_or_default()"));
    assert!(
        normalized.contains(
            "name:__field_name.ok_or_else(||::serde::de::Error::missing_field(\"name\"))?"
        )
    );
    assert!(normalized.contains("let__field_id=seq.next_element()?.unwrap_or_default();"));
    assert!(normalized.contains("let__field_nickname=seq.next_element()?.unwrap_or_default();"));
    assert!(normalized.contains("let__field_name=seq.next_element()?.ok_or_else(||::serde::de::Error::invalid_length(1usize,&self))?;"));
}
