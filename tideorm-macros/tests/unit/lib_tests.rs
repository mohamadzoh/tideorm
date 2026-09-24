use super::*;
use darling::FromDeriveInput;
use proc_macro2::{Delimiter, Group};
use quote::quote;
use syn::{DeriveInput, Type, parse_quote};

use crate::case::{to_pascal_case, to_snake_case};
use crate::context::BuildContext;
use crate::meta_support::{detect_existing_derives, pluralize};
use crate::parse::{
    ModelField, ModelInput, RelationKind, is_optional_type, option_inner_type,
    parse_index_attributes,
};
use crate::serde_gen::generate_trait_impls;

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

fn build_context_for(input: &DeriveInput) -> syn::Result<BuildContext> {
    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);
    let existing_derives = detect_existing_derives(&input.attrs);
    let model_input = ModelInput::from_derive_input(input).expect("model input should parse");
    BuildContext::new(&model_input, indexes, unique_indexes, &existing_derives)
}

/// The error the derive reports for `input`: from building the context, or
/// one it defers so the rest of the model still expands.
fn build_error_for(input: &DeriveInput) -> String {
    match build_context_for(input) {
        Ok(ctx) => ctx
            .attribute_errors
            .first()
            .map(ToString::to_string)
            .expect("the model should be rejected"),
        Err(error) => error.to_string(),
    }
}

/// Expand a model definition through the real derive and return its tokens with all
/// whitespace removed, which is how the assertions below match generated fragments.
fn expand_model_tokens(input: DeriveInput) -> String {
    let existing_derives = detect_existing_derives(&input.attrs);
    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);
    let model_input = ModelInput::from_derive_input(&input).expect("model input should parse");

    normalize_tokens(
        &generate_model_impl(&model_input, indexes, unique_indexes, &existing_derives).to_string(),
    )
}

/// The `ActiveModel { .. }` setters of one generated fn, sliced out of a normalized
/// expansion.
///
/// The insert and update paths emit setters that read identically, so a `contains`
/// over the whole expansion cannot tell which path it matched — which is exactly how
/// an `updated_at` that was right on INSERT and wrong on UPDATE slipped through.
fn active_model_setters<'a>(expanded: &'a str, fn_marker: &str) -> &'a str {
    let start = expanded
        .find(fn_marker)
        .unwrap_or_else(|| panic!("expansion should contain `{fn_marker}`"));
    let body = &expanded[start..];
    let literal = body
        .find("__tideorm_internal_")
        .and_then(|path| Some(path + body[path..].find("ActiveModel{")?))
        .unwrap_or_else(|| panic!("`{fn_marker}` should build an ActiveModel"));
    let setters = &body[literal + "ActiveModel{".len()..];
    &setters[..setters
        .find('}')
        .expect("the ActiveModel literal should close")]
}

/// The INSERT path, `InternalModel::into_active_model`.
fn insert_setters(expanded: &str) -> &str {
    active_model_setters(expanded, "fninto_active_model(self)")
}

/// The encrypting INSERT path, emitted for encrypted models only.
fn encrypted_insert_setters(expanded: &str) -> &str {
    active_model_setters(expanded, "fntry_into_active_model(self)")
}

/// The UPDATE path, `__into_update_active_model`.
fn update_setters(expanded: &str) -> &str {
    active_model_setters(expanded, "fn__into_update_active_model(self)")
}

fn rendered_rules(ctx: &BuildContext, field: &str) -> String {
    let rules = ctx
        .validation_rules
        .iter()
        .find(|(ident, _)| ident == field)
        .map(|(_, rules)| rules)
        .expect("field should carry validation rules");

    let rendered: Vec<String> = rules.iter().map(ToString::to_string).collect();
    normalize_tokens(&rendered.join(" "))
}

#[test]
fn detect_existing_derives_matches_last_path_segments_only() {
    let item: syn::DeriveInput = parse_quote! {
        #[derive(core::fmt::Debug, std::clone::Clone, ::serde::Deserialize, serde::Serialize)]
        struct Example;
    };
    let existing = detect_existing_derives(&item.attrs);
    assert!(existing.has_debug);
    assert!(existing.has_clone);
    assert!(!existing.has_default);
    assert!(existing.has_serialize);
    assert!(existing.has_deserialize);

    let item: syn::DeriveInput = parse_quote! {
        #[derive(MyDebugHelper, Cloneable, Defaultish, SerializeWith, DeserializeSeed)]
        struct Example;
    };
    let existing = detect_existing_derives(&item.attrs);
    assert!(!existing.has_debug);
    assert!(!existing.has_clone);
    assert!(!existing.has_default);
    assert!(!existing.has_serialize);
    assert!(!existing.has_deserialize);
}

#[test]
fn pluralize_handles_irregular_nouns_and_suffixes() {
    assert_eq!(pluralize("person"), "people");
    assert_eq!(pluralize("child"), "children");
    assert_eq!(pluralize("mouse"), "mice");
    assert_eq!(pluralize("leaf"), "leaves");
    assert_eq!(pluralize("knife"), "knives");
    assert_eq!(pluralize("profile"), "profiles");
    assert_eq!(pluralize("roof"), "roofs");
    assert_eq!(pluralize("belief"), "beliefs");
    assert_eq!(pluralize("chef"), "chefs");
    assert_eq!(pluralize("cliff"), "cliffs");
    assert_eq!(pluralize("proof"), "proofs");
    assert_eq!(pluralize("staff"), "staffs");
    assert_eq!(pluralize("quiz"), "quizzes");
    assert_eq!(pluralize("fez"), "fezzes");
    assert_eq!(pluralize("bus"), "buses");
    assert_eq!(pluralize("status"), "statuses");
    assert_eq!(pluralize("topaz"), "topazes");
    assert_eq!(pluralize("index"), "indexes");
    assert_eq!(pluralize("category"), "categories");
    assert_eq!(pluralize("key"), "keys");
}

