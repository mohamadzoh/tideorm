//! Derive inputs that must expand, or fail with a compile error rather than a
//! panic: the derives' own checks, read straight off the expansion.

use super::attributes::ValueTypeAttrs;
use super::{
    EngineField, expand_active_model, expand_column, expand_derive_value_type, expand_model,
    expand_primary_key,
};
use crate::orm::enum_iter;
use proc_macro2::{Ident, Span, TokenStream};
use syn::{DeriveInput, Type, parse_str};

fn input(code: &str) -> DeriveInput {
    parse_str(code).expect("parse the derive input")
}

/// Whitespace-free rendering, to look for a fragment of the expansion.
fn compact(tokens: &TokenStream) -> String {
    tokens.to_string().split_whitespace().collect()
}

fn ident(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}

#[test]
fn a_model_reads_its_columns_and_defaults_its_other_fields() {
    let kind = Ident::new_raw("type", Span::call_site());
    let (name, posts) = (ident("name"), ident("posts"));
    let (kind_variant, name_variant) = (ident("Type"), ident("Label"));
    let ty: Type = parse_str("String").unwrap();
    let fields = [
        EngineField {
            ident: &kind,
            ty: &ty,
            column: Some(&kind_variant),
            encrypted: false,
        },
        EngineField {
            ident: &name,
            ty: &ty,
            column: Some(&name_variant),
            encrypted: false,
        },
        EngineField {
            ident: &posts,
            ty: &ty,
            column: None,
            encrypted: false,
        },
    ];
    let tokens = compact(&expand_model(&ident("Cake"), &fields));
    assert!(tokens.contains("Column::Type=>self.r#type"), "{tokens}");
    assert!(tokens.contains("Column::Label=>self.name"), "{tokens}");
    assert!(tokens.contains("posts:Default::default()"), "{tokens}");
    assert!(!tokens.contains("self.posts"), "{tokens}");
}

/// An encrypted column holds its payload's text: a row reads it as text and
/// decrypts it into the field's type, and the active model holds the text,
/// which only `try_into_active_model` seals, never `From<Model>`.
#[test]
fn an_encrypted_column_holds_text_and_decodes_into_its_field() {
    let (pin, variant) = (ident("pin"), ident("Pin"));
    let ty: Type = parse_str("u32").unwrap();
    let fields = [EngineField {
        ident: &pin,
        ty: &ty,
        column: Some(&variant),
        encrypted: true,
    }];
    let tokens = compact(&expand_model(&ident("Cake"), &fields));
    assert!(
        tokens.contains("letpin=::tideorm::orm::__read_model_field::<::std::string::String>("),
        "{tokens}"
    );
    assert!(
        tokens.contains("pin:::tideorm::model::__decrypt_model_field::<Self,u32>(pin,\"pin\","),
        "{tokens}"
    );
    assert!(
        tokens.contains("Column::Pin=>::tideorm::model::__encrypted_field_value(&self.pin)"),
        "{tokens}"
    );

    let (active_model, impls) = expand_active_model(&TokenStream::new(), &ident("Cake"), &fields);
    let active_model = compact(&active_model);
    assert!(
        active_model.contains(
            "pubpin:::tideorm::orm::ActiveValue<::core::option::Option<::std::string::String>>"
        ),
        "{active_model}"
    );
    let impls = compact(&impls);
    assert!(
        impls.contains("fnfrom(m:Cake)->Self{Self{pin:::tideorm::orm::ActiveValue::NotSet}}"),
        "{impls}"
    );
    assert!(
        impls.contains(
            "pin:::tideorm::model::__decrypt_model_field::<Self,_>(a.pin.unwrap(),\"pin\","
        ),
        "{impls}"
    );
}

#[test]
fn a_column_enum_names_its_columns() {
    let variants = [ident("Id"), ident("S3Key")];
    let names = ["id".to_owned(), "s_3_key".to_owned()];
    let tokens = compact(&expand_column(&ident("Column"), &variants, &names));
    assert!(
        tokens.contains("Self::Id=>\"id\",Self::S3Key=>\"s_3_key\""),
        "{tokens}"
    );
    assert!(tokens.contains("fnquoted"), "{tokens}");
    assert!(
        tokens.contains("constALL:&'static[Self]=&[Column::Id,Column::S3Key];"),
        "{tokens}"
    );
    // A name that needs quoting is quoted when the statement renders.
    let names = ["id".to_owned(), "s3 key".to_owned()];
    let tokens = compact(&expand_column(&ident("Column"), &variants, &names));
    assert!(!tokens.contains("fnquoted"), "{tokens}");

    let tokens = compact(&expand_primary_key(&variants[..1], &names[..1]));
    assert!(
        tokens.contains("Self::Column::Id=>Some(Self::Id),_=>None"),
        "{tokens}"
    );
}

#[test]
fn every_orm_attribute_is_read() {
    let wrapped = input(
        "#[orm(value_type = \"String\")]
        #[orm(from_str = \"parse\", to_str = \"render\")]
        pub enum Tag { A }",
    );
    let attr = ValueTypeAttrs::try_from_attributes(&wrapped.attrs)
        .unwrap()
        .unwrap();
    assert!(attr.value_type.is_some() && attr.from_str.is_some() && attr.to_str.is_some());
    assert!(
        ValueTypeAttrs::try_from_attributes(&input("struct Plain(u8);").attrs)
            .unwrap()
            .is_none()
    );

    let unknown = input("#[orm(value_type = \"String\")] #[orm(valu_type = \"x\")] enum Tag { A }");
    assert!(ValueTypeAttrs::try_from_attributes(&unknown.attrs).is_err());
}

#[test]
fn a_wrapped_value_type_keeps_its_tokens() {
    let tokens = expand_derive_value_type(input("pub struct Count(<S as Tr>::A);"))
        .expect("expand")
        .to_string();
    assert!(
        tokens.contains(
            "< < S as Tr > :: A as :: tideorm :: orm :: sql_builder :: ValueType > :: array_type ()"
        ),
        "{tokens}"
    );
    let tokens = compact(&expand_derive_value_type(input("pub struct Count(u32);")).unwrap());
    assert!(tokens.contains("TryFromU64"), "{tokens}");
    // No `u64` converts into an `Option`, however the `Option` is written.
    for wrapped in ["Option<u32>", "std::option::Option<u32>"] {
        let code = format!("pub struct Count({wrapped});");
        let tokens = compact(&expand_derive_value_type(input(&code)).unwrap());
        assert!(!tokens.contains("TryFromU64"), "{tokens}");
    }
}

#[test]
fn enum_iter_lists_unit_variants_and_refuses_anything_else() {
    let tokens = compact(&enum_iter::expand(&input("enum Column { A, Table }")).unwrap());
    assert!(
        tokens.contains("constALL:&'static[Self]=&[Column::A,Column::Table];"),
        "{tokens}"
    );
    for code in ["enum Tag { A(u8) }", "enum Tag<T> { A }", "struct Tag;"] {
        assert!(enum_iter::expand(&input(code)).is_err(), "{code}");
    }
}
