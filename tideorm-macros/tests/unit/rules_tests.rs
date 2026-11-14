use super::{Limits, ModelRules, parse_rules};

fn parsed(text: &str, model: &str) -> ModelRules {
    parse_rules(text, model).expect("valid rules")
}

/// The query rules' `Rules` tokens, spaces removed.
fn rules_of(text: &str, model: &str) -> Option<String> {
    parsed(text, model)
        .query_rules
        .map(|tokens| tokens.to_string().replace(' ', ""))
}

fn refusal(text: &str) -> String {
    match parse_rules(text, "User") {
        Ok(_) => panic!("rules accepted: {text}"),
        Err(message) => message,
    }
}

#[test]
fn a_file_without_rules_sets_none() {
    assert_eq!(rules_of("[project]\nname = \"app\"\n", "User"), None);
    assert_eq!(rules_of("", "User"), None);
    assert_eq!(parsed("", "User").limits, Limits::default());
}

#[test]
fn rules_are_written_with_the_rest_left_unset() {
    let rules = rules_of(
        "[rules]\nmax_columns = 20\nmax_rows = 500\nforbid_raw_sql = true\nsensitive_fields = [\"Password\"]\n",
        "User",
    )
    .unwrap();
    assert!(rules.contains("max_rows:::core::option::Option::Some(500u64)"));
    assert!(rules.contains("forbid_raw_sql:true"));
    assert!(rules.ends_with("..::tideorm::rules::Rules::NONE}"));
    // Model rules are checked as the model compiles, not written for queries.
    assert!(!rules.contains("max_columns"));
    let limits = parsed(
        "[rules]\nmax_columns = 20\nsensitive_fields = [\"Password\"]\n",
        "User",
    )
    .limits;
    assert_eq!(limits.max_columns, Some(20));
    assert_eq!(limits.sensitive_fields, ["password"]);
}

#[test]
fn a_false_flag_applies_nothing() {
    let rules = parsed(
        "[rules]\nrequire_timestamps = false\nforbid_raw_sql = false\n",
        "User",
    );
    assert!(rules.query_rules.is_none());
    assert!(!rules.forbids_raw_sql);
    assert_eq!(rules.limits, Limits::default());
}

#[test]
fn exceptions_exempt_only_the_named_model() {
    let text = "[rules]\nmax_columns = 20\nmax_joins = 2\n\n[rules.allow]\nLegacyUser = [\"max_columns\"]\n";
    let legacy = parsed(text, "LegacyUser");
    assert_eq!(legacy.limits.max_columns, None);
    assert!(
        legacy
            .query_rules
            .unwrap()
            .to_string()
            .contains("max_joins")
    );
    assert_eq!(parsed(text, "User").limits.max_columns, Some(20));
}

#[test]
fn a_model_exempt_from_every_rule_has_none() {
    let text = "[rules]\nmax_columns = 20\nmax_rows = 9\n\n[rules.allow]\nUser = [\"max_columns\", \"max_rows\"]\n";
    let rules = parsed(text, "User");
    assert!(rules.query_rules.is_none());
    assert_eq!(rules.limits, Limits::default());
}

#[test]
fn raw_sql_stays_forbidden_for_the_project_when_a_model_is_exempt() {
    let text = "[rules]\nforbid_raw_sql = true\n\n[rules.allow]\nReport = [\"forbid_raw_sql\"]\n";
    let report = parse_rules(text, "Report").unwrap();
    assert!(report.query_rules.is_none());
    assert!(report.forbids_raw_sql);
}

#[test]
fn mistakes_are_refused() {
    assert!(refusal("[rules]\nmax_colums = 3\n").contains("names no rule `max_colums`"));
    assert!(
        refusal("[rules]\nmax_columns = -1\n").contains("`max_columns` must be a whole number")
    );
    assert!(refusal("[rules]\nmax_rows = 0\n").contains("1 or more"));
    assert!(refusal("[rules]\nforbid_raw_sql = \"yes\"\n").contains("true or false"));
    assert!(
        refusal("[rules]\nprimary_key_types = [\"serial\"]\n").contains("no key type `serial`")
    );
    assert!(refusal("[rules]\nsensitive_fields = []\n").contains("one or more strings"));
    assert!(refusal("rules = 3\n").contains("must be a table"));
    assert!(refusal("[rules]\n[rules.allow]\nUser = \"max_rows\"\n").contains("must be a list"));
    assert!(
        refusal("[rules]\n[rules.allow]\nOther = [\"max_rowz\"]\n").contains("no rule `max_rowz`")
    );
    assert!(refusal("[rules\n").contains("is not valid TOML"));
}

#[test]
fn key_types_are_read_in_any_case() {
    let limits = parsed(
        "[rules]\nprimary_key_types = [\"BigInt\", \"uuid\"]\n",
        "User",
    )
    .limits;
    assert_eq!(limits.primary_key_types, ["bigint", "uuid"]);
}

#[test]
fn values_out_of_shape_are_refused() {
    assert!(refusal("[rules]\nmax_columns = 4294967296\n").contains("0 or more"));
    assert!(refusal("[rules]\nmax_columns = 3.0\n").contains("whole number"));
    assert!(refusal("[rules]\nmax_rows = \"10\"\n").contains("1 or more"));
    assert!(refusal("[rules]\nsensitive_fields = [1]\n").contains("non-empty strings"));
    assert!(refusal("[rules]\nsensitive_fields = [\"\"]\n").contains("non-empty strings"));
    assert!(refusal("[rules]\nsensitive_fields = \"password\"\n").contains("list"));
    assert!(refusal("[rules]\n[rules.max]\n").contains("names no rule `max`"));
    assert!(refusal("[rules]\n[rules.allow]\nUser = [3]\n").contains("as strings"));
}

#[test]
fn limits_of_zero_and_empty_exceptions_are_kept() {
    let rules = rules_of(
        "[rules]\nmax_joins = 0\n\n[rules.allow]\nUser = []\n",
        "User",
    )
    .unwrap();
    assert!(rules.contains("max_joins:::core::option::Option::Some(0u32)"));
}

#[test]
fn a_mistake_in_another_models_exception_is_still_reported() {
    let text = "[rules]\nmax_rows = 10\n\n[rules.allow]\nOther = [\"max_row\"]\n";
    assert!(parse_rules(text, "User").is_err());
}

#[test]
fn dependencies_cargo_fetched_read_no_rules() {
    use std::path::Path;
    let fetched = |dir: &str| super::fetched_by_cargo(Path::new(dir));
    assert!(fetched(
        "/home/dev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/shop-models-1.0.0"
    ));
    assert!(fetched(
        "/usr/local/cargo/git/checkouts/shop-models-3f2a/1c9e0d4"
    ));
    assert!(!fetched("/home/dev/projects/shop/models"));
    assert!(!fetched("/home/dev/src/registry"));
}