#[test]
fn case_conversion_keeps_the_names_existing_models_map_to() {
    // Derived table and column names come from these; changing a row renames a
    // table or column in every database an existing model already maps to.
    let cases = [
        ("UserProfile", "user_profile", "UserProfile"),
        ("HTTPRequest", "http_request", "HttpRequest"),
        ("MyXMLParser", "my_xml_parser", "MyXmlParser"),
        ("CustomerID", "customer_id", "CustomerId"),
        ("camelCase", "camel_case", "CamelCase"),
        ("created_at", "created_at", "CreatedAt"),
        ("sha256", "sha_256", "Sha256"),
        ("address_line1", "address_line_1", "AddressLine1"),
        ("S3Object", "s_3_object", "S3Object"),
        ("OAuth2Token", "o_auth_2_token", "OAuth2Token"),
        ("a1b2", "a_1_b_2", "A1B2"),
        ("_private", "_private", "Private"),
        ("field__double", "field__double", "FieldDouble"),
        ("Größe", "größe", "Größe"),
        // `ọ̀` is `ọ` plus a combining grave accent: one grapheme, one letter.
        ("Ọ̀rọ̀Ìwé", "ọ̀rọ̀_ìwé", "Ọ̀rọ̀Ìwé"),
    ];
    for (ident, snake, pascal) in cases {
        assert_eq!(to_snake_case(ident), snake, "snake_case of {ident}");
        assert_eq!(to_pascal_case(ident), pascal, "PascalCase of {ident}");
    }
}

#[test]
fn option_detection_is_structural_not_a_substring_test() {
    assert!(is_optional_type(&parse_quote!(Option<String>)));
    assert!(is_optional_type(&parse_quote!(std::option::Option<i64>)));
    assert!(option_inner_type(&parse_quote!(Option<i64>)).is_some());

    assert!(!is_optional_type(&parse_quote!(OptionalMode)));
    assert!(!is_optional_type(&parse_quote!(MyOptions)));
    assert!(!is_optional_type(&parse_quote!(Vec<Option<String>>)));
    assert!(option_inner_type(&parse_quote!(String)).is_none());

    // Macro-substituted types arrive wrapped in an invisible group.
    let grouped = Group::new(Delimiter::None, quote!(Option<String>));
    let ty: Type = syn::parse2(quote!(#grouped)).expect("grouped type should parse");
    assert!(matches!(ty, Type::Group(_)));
    assert!(is_optional_type(&ty));
}

#[test]
fn relation_kind_comes_from_the_wrapper_type() {
    for (ty, kind) in [
        (parse_quote!(HasOne<User>), RelationKind::HasOne),
        (
            parse_quote!(::tideorm::relations::HasMany<Post>),
            RelationKind::HasMany,
        ),
        (parse_quote!(BelongsTo<Account>), RelationKind::BelongsTo),
        (
            parse_quote!(HasManyThrough<Role, UserRole>),
            RelationKind::HasManyThrough,
        ),
        (parse_quote!(MorphTo<Commentable>), RelationKind::MorphTo),
        (parse_quote!(MorphOne<Image>), RelationKind::MorphOne),
        (parse_quote!(MorphMany<Tag>), RelationKind::MorphMany),
        (parse_quote!(SelfRef<Employee>), RelationKind::SelfRef),
        (
            parse_quote!(SelfRefMany<Employee>),
            RelationKind::SelfRefMany,
        ),
        (parse_quote!(Option<HasOne<User>>), RelationKind::HasOne),
        (
            parse_quote!(Box<::tideorm::MorphMany<Tag>>),
            RelationKind::MorphMany,
        ),
    ] {
        assert_eq!(field_with_type(ty).relation_kind(), Some(kind));
    }

    for ty in [
        parse_quote!(HasOneCount),
        parse_quote!(MyBelongsToMetadata),
        parse_quote!(Vec<HasManyLabel>),
        parse_quote!(String),
    ] {
        assert_eq!(field_with_type(ty).relation_kind(), None);
    }

    let through = field_with_type(parse_quote!(HasManyThrough<Role, UserRole>));
    let expected: Vec<Type> = vec![parse_quote!(Role), parse_quote!(UserRole)];
    assert_eq!(through.related_types(), expected);
}

#[test]
fn relation_attributes_must_agree_with_the_wrapper_type() {
    let mismatched = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(has_many = "Post", foreign_key = "user_id")]
            posts: HasOne<Post>,
        }
    });
    assert!(
        mismatched.contains("#[tideorm(has_many = \"...\")] requires a `HasMany<..>` field"),
        "{mismatched}"
    );

    let not_a_wrapper = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(has_one = "Profile", foreign_key = "user_id")]
            profile: Option<Profile>,
        }
    });
    assert!(
        not_a_wrapper.contains("#[tideorm(has_one = \"...\")] requires a `HasOne<..>` field"),
        "{not_a_wrapper}"
    );
}

#[test]
fn relations_require_the_keys_they_cannot_default() {
    let missing = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            profile: HasOne<Profile>,
        }
    });
    assert!(
        missing.contains("has_one relations require #[tideorm(foreign_key = \"...\")]"),
        "{missing}"
    );

    let through = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            roles: HasManyThrough<Role, UserRole>,
        }
    });
    assert!(
        through.contains(
            "has_many_through relations require #[tideorm(pivot = \"...\")], #[tideorm(foreign_key = \"...\")], #[tideorm(related_key = \"...\")]"
        ),
        "{through}"
    );
}

