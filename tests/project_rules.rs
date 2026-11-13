//! The project rules of `tideorm.toml` end to end: `tests/project_rules/` is a
//! crate with a `[rules]` table, copied into the target directory, pointed at
//! this checkout and built by cargo, which is the only way to run the derive
//! against a rules file. Its `startup` binary holds models that break the
//! model rules, its `queries` binary checks each query rule itself, and a copy
//! with a mistake in the table must fail to compile.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/project_rules");

/// A copy of the fixture under `name`, its rules file holding `rules`.
fn project(name: &str, rules: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("project_rules");
    let dir = root.join(name);
    let bin = dir.join("src/bin");
    std::fs::create_dir_all(&bin).expect("create the project");
    for file in ["startup.rs", "queries.rs"] {
        std::fs::copy(
            Path::new(FIXTURE).join("src/bin").join(file),
            bin.join(file),
        )
        .expect("copy a binary");
    }
    std::fs::write(dir.join("tideorm.toml"), rules).expect("write the rules");
    let manifest = format!(
        "[package]\nname = \"project-rules\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [dependencies]\n\
         tideorm = {{ path = '{}', default-features = false, features = [\"sqlite\", \"runtime-tokio\"] }}\n\
         tokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}\n\n\
         [workspace]\n",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("write the manifest");
    // The checkout's lockfile, so the copy builds the versions it does.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .expect("copy the lockfile");
    dir
}

fn cargo(dir: &Path, args: &[&str]) -> Output {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("project_rules/target");
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(args)
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .expect("run cargo")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn rules() -> String {
    std::fs::read_to_string(Path::new(FIXTURE).join("tideorm.toml")).expect("read the rules")
}

#[test]
fn models_and_queries_are_held_to_the_project_rules() {
    let dir = project("valid", &rules());

    let startup = cargo(&dir, &["run", "--quiet", "--bin", "startup"]);
    let printed = text(&startup);
    assert!(startup.status.success(), "{printed}");
    for broken in [
        "Account has 6 columns, over the 5 max_columns allows",
        "Account has 2 JSON columns, over the 1 max_json_columns allows",
        "Account's column `password` looks sensitive and is neither hidden nor encrypted (sensitive_fields)",
        "Token's primary key `id` is string, not one of bigint (primary_key_types)",
        "BadName's table name `BadNames` is not lowercase snake_case (snake_case_names)",
        "Author's relations nest 2 deep (Author → Book → Chapter), over the 1 max_relation_depth allows",
        "Team's relations lead back to it: Team → Member → Team (forbid_relation_cycles)",
        "Member's relations lead back to it: Member → Team → Member (forbid_relation_cycles)",
        "Team's relations reach a cycle, so they nest without end (max_relation_depth)",
        "Chapter's foreign key `book_id` leads no index (require_foreign_key_index)",
    ] {
        assert!(
            printed.contains(broken),
            "missing `{broken}` in:\n{printed}"
        );
    }
    assert!(!printed.contains("Legacy"), "Legacy is exempt:\n{printed}");
    assert!(!printed.contains("connected"), "{printed}");

    let queries = cargo(&dir, &["run", "--quiet", "--bin", "queries"]);
    let printed = text(&queries);
    assert!(queries.status.success(), "{printed}");
    assert!(printed.contains("all project rules held"), "{printed}");
}

#[test]
fn a_mistake_in_the_rules_fails_the_build() {
    let rules = rules().replace("max_columns = 5", "max_colums = 5");
    let dir = project("mistake", &rules);
    let build = cargo(&dir, &["build", "--bin", "queries"]);
    let printed = text(&build);
    assert!(!build.status.success(), "{printed}");
    assert!(
        printed.contains("names no rule `max_colums`; the rules are max_columns"),
        "{printed}"
    );
}
