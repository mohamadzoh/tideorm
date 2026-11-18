use super::*;
use quote::quote;

fn renamed(tokens: TokenStream) -> String {
    rename(tokens, "orm").to_string()
}

fn manifest(text: &str) -> toml::Table {
    text.parse().expect("a manifest")
}

#[test]
fn paths_starting_with_the_crate_are_renamed() {
    let tokens = quote! {
        use ::tideorm::prelude::*;
        impl<'de> ::tideorm::serde::Deserialize<'de> for X {}
        fn f(x: &'a ::tideorm::Value) -> ::tideorm::Result<Vec<::tideorm::Row>> {
            let v = x as ::tideorm::Value;
            match v { _ => ::tideorm::db() }
        }
        #[derive(::tideorm::Model)]
        struct S where T: ::tideorm::ModelMeta;
    };
    let text = renamed(tokens);
    assert!(!text.contains("tideorm"), "{text}");
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert_eq!(compact.matches("::orm::").count(), 9, "{text}");
}

#[test]
fn other_names_spelt_tideorm_stay() {
    let tokens = quote! {
        #[tideorm(primary_key)]
        use crate::tideorm::x;
        let y = module::tideorm::z + self.tideorm;
        fn tideorm() {}
    };
    let before = tokens.to_string();
    assert_eq!(renamed(tokens), before);
}

#[test]
fn the_manifest_names_the_dependency() {
    let plain = manifest("[dependencies]\ntideorm = \"0.17\"\n");
    assert_eq!(renamed_in(&plain, || None), None);

    let renamed =
        manifest("[dependencies]\nmy-orm = { package = \"tideorm\", version = \"0.17\" }\n");
    assert_eq!(renamed_in(&renamed, || None).as_deref(), Some("my_orm"));

    let target = manifest("[target.'cfg(unix)'.dependencies]\norm = { package = \"tideorm\" }\n");
    assert_eq!(renamed_in(&target, || None).as_deref(), Some("orm"));

    let dev = manifest("[dev-dependencies]\norm = { package = \"tideorm\" }\n");
    assert_eq!(renamed_in(&dev, || None).as_deref(), Some("orm"));

    // Both names: `::tideorm` resolves, nothing to rename.
    let both = manifest(
        "[dependencies]\norm = { package = \"tideorm\" }\n[dev-dependencies]\ntideorm = \"0.17\"\n",
    );
    assert_eq!(renamed_in(&both, || None), None);

    let other = manifest("[dependencies]\ntideorm = { package = \"something-else\" }\n");
    assert_eq!(renamed_in(&other, || None), None);
}

#[test]
fn an_inherited_dependency_is_named_by_the_workspace() {
    let member =
        manifest("[dependencies]\norm = { workspace = true }\nserde = { workspace = true }\n");
    let workspace = || {
        Some(manifest(
            "orm = { package = \"tideorm\", version = \"0.17\" }\nserde = \"1\"\n",
        ))
    };
    assert_eq!(renamed_in(&member, workspace).as_deref(), Some("orm"));

    let unrenamed = manifest("[dependencies]\ntideorm = { workspace = true }\n");
    assert_eq!(
        renamed_in(&unrenamed, || Some(manifest("tideorm = \"0.17\"\n"))),
        None
    );
}