/// A wrapper field used to need the matching `has_one = ".."` attribute to be wired
/// at all: without it the field silently got an inert `Default` and its
/// `foreign_key` was ignored.
#[test]
fn a_relation_without_a_kind_attribute_is_wired_from_its_wrapper_type() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Parent {
            #[tideorm(primary_key)]
            id: i64,
            #[tideorm(foreign_key = "parent_id")]
            child: HasOne<Child>,
        }
    });

    assert!(expanded.contains("::tideorm::relations::HasOne::new(\"parent_id\",\"id\")"));
    assert!(!expanded.contains("self.child=Default::default()"));
    assert!(expanded.contains(
        "impl::tideorm::orm::Related<<Childas::tideorm::internal::InternalModel>::Entity>forEntity"
    ));
    assert!(!expanded.contains("hasnoeagerpathforHasOnerelations"));
}

#[test]
fn column_type_expr_rejects_unknown_types_with_the_supported_ones() {
    let message = field_with_type(parse_quote!(CustomEnum))
        .column_type_expr()
        .unwrap_err()
        .to_string();

    assert!(
        message.starts_with("unsupported TideORM column type 'CustomEnum': a model field can be"),
        "{message}"
    );
    assert!(message.contains("Store an enum as a String"), "{message}");
}

/// One unsupported field fails the derive with that error alone, instead of
/// also emitting an entity whose trait bounds fail on the same type.
#[test]
fn an_unsupported_field_type_is_the_only_error() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Order {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            status: Status,
        }
    });

    assert!(expanded.starts_with("::core::compile_error!"), "{expanded}");
    assert!(!expanded.contains("DeriveEntity"), "{expanded}");
}

#[test]
fn utc_timestamp_columns_are_recognised_structurally() {
    for ty in [
        parse_quote!(DateTime<Utc>),
        parse_quote!(chrono::DateTime<chrono::Utc>),
        parse_quote!(::chrono::DateTime<::chrono::Utc>),
        parse_quote!(Option<tideorm::chrono::DateTime<tideorm::chrono::Utc>>),
    ] {
        let tokens = normalize_tokens(&field_with_type(ty).column_type_expr().unwrap().to_string());
        assert!(tokens.contains("TimestampWithTimeZone"), "{tokens}");
    }

    // A collection of timestamps is not a timestamp column.
    let message = field_with_type(parse_quote!(Vec<DateTime<Utc>>))
        .column_type_expr()
        .unwrap_err()
        .to_string();
    assert!(
        message.starts_with("unsupported TideORM column type 'Vec<DateTime<Utc>>'"),
        "{message}"
    );

    let tokens = normalize_tokens(
        &field_with_type(parse_quote!(chrono::NaiveDate))
            .column_type_expr()
            .unwrap()
            .to_string(),
    );
    assert!(tokens.contains("ColumnType::Date.def()"), "{tokens}");
}

