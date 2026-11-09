use super::*;

#[test]
fn each_field_reads_the_column_of_its_name() {
    let ast: DeriveInput = syn::parse_quote! {
        struct Totals { country: String, r#type: Option<i64> }
    };
    let expanded = expand(&ast).unwrap().to_string();
    for field in [
        quote!(country: row.try_get("country")?),
        quote!(r#type: row.try_get("type")?),
    ] {
        assert!(expanded.contains(&field.to_string()), "{expanded}");
    }
}

#[test]
fn only_structs_with_named_fields_derive() {
    for ast in [
        syn::parse_quote!(
            struct Pair(i64, i64);
        ),
        syn::parse_quote!(
            enum Kind {
                A,
            }
        ),
    ] {
        assert!(expand(&ast).is_err());
    }
}
