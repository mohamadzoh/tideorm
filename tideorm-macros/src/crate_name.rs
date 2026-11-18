//! The name the crate being compiled gives the `tideorm` dependency.
//!
//! The generated code names the runtime by absolute `::tideorm::` paths. A
//! crate that renames the dependency (`orm = { package = "tideorm", .. }`)
//! has no crate of that name, so each macro's output has those paths
//! rewritten to the name its `Cargo.toml` uses.

use std::path::Path;

use proc_macro2::{Group, Ident, Spacing, TokenStream, TokenTree};

/// `tokens` with every path that starts `::tideorm` started with the name the
/// crate being compiled gives the dependency, when that is another name.
pub(crate) fn resolve(tokens: TokenStream) -> TokenStream {
    match renamed() {
        Some(name) => rename(tokens, &name),
        None => tokens,
    }
}

/// Whether `name` is what the crate being compiled calls the dependency.
pub(crate) fn names_tideorm(name: &str) -> bool {
    name == "tideorm" || renamed().is_some_and(|renamed| renamed == name)
}

/// The name the manifest in `CARGO_MANIFEST_DIR` gives the dependency, if it
/// is not `tideorm`. None for a crate that does not depend on it by name,
/// such as tideorm itself (`extern crate self as tideorm`).
fn renamed() -> Option<String> {
    let dir = std::env::var_os("CARGO_MANIFEST_DIR")?;
    let manifest = read_manifest(&Path::new(&dir).join("Cargo.toml"))?;
    renamed_in(&manifest, || {
        workspace_dependencies(Path::new(&dir), &manifest)
    })
}

fn read_manifest(path: &Path) -> Option<toml::Table> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

/// The name `manifest` gives the dependency, if not `tideorm`. `workspace`
/// gives the workspace's `[workspace.dependencies]`, read only for an entry
/// that inherits from it.
pub(crate) fn renamed_in(
    manifest: &toml::Table,
    workspace: impl FnOnce() -> Option<toml::Table>,
) -> Option<String> {
    let mut workspace = Some(workspace);
    let mut inherited = None;
    let mut found = None;
    for (key, spec) in dependency_tables(manifest).into_iter().flatten() {
        let package = match spec.get("package").and_then(toml::Value::as_str) {
            Some(package) => package,
            None if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) => {
                let table = inherited.get_or_insert_with(|| {
                    workspace.take().and_then(|read| read()).unwrap_or_default()
                });
                table
                    .get(key)
                    .and_then(|spec| spec.get("package"))
                    .and_then(toml::Value::as_str)
                    .unwrap_or(key)
            }
            None => key,
        };
        if package == "tideorm" {
            if key == "tideorm" {
                return None;
            }
            found.get_or_insert_with(|| key.replace('-', "_"));
        }
    }
    found
}

/// `[dependencies]` and each `[target.<cfg>.dependencies]`, then the dev
/// dependencies the same way: the first rename found wins.
fn dependency_tables(manifest: &toml::Table) -> Vec<&toml::Table> {
    let targets: Vec<&toml::Table> = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values().filter_map(toml::Value::as_table))
        .collect();
    let mut tables = Vec::new();
    for section in ["dependencies", "dev-dependencies"] {
        tables.extend(
            std::iter::once(manifest)
                .chain(targets.iter().copied())
                .filter_map(|table| table.get(section).and_then(toml::Value::as_table)),
        );
    }
    tables
}

/// `[workspace.dependencies]` of the workspace `manifest` belongs to: its own,
/// the one `package.workspace` points at, or the nearest manifest above with a
/// `[workspace]`.
fn workspace_dependencies(dir: &Path, manifest: &toml::Table) -> Option<toml::Table> {
    let dependencies = |root: &toml::Table| {
        root.get("workspace")?
            .get("dependencies")?
            .as_table()
            .cloned()
    };
    if manifest.contains_key("workspace") {
        return dependencies(manifest);
    }
    if let Some(path) = manifest
        .get("package")
        .and_then(|package| package.get("workspace"))
        .and_then(toml::Value::as_str)
    {
        return dependencies(&read_manifest(&dir.join(path).join("Cargo.toml"))?);
    }
    dir.ancestors()
        .skip(1)
        .filter_map(|dir| read_manifest(&dir.join("Cargo.toml")))
        .find(|root| root.contains_key("workspace"))
        .and_then(|root| dependencies(&root))
}

/// `tokens` with the `tideorm` of every path starting `::tideorm` replaced by
/// `name`, inside groups too; a `tideorm` after another segment
/// (`crate::tideorm`) is a name of the user's and stays.
pub(crate) fn rename(tokens: TokenStream, name: &str) -> TokenStream {
    let mut out: Vec<TokenTree> = Vec::new();
    for tree in tokens {
        let tree = match tree {
            TokenTree::Group(group) => {
                let mut renamed = Group::new(group.delimiter(), rename(group.stream(), name));
                renamed.set_span(group.span());
                TokenTree::Group(renamed)
            }
            TokenTree::Ident(ident) if ident == "tideorm" && starts_path(&out) => {
                TokenTree::Ident(Ident::new(name, ident.span()))
            }
            tree => tree,
        };
        out.push(tree);
    }
    out.into_iter().collect()
}

/// Whether `before` ends with a `::` that starts a path: no segment of one
/// comes before it.
fn starts_path(before: &[TokenTree]) -> bool {
    let colon = |tree: &TokenTree, spacing| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ':' && punct.spacing() == spacing);
    let [rest @ .., first, second] = before else {
        return false;
    };
    if !colon(first, Spacing::Joint) || !colon(second, Spacing::Alone) {
        return false;
    }
    // After a segment (`crate::tideorm`, `module::tideorm`) the name is the
    // user's; a keyword or a lifetime (`&'a ::tideorm::X`) ends before a path.
    match rest.split_last() {
        Some((TokenTree::Ident(ident), earlier)) => {
            matches!(earlier.last(), Some(TokenTree::Punct(punct)) if punct.as_char() == '\'')
                || KEYWORDS.contains(&ident.to_string().as_str())
        }
        _ => true,
    }
}

/// The keywords a path can follow; `crate`, `self`, `super` and `Self` start
/// a path of their own, so a `::tideorm` after them is one of its segments.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "do", "dyn", "else", "enum",
    "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "type", "unsafe", "use", "where", "while",
    "yield",
];

#[cfg(test)]
#[path = "../tests/unit/crate_name_tests.rs"]
mod tests;