#[test]
fn soft_delete_accepts_qualified_timestamp_spellings() {
    for ty in [
        quote!(Option<DateTime<Utc>>),
        quote!(std::option::Option<chrono::DateTime<chrono::Utc>>),
        quote!(Option<::chrono::DateTime<::chrono::Utc>>),
        quote!(::core::option::Option<tideorm::chrono::DateTime<tideorm::chrono::Utc>>),
    ] {
        let ty: Type = syn::parse2(ty).expect("type should parse");
        let ctx = build_context_for(&parse_quote! {
            #[tideorm(soft_delete)]
            struct Post {
                #[tideorm(primary_key, auto_increment)]
                id: i64,
                deleted_at: #ty,
            }
        })
        .unwrap_or_else(|error| panic!("`{}` should be accepted: {error}", quote!(#ty)));

        let (ident, column) = ctx.soft_delete.expect("soft delete should be resolved");
        assert_eq!(ident, "deleted_at");
        assert_eq!(column, "deleted_at");
    }

    let error = build_error_for(&parse_quote! {
        #[tideorm(soft_delete)]
        struct Post {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            deleted_at: Vec<DateTime<Utc>>,
        }
    });
    assert!(
        error.contains("soft_delete field must have type Option<chrono::DateTime<chrono::Utc>>")
    );
}

#[test]
fn deleted_at_column_is_reported_by_model_meta_only() {
    let expanded = expand_model_tokens(parse_quote! {
        #[tideorm(soft_delete, deleted_at_column = "archived_on")]
        struct Post {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            archived_on: Option<chrono::DateTime<chrono::Utc>>,
        }
    });

    assert_eq!(expanded.matches("fndeleted_at_column()").count(), 1);
    assert!(expanded.contains("fndeleted_at_column()->&'staticstr{\"archived_on\"}"));
    assert!(expanded.contains("fnsoft_delete_enabled()->bool{true}"));
    assert!(expanded.contains("impl::tideorm::SoftDeleteforPost"));
}

#[test]
fn deleted_at_column_without_soft_delete_is_rejected() {
    let error = build_error_for(&parse_quote! {
        #[tideorm(deleted_at_column = "archived_at")]
        struct Post {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            archived_at: Option<chrono::DateTime<chrono::Utc>>,
        }
    });
    assert!(error.contains("has no effect without #[tideorm(soft_delete)]"));
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
fn model_attribute_keeps_generic_where_clause() {
    let input: DeriveInput = parse_quote! {
        pub struct Wrapper<T>
        where
            T: Clone,
        {
            pub id: i64,
            pub payload: T,
        }
    };

    let expanded = expand_model(quote!(table = "wrappers"), input)
        .expect("generic model should expand")
        .to_string();

    assert!(normalize_tokens(&expanded).contains("structWrapper<T>whereT:Clone"));
}

#[test]
fn model_attribute_keeps_tuple_and_unit_struct_shapes() {
    let tuple: DeriveInput = parse_quote! {
        pub struct Wrapper<T>(pub T) where T: Clone;
    };
    let expanded = expand_model(TokenStream2::new(), tuple)
        .expect("tuple struct should expand")
        .to_string();
    assert!(normalize_tokens(&expanded).contains("structWrapper<T>(pubT)whereT:Clone;"));

    let unit: DeriveInput = parse_quote! {
        pub struct Marker;
    };
    let expanded = expand_model(TokenStream2::new(), unit)
        .expect("unit struct should expand")
        .to_string();
    assert!(normalize_tokens(&expanded).contains("structMarker;"));
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
fn encrypted_fields_accept_field_and_column_names_and_canonicalize_metadata() {
    let ctx = build_context_for(&parse_quote! {
        #[tideorm(encrypted = "customer_phone_number, backup_phone")]
        struct Customer {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(column = "customer_phone_number")]
            phone_number: String,
            backup_phone: Option<String>,
        }
    })
    .expect("build context should be constructed");

    assert_eq!(ctx.encrypted_fields, vec!["phone_number", "backup_phone"]);
    assert_eq!(
        ctx.encrypted_column_names,
        vec!["customer_phone_number", "backup_phone"]
    );
}

#[test]
fn encrypted_fields_accept_fully_qualified_optional_string_types() {
    build_context_for(&parse_quote! {
        #[tideorm(encrypted = "phone_number, backup_phone")]
        struct Customer {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            phone_number: ::std::string::String,
            backup_phone: std::option::Option<std::string::String>,
        }
    })
    .expect("fully qualified encrypted string fields should be accepted");
}

#[test]
fn encrypted_fields_reject_non_string_storage_types() {
    let error = build_error_for(&parse_quote! {
        #[tideorm(encrypted = "age")]
        struct Customer {
            #[tideorm(primary_key)]
            id: i64,
            age: i64,
        }
    });

    assert!(error.contains("#[tideorm(encrypted = ...)] only supports String/Text fields"));
}

/// The plaintext conversions of an encrypted model must never carry an encrypted
/// column, and only an encrypted model needs the fallible overrides at all.
#[test]
fn encrypted_columns_only_cross_the_boundary_through_the_cipher_hooks() {
    let expanded = expand_model_tokens(parse_quote! {
        #[tideorm(encrypted = "phone")]
        struct Customer {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            phone: String,
        }
    });

    assert!(insert_setters(&expanded).contains("phone:ActiveValue::NotSet"));
    assert!(encrypted_insert_setters(&expanded).contains(
        "phone:ActiveValue::Set(::tideorm::model::__encrypt_model_field(self.phone,\"customers\",\"phone\",\"phone\")?)"
    ));
    assert!(expanded.contains(
        "phone:::tideorm::model::__decrypt_model_field(model.phone,\"customers\",\"phone\",\"phone\")?"
    ));
    assert!(expanded.contains("fntry_to_entity_model(&self)"));
    assert!(!expanded.contains("fnfrom_entity_model"));

    let plain = expand_model_tokens(parse_quote! {
        struct Tag {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            name: String,
        }
    });
    assert!(!plain.contains("fntry_into_active_model"));
    assert!(!plain.contains("fntry_to_entity_model"));
    assert!(!plain.contains("fnfrom_entity_model"));
    assert!(plain.contains("fntry_from_entity_model"));
}

#[test]
fn deserialize_impl_requires_missing_non_optional_fields() {
    let ctx = build_context_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            name: String,
            nickname: Option<String>,
        }
    })
    .expect("build context should be constructed");
    let normalized = normalize_tokens(&generate_trait_impls(&ctx).to_string());

    assert!(normalized.contains("id:__field_id.unwrap_or_default()"));
    assert!(normalized.contains("nickname:__field_nickname.unwrap_or_default()"));
    assert!(normalized.contains(
        "name:__field_name.ok_or_else(||::tideorm::serde::de::Error::missing_field(\"name\"))?"
    ));
    assert!(normalized.contains("let__field_id=seq.next_element()?.unwrap_or_default();"));
    assert!(normalized.contains("let__field_nickname=seq.next_element()?.unwrap_or_default();"));
    assert!(normalized.contains("let__field_name=seq.next_element()?.ok_or_else(||::tideorm::serde::de::Error::invalid_length(1usize,&self))?;"));
}

/// A request body never has to carry what TideORM fills in itself: the managed
/// timestamps and fields that are not columns.
#[test]
fn deserialize_impl_defaults_managed_timestamps_and_skipped_fields() {
    let ctx = build_context_for(&parse_quote! {
        struct Account {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            email: String,
            created_at: chrono::DateTime<chrono::Utc>,
            updated_at: chrono::NaiveDateTime,
            #[tideorm(skip)]
            badges: Vec<String>,
            archived_at: chrono::DateTime<chrono::Utc>,
        }
    })
    .expect("build context should be constructed");
    let normalized = normalize_tokens(&generate_trait_impls(&ctx).to_string());

    for field in ["created_at", "updated_at", "badges"] {
        assert!(
            normalized.contains(&format!("{field}:__field_{field}.unwrap_or_default()")),
            "{field}"
        );
    }
    // A timestamp TideORM does not manage is the caller's to send.
    assert!(normalized.contains("archived_at:__field_archived_at.ok_or_else"));
}

/// Auto-population is decided per column, and `has_timestamps()` reports the same
/// answer.
///
/// Gating on the `created_at`/`updated_at` *pair* left a lone `created_at` written
/// through verbatim, so a default-constructed model persisted `1970-01-01T00:00:00Z`
/// while `has_timestamps()` still claimed the model was managed.
#[test]
fn timestamp_population_and_has_timestamps_agree() {
    let now = "ActiveValue::Set(::tideorm::chrono::Utc::now())";

    // A lone `created_at` is still populated, and reported.
    let lone = expand_model_tokens(parse_quote! {
        struct Signup {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            created_at: chrono::DateTime<chrono::Utc>,
        }
    });
    assert!(insert_setters(&lone).contains(&format!("created_at:{now}")));
    assert!(lone.contains("fnhas_timestamps()->bool{true}"));

    // Aliased columns count the same way: the column name is what matters.
    let aliased = expand_model_tokens(parse_quote! {
        struct AuditLog {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(column = "created_at")]
            inserted_on: chrono::DateTime<chrono::Utc>,
            #[tideorm(column = "updated_at")]
            modified_on: chrono::DateTime<chrono::Utc>,
        }
    });
    let insert = insert_setters(&aliased);
    assert!(insert.contains(&format!("inserted_on:{now}")));
    assert!(insert.contains(&format!("modified_on:{now}")));
    assert!(update_setters(&aliased).contains("inserted_on:ActiveValue::NotSet"));
    assert!(aliased.contains("fnhas_timestamps()->bool{true}"));

    // A timestamp column of an unmanageable type keeps the caller's value on both
    // paths, and a lone one is not reported as managed either.
    let untyped = expand_model_tokens(parse_quote! {
        struct Event {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            created_at: i64,
        }
    });
    assert!(insert_setters(&untyped).contains("created_at:ActiveValue::Set(self.created_at)"));
    assert!(untyped.contains("fnhas_timestamps()->bool{false}"));

    let untyped = expand_model_tokens(parse_quote! {
        struct Reading {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            updated_at: i64,
        }
    });
    assert!(update_setters(&untyped).contains("updated_at:ActiveValue::Set(self.updated_at)"));

    let none = expand_model_tokens(parse_quote! {
        struct Tag {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            name: String,
        }
    });
    assert!(none.contains("fnhas_timestamps()->bool{false}"));
}

/// An `Option<DateTime<Utc>>` timestamp has to get `Some(..)` on *both* paths, and
/// an UPDATE leaves `created_at` alone.
#[test]
fn insert_and_update_agree_on_optional_timestamp_columns() {
    let expanded = expand_model_tokens(parse_quote! {
        struct AuditEntry {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            created_at: Option<chrono::DateTime<chrono::Utc>>,
            updated_at: Option<chrono::DateTime<chrono::Utc>>,
        }
    });

    let now = "ActiveValue::Set(Some(::tideorm::chrono::Utc::now()))";
    let insert = insert_setters(&expanded);
    assert!(insert.contains(&format!("created_at:{now}")));
    assert!(insert.contains(&format!("updated_at:{now}")));

    let update = update_setters(&expanded);
    assert!(update.contains(&format!("updated_at:{now}")));
    assert!(update.contains("created_at:ActiveValue::NotSet"));
    assert!(update.contains("id:ActiveValue::Unchanged(self.id)"));
}

/// `timestamps_naive()` columns are `NaiveDateTime` fields, and they are managed
/// too: left alone, a default-constructed model stored `1970-01-01 00:00:00`.
#[test]
fn naive_timestamp_columns_are_populated() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Note {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            created_at: chrono::NaiveDateTime,
            updated_at: Option<NaiveDateTime>,
        }
    });

    let now = "::tideorm::chrono::Utc::now().naive_utc()";
    let insert = insert_setters(&expanded);
    assert!(insert.contains(&format!("created_at:ActiveValue::Set({now})")));
    assert!(insert.contains(&format!("updated_at:ActiveValue::Set(Some({now}))")));

    let update = update_setters(&expanded);
    assert!(update.contains(&format!("updated_at:ActiveValue::Set(Some({now}))")));
    assert!(update.contains("created_at:ActiveValue::NotSet"));
    assert!(expanded.contains("fnhas_timestamps()->bool{true}"));
}

/// A composite key is unsaved when *any* component is still at its default.
/// The runtime counterpart is
/// `test_is_new_treats_defaulted_composite_primary_key_component_as_unsaved`.
#[test]
fn composite_primary_keys_are_new_when_any_component_is_default() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Membership {
            #[tideorm(primary_key)]
            user_id: i64,
            #[tideorm(primary_key)]
            role_id: i64,
        }
    });

    assert!(expanded.contains(
        "false||::tideorm::model::__is_default(&pk_0)||::tideorm::model::__is_default(&pk_1)"
    ));
}

#[test]
fn skipped_fields_are_defaulted_in_generated_constructors() {
    let expanded = expand_model_tokens(parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            email: String,
            #[tideorm(skip)]
            cached_label: Option<String>,
        }
    });

    assert!(expanded.contains("id:model.id,email:model.email,cached_label:Default::default(),"));
    assert!(!expanded.contains("cached_label:model.cached_label"));
}

/// Two relations to the same model are rejected rather than deduplicated.
///
/// Rust permits one `Related<X>` impl per entity pair, and sea-orm's eager loaders
/// resolve through that impl alone. Keeping only the first one compiled, but made
/// `.with("editor")` join on `author_id` and turned a mixed `HasMany`/`HasOne` pair
/// into a runtime cardinality error. A compile error beats silently wrong rows.
#[test]
fn two_relations_to_the_same_model_are_rejected() {
    let error = build_context_for(&parse_quote! {
        struct Article {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            author_id: i64,
            editor_id: i64,
            #[tideorm(belongs_to = "User", foreign_key = "author_id")]
            author: BelongsTo<User>,
            #[tideorm(belongs_to = "User", foreign_key = "editor_id")]
            editor: BelongsTo<User>,
        }
    })
    .and_then(|ctx| crate::entity_gen::generate_entity_support(&ctx))
    .expect_err("two relations to one model should be rejected")
    .to_string();

    assert!(error.contains("relations `author` and `editor` both target `User`"));
    assert!(error.contains("sea-orm permits one `Related<User>` impl per entity pair"));
    assert!(error.contains("load `editor` explicitly with its own query instead of eagerly"));

    // Distinct targets are untouched: one `Related` impl and one `def` arm each.
    let ok = expand_model_tokens(parse_quote! {
        struct Post {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            user_id: i64,
            #[tideorm(belongs_to = "User", foreign_key = "user_id")]
            author: BelongsTo<User>,
            #[tideorm(has_many = "Comment", foreign_key = "post_id")]
            comments: HasMany<Comment>,
        }
    });

    assert!(!ok.contains("compile_error!"));
    assert_eq!(ok.matches("impl::tideorm::orm::Related<").count(), 2);
    assert!(ok.contains("Self::Author=>"));
    assert!(ok.contains("Self::Comments=>"));
}

/// `Related::to`/`via` delegate to the `def` arm, so a bad key is checked (and
/// reported) once, and the polymorphic and self-referencing relations — which no
/// `Related` impl covers — get no `Relation` variant at all.
#[test]
fn relation_joins_are_defined_once_per_entity_relation() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Post {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            user_id: i64,
            parent_id: Option<i64>,
            #[tideorm(belongs_to = "User", foreign_key = "user_id")]
            author: BelongsTo<User>,
            #[tideorm(has_many_through = "Tag", pivot = "post_tags", foreign_key = "post_id", related_key = "tag_id")]
            tags: HasManyThrough<Tag, PostTag>,
            #[tideorm(morph_name = "imageable")]
            images: MorphMany<Image>,
            #[tideorm(foreign_key = "parent_id")]
            children: SelfRefMany<Post>,
        }
    });

    assert!(expanded.contains("pubenumRelation{Author,Tags}"));
    assert!(expanded.contains("fnto()->RelationDef{Relation::Author.def()}"));
    assert!(expanded.contains("fnvia()->Option<RelationDef>{Some(Relation::Tags.def())}"));
    assert_eq!(
        expanded
            .matches("const_:()=assert!(<User>::__has_column_name(\"id\")")
            .count(),
        1
    );
    assert_eq!(
        expanded
            .matches("const_:()=assert!(<PostTag>::__has_column_name(\"post_id\")")
            .count(),
        1
    );
}

/// `MorphTo`'s target is chosen per row, so its type parameter is deliberately
/// unbounded; the derive must not force it through `InternalModel`/`ModelMeta`.
#[test]
fn morph_to_targets_are_never_treated_as_models() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Comment {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            commentable_type: String,
            commentable_id: i64,
            #[tideorm(morph_name = "commentable")]
            commentable: MorphTo<Commentable>,
        }
    });

    assert!(expanded.contains("pubenumRelation{}"));
    assert!(!expanded.contains("<Commentableas::tideorm::internal::InternalModel>"));
    assert!(!expanded.contains("<Commentableas::tideorm::model::ModelMeta>"));
    assert!(
        expanded.contains(
            "::tideorm::relations::MorphTo::new(\"commentable_type\",\"commentable_id\")"
        )
    );
}

#[test]
fn searchable_must_name_a_real_field_or_column() {
    let error = build_context_for(&parse_quote! {
        #[tideorm(searchable = "emial")]
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            email: String,
        }
    })
    .and_then(|ctx| crate::entity_gen::generate_entity_support(&ctx))
    .expect_err("an unknown searchable field should be rejected")
    .to_string();

    assert!(error.contains("references unknown field or column 'emial'"));
}

#[test]
fn language_overrides_are_emitted_only_when_declared() {
    let configured = expand_model_tokens(parse_quote! {
        #[tideorm(translatable = "title")]
        struct Article {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            title: String,
        }
    });
    assert!(!configured.contains("fnallowed_languages()"));
    assert!(!configured.contains("fnfallback_language()"));

    let custom = expand_model_tokens(parse_quote! {
        #[tideorm(translatable = "title", languages = "en,fr", fallback_language = "fr")]
        struct Article {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            title: String,
        }
    });
    assert!(custom.contains(
        "fnallowed_languages()->Vec<String>{vec![\"en\".to_string(),\"fr\".to_string()]}"
    ));
    assert!(custom.contains("fnfallback_language()->String{\"fr\".to_string()}"));
}

/// `tokenization_enabled` is gone from both traits, so a tokenized model emits the
/// `Tokenizable` impl without it and no inherent shims.
#[test]
fn tokenized_models_emit_only_the_tokenizable_impl() {
    let tokenized = expand_model_tokens(parse_quote! {
        #[tideorm(tokenize)]
        struct Session {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
        }
    });
    assert!(tokenized.contains("impl::tideorm::tokenization::TokenizableforSession"));
    assert!(!tokenized.contains("tokenization_enabled"));
    assert!(!tokenized.contains("Tokenizable>::token_encoder()"));

    let plain = expand_model_tokens(parse_quote! {
        struct Plain {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
        }
    });
    assert!(!plain.contains("Tokenizable"));
}

#[test]
fn relation_column_assertion_accessor_is_reachable_across_crates() {
    let expanded = expand_model_tokens(parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(column = "mail")]
            email: String,
        }
    });

    assert!(expanded.contains("pubconstfn__has_column_name(name:&str)->bool{::tideorm::model::__str_eq(name,\"id\")||::tideorm::model::__str_eq(name,\"email\")||::tideorm::model::__str_eq(name,\"mail\")}"));
}

#[test]
fn name_lookups_do_not_repeat_identical_patterns() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Customer {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(column = "customer_phone")]
            phone: String,
        }
    });

    assert!(!expanded.contains("\"id\"|\"id\""));
    assert!(expanded.contains("\"phone\"|\"customer_phone\"=>"));
}

#[test]
fn entity_manager_pk_key_falls_back_instead_of_panicking() {
    let expanded = expand_model_tokens(parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
        }
    });

    assert!(expanded.contains("__pk_to_entity_manager_key(&primary_key).unwrap_or_else("));
    assert!(!expanded.contains("entitymanagerprimarykeyshouldserialize"));
}

/// A `cfg(feature = "entity-manager")` in generated code is evaluated against the
/// model's crate: it warned in every crate without that feature and dropped the
/// items in one that enabled `tideorm/entity-manager` without declaring it.
#[test]
fn entity_manager_items_follow_tideorms_own_feature() {
    let expanded = expand_model_tokens(parse_quote! {
        struct User {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[tideorm(has_many = "Post", foreign_key = "user_id")]
            posts: HasMany<Post>,
        }
    });

    assert!(!expanded.contains("feature=\"entity-manager\""));
    assert!(expanded.contains("::tideorm::__if_entity_manager!{impl::tideorm::entity_manager::TideEntityManagerFieldWriterforUser"));
    assert!(
        expanded.contains("::tideorm::__if_entity_manager!{letrelation=relation.with_metadata(")
    );
}

#[test]
fn relations_without_an_eager_path_name_the_limitation() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Node {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            parent_id: i64,
            children: SelfRefMany<Node>,
        }
    });

    assert!(expanded.contains("hasnoeagerpathforSelfRefManyrelations"));
}

#[test]
fn validation_rejects_email_rule_on_non_string_field() {
    let error = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key)]
            id: i64,
            #[validate(email)]
            age: i64,
        }
    });

    assert!(error.contains("validation rule 'email' is incompatible"));
    assert!(error.contains("field 'age'"));
    assert!(error.contains("type 'i64'"));
}

#[test]
fn validation_rejects_length_rule_on_non_string_field() {
    let error = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key)]
            id: i64,
            #[validate(min_length = 3)]
            age: i64,
        }
    });

    assert!(error.contains("validation rule 'min_length' is incompatible"));
    assert!(error.contains("field 'age'"));
}

#[test]
fn validation_allows_string_rules_on_optional_string_fields() {
    for ty in [
        quote!(Option<String>),
        quote!(std::option::Option<std::string::String>),
    ] {
        let ty: Type = syn::parse2(ty).expect("type should parse");
        let ctx = build_context_for(&parse_quote! {
            struct User {
                #[tideorm(primary_key)]
                id: i64,
                #[validate(email, max_length = 255)]
                email: #ty,
            }
        })
        .expect("optional string validation rules should be accepted");

        assert_eq!(ctx.validation_rules.len(), 1);
    }
}

#[test]
fn validation_values_are_not_split_on_embedded_commas() {
    let ctx = build_context_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key)]
            id: i64,
            #[validate(regex = "^[a-z]{3,5}$", max_length = 5)]
            code: String,
        }
    })
    .expect("validation attributes should parse");
    let rendered = rendered_rules(&ctx, "code");

    assert!(rendered.contains("Regex(\"^[a-z]{3,5}$\".to_string())"));
    assert!(rendered.contains("MaxLength("), "{rendered}");
}

#[test]
fn validation_rejects_unknown_rule_names_instead_of_dropping_them() {
    let error = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key)]
            id: i64,
            #[validate(minlength = 3)]
            name: String,
        }
    });

    assert!(error.contains("unknown validation rule 'minlength'"));
}

#[test]
fn validation_rejects_unparsable_rule_values_instead_of_dropping_them() {
    let error = build_error_for(&parse_quote! {
        struct User {
            #[tideorm(primary_key)]
            id: i64,
            #[validate(min_length = "three")]
            name: String,
        }
    });

    assert!(error.contains("validation rule 'min_length' expects a non-negative integer"));
}

#[test]
fn validation_accepts_assignment_function_and_range_forms() {
    let ctx = build_context_for(&parse_quote! {
        struct Reading {
            #[tideorm(primary_key)]
            id: i64,
            #[validate(min_length(3), range = "1..10")]
            label: String,
        }
    })
    .expect("both rule spellings should parse");
    let rendered = rendered_rules(&ctx, "label");

    assert!(rendered.contains("MinLength(3usize)"));
    assert!(rendered.contains("Range(1f64,10f64)"));
}

/// A raw-identifier field is named without its `r#` everywhere a name is a string —
/// columns, `ModelMeta::field_names`, serde, lookups, validation messages — and keeps
/// it wherever it is code, so a validated `r#type` no longer expands to `self.type`.
#[test]
fn raw_identifier_fields_use_one_unraw_name() {
    let input: DeriveInput = parse_quote! {
        #[tideorm(table = "docs", encrypted = "type")]
        struct Doc {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            #[validate(min_length = 1)]
            r#type: String,
        }
    };

    let ctx = build_context_for(&input).expect("raw identifier fields should be supported");
    assert_eq!(ctx.field_names, vec!["id", "type"]);
    assert_eq!(ctx.column_names, vec!["id", "type"]);
    assert!(ctx.column_variants.iter().any(|variant| variant == "Type"));
    assert_eq!(ctx.encrypted_fields, vec!["type"]);

    let expanded = expand_model_tokens(input);
    assert!(expanded.contains("fnfield_names()->&'static[&'staticstr]{&[\"id\",\"type\"]}"));
    assert!(!expanded.contains("\"r#type\""));
    assert!(!expanded.contains("R#type"));
    assert!(expanded.contains("validate_rule(&self.r#type,rule,\"type\")"));
    assert!(!expanded.contains("self.type"));
    assert!(expanded.contains("\"type\"=>Some(__tideorm_internal_doc::Column::Type)"));
    assert!(encrypted_insert_setters(&expanded).contains(
        "r#type:ActiveValue::Set(::tideorm::model::__encrypt_model_field(self.r#type,\"docs\",\"type\",\"type\")?)"
    ));
}

#[test]
fn internal_entity_module_name_is_snake_cased() {
    let ctx = build_context_for(&parse_quote! {
        struct ApiKey {
            #[tideorm(primary_key)]
            id: i64,
        }
    })
    .expect("build context should be constructed");

    assert_eq!(
        ctx.internal_entity_mod.to_string(),
        "__tideorm_internal_api_key"
    );
}

#[test]
fn index_columns_are_parsed_from_string_literals_containing_option_names() {
    let input: DeriveInput = parse_quote! {
        #[index("name,columns")]
        struct Report {
            #[tideorm(primary_key)]
            id: i64,
            name: String,
            columns: String,
        }
    };

    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);

    assert!(unique_indexes.is_empty());
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].columns, vec!["name", "columns"]);
    assert!(indexes[0].name.is_none());

    build_context_for(&input).expect("index over declared columns should be accepted");
}

#[test]
fn named_index_attributes_still_parse() {
    let input: DeriveInput = parse_quote! {
        #[index(name = "idx_reports_owner", columns = "owner_id, name")]
        #[unique_index("name")]
        struct Report {
            #[tideorm(primary_key)]
            id: i64,
            owner_id: i64,
            name: String,
        }
    };

    let (indexes, unique_indexes) = parse_index_attributes(&input.attrs);

    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].name.as_deref(), Some("idx_reports_owner"));
    assert_eq!(indexes[0].columns, vec!["owner_id", "name"]);
    assert_eq!(unique_indexes.len(), 1);
    assert!(unique_indexes[0].unique);

    build_context_for(&input).expect("named index definitions should be accepted");
}

#[test]
fn malformed_index_attributes_are_reported() {
    let error = build_error_for(&parse_quote! {
        #[index(name = "idx_reports_owner")]
        struct Report {
            #[tideorm(primary_key)]
            id: i64,
            owner_id: i64,
        }
    });

    assert!(error.contains("requires a 'columns' option"));
}

#[test]
fn index_attributes_reject_unknown_columns() {
    let error = build_error_for(&parse_quote! {
        #[unique_index("nickname")]
        struct Report {
            #[tideorm(primary_key)]
            id: i64,
            name: String,
        }
    });

    assert!(error.contains("#[unique_index(..)] references unknown field or column 'nickname'"));
}

#[test]
fn exported_type_aliases_all_map_to_a_column_type() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Doc {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            body: Text,
            tags: TextArray,
            counts: IntArray,
            payloads: JsonArray,
            meta: Json,
            qualified: tideorm::types::Json,
        }
    });

    assert!(
        !expanded.contains("unsupportedTideORMcolumntype"),
        "every alias exported for model fields must map to a ColumnType"
    );
}

/// The sync schema records each column's Rust type as compact source text, which
/// the macro now renders itself instead of emitting a runtime normalisation call.
#[test]
fn sync_schema_records_compact_rust_types() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Event {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            happened_at: Option<chrono::DateTime<chrono::Utc>>,
        }
    });

    assert!(expanded.contains(
        "::tideorm::sync::ColumnDef::new(\"happened_at\",\"Option<chrono::DateTime<chrono::Utc>>\")"
    ));
    assert!(expanded.contains(
        "::tideorm::sync::ColumnDef::new(\"id\",\"i64\").primary_key().auto_increment().not_null()"
    ));
    assert!(!expanded.contains("normalize_rust_type"));
    assert!(!expanded.contains("__register_for_sync"));
}

/// A `Uuid` key the caller left nil is keyed on insert, on every insert path;
/// the upsert keys the model itself, since it looks the row up by it afterwards.
#[test]
fn a_nil_uuid_key_is_keyed_on_insert() {
    let expanded = expand_model_tokens(parse_quote! {
        struct Account {
            #[tideorm(primary_key)]
            id: uuid::Uuid,
            name: String,
        }
    });
    assert!(
        insert_setters(&expanded)
            .contains("id:ActiveValue::Set(::tideorm::model::__uuid_key(self.id))"),
        "{expanded}"
    );
    assert!(expanded.contains(
        "letmutmodel=model;model.id=::tideorm::model::__uuid_key(model.id);letmodel_for_lookup=model.clone();"
    ));

    // Other keys are left as given, and their upsert needs no rebinding.
    let expanded = expand_model_tokens(parse_quote! {
        struct Setting {
            #[tideorm(primary_key)]
            key: String,
        }
    });
    assert!(!expanded.contains("__uuid_key"));
    assert!(
        expanded.contains(".map_err(::tideorm::Error::from)?;letmodel_for_lookup=model.clone();")
    );
}

#[test]
fn serde_rename_rules_match_serdes_field_rules() {
    use crate::serde_names::RenameRule;

    for (rule, key) in [
        (RenameRule::Camel, "displayName"),
        (RenameRule::Pascal, "DisplayName"),
        (RenameRule::Kebab, "display-name"),
        (RenameRule::ScreamingSnake, "DISPLAY_NAME"),
        (RenameRule::ScreamingKebab, "DISPLAY-NAME"),
        (RenameRule::Upper, "DISPLAY_NAME"),
        (RenameRule::Lower, "display_name"),
        (RenameRule::Snake, "display_name"),
    ] {
        assert_eq!(rule.apply("display_name"), key);
    }
}

/// A model that derives `Serialize` itself tells TideORM where serde writes
/// each field, so hidden attributes and relation payloads are found by key.
#[test]
fn a_user_serialize_derive_maps_fields_to_their_serde_keys() {
    let expanded = expand_model_tokens(parse_quote! {
        #[derive(serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        #[tideorm(hidden = "password_hash")]
        struct Author {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            display_name: String,
            #[serde(rename(serialize = "pw", deserialize = "password"))]
            password_hash: String,
            #[serde(skip_serializing, default)]
            internal_note: String,
            #[tideorm(has_many = "Book", foreign_key = "author_id")]
            book_list: HasMany<Book>,
        }
    });

    assert!(
        expanded.contains(
            "fnserialized_name(field:&str)->&str{matchfield{\"display_name\"=>\"displayName\",\"password_hash\"=>\"pw\",\"book_list\"=>\"bookList\",other=>other,}}"
        ),
        "{expanded}"
    );
    assert!(
        expanded
            .contains("(\"bookList\",<Bookas::tideorm::model::ModelMeta>::__strip_hidden_payload")
    );
    assert!(expanded.contains("fnset_field_json(&mutself,field:&str"));

    // TideORM's own serializer writes every field under its name.
    let expanded = expand_model_tokens(parse_quote! {
        struct Plain {
            #[tideorm(primary_key, auto_increment)]
            id: i64,
            display_name: String,
        }
    });
    assert!(!expanded.contains("fnserialized_name"));
}
