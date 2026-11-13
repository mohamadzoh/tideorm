use super::*;
use crate::orm::ColumnType;

fn column(name: &str, col_type: ColumnType, nullable: bool, primary_key: bool) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        col_type,
        nullable,
        primary_key,
        auto_increment: primary_key,
        default: None,
    }
}

fn model(name: &'static str, table: &str, rules: Rules) -> ModelOutline {
    ModelOutline {
        name,
        table: table.to_string(),
        rules,
        columns: vec![column("id", ColumnType::BigInteger, false, true)],
        primary_keys: vec!["id".to_string()],
        indexes: Vec::new(),
        relations: Vec::new(),
        protected_columns: Vec::new(),
        soft_delete: false,
        timestamps: false,
    }
}

fn owns(table: &str) -> RelationOutline {
    RelationOutline {
        field: "children",
        related: Some(table.to_string()),
        owns: true,
        foreign_keys: Vec::new(),
    }
}

#[test]
fn no_rules_break_nothing() {
    let mut user = model("app::User", "Users", Rules::NONE);
    user.columns
        .push(column("order", ColumnType::Double, true, false));
    user.relations.push(owns("Users"));
    assert!(broken_rules(&[user]).is_empty());
}

#[test]
fn counts_are_capped() {
    let rules = Rules {
        max_columns: Some(3),
        max_json_columns: Some(1),
        ..Rules::NONE
    };
    let mut user = model("app::User", "users", rules);
    user.columns
        .push(column("settings", ColumnType::Json, false, false));
    assert!(broken_rules(std::slice::from_ref(&user)).is_empty());
    user.columns
        .push(column("meta", ColumnType::JsonBinary, true, false));
    user.columns
        .push(column("name", ColumnType::Text, false, false));
    assert_eq!(
        broken_rules(&[user]),
        [
            "User has 4 columns, over the 3 max_columns allows",
            "User has 2 JSON columns, over the 1 max_json_columns allows",
        ]
    );
}

#[test]
fn names_keys_and_types_are_checked() {
    let rules = Rules {
        snake_case_names: true,
        avoid_reserved_words: true,
        max_identifier_length: Some(10),
        primary_key_types: &["uuid"],
        forbid_float_columns: true,
        ..Rules::NONE
    };
    let mut user = model("User", "UserAccounts", rules);
    user.columns
        .push(column("order", ColumnType::Float, false, false));
    let broken = broken_rules(&[user]).join("\n");
    assert!(
        broken.contains("table name `UserAccounts` is 12 bytes"),
        "{broken}"
    );
    assert!(
        broken.contains("`UserAccounts` is not lowercase snake_case"),
        "{broken}"
    );
    assert!(
        broken.contains("`order` is an SQL reserved word"),
        "{broken}"
    );
    assert!(
        broken.contains("primary key `id` is bigint, not one of uuid"),
        "{broken}"
    );
    assert!(
        broken.contains("`order` is a floating-point number"),
        "{broken}"
    );
}

#[test]
fn sensitive_columns_must_be_hidden_or_encrypted() {
    let rules = Rules {
        sensitive_fields: &["password", "token"],
        ..Rules::NONE
    };
    let mut user = model("User", "users", rules);
    user.columns
        .push(column("password_hash", ColumnType::Text, false, false));
    user.columns
        .push(column("api_token", ColumnType::Text, false, false));
    user.protected_columns.push("api_token".to_string());
    assert_eq!(
        broken_rules(&[user]),
        [
            "User's column `password_hash` looks sensitive and is neither hidden nor encrypted (sensitive_fields)"
        ]
    );
}

#[test]
fn owned_relations_nest_no_deeper_than_allowed() {
    let rules = Rules {
        max_relation_depth: Some(1),
        ..Rules::NONE
    };
    let mut user = model("User", "users", rules);
    user.relations.push(owns("posts"));
    let mut post = model("Post", "posts", Rules::NONE);
    post.relations.push(owns("comments"));
    let comment = model("Comment", "comments", Rules::NONE);
    assert_eq!(
        broken_rules(&[user.clone(), post.clone(), comment.clone()]),
        [
            "User's relations nest 2 deep (User → Post → Comment), over the 1 max_relation_depth allows"
        ]
    );

    // A relation the model does not own is no step deeper.
    post.relations[0].owns = false;
    assert!(broken_rules(&[user, post, comment]).is_empty());
}

#[test]
fn cycles_are_reported() {
    let rules = Rules {
        forbid_relation_cycles: true,
        max_relation_depth: Some(5),
        ..Rules::NONE
    };
    let mut user = model("User", "users", rules);
    user.relations.push(owns("teams"));
    let mut team = model("Team", "teams", Rules::NONE);
    team.relations.push(owns("users"));
    assert_eq!(
        broken_rules(&[user, team]),
        [
            "User's relations lead back to it: User → Team → User (forbid_relation_cycles)",
            "User's relations reach a cycle, so they nest without end (max_relation_depth)",
        ]
    );
}

#[test]
fn foreign_keys_lead_an_index() {
    let rules = Rules {
        require_foreign_key_index: true,
        ..Rules::NONE
    };
    let mut user = model("User", "users", Rules::NONE);
    user.relations.push(RelationOutline {
        foreign_keys: vec![("posts".to_string(), "user_id")],
        ..owns("posts")
    });
    let mut post = model("Post", "posts", rules);
    post.columns
        .push(column("user_id", ColumnType::BigInteger, false, false));
    assert_eq!(
        broken_rules(&[user.clone(), post.clone()]),
        ["Post's foreign key `user_id` leads no index (require_foreign_key_index)"]
    );

    // An index whose first column is the key covers it; a later column does not.
    post.indexes.push(IndexDefinition::new(
        "posts_title_user",
        vec!["title".into(), "user_id".into()],
        false,
    ));
    assert_eq!(broken_rules(&[user.clone(), post.clone()]).len(), 1);
    post.indexes.push(IndexDefinition::new(
        "posts_user",
        vec!["user_id".into()],
        false,
    ));
    assert!(broken_rules(&[user, post]).is_empty());
}

#[test]
fn rule_scopes_check_what_they_say() {
    assert!(RuleScope::Developer.checks_rows() && RuleScope::Developer.checks_shape());
    assert!(!RuleScope::RowsChecked.checks_rows() && RuleScope::RowsChecked.checks_shape());
    assert!(!RuleScope::Relation.checks_shape() && RuleScope::Relation.checks_raw_sql());
    assert!(!RuleScope::Internal.checks_raw_sql());
}

#[test]
fn short_names_drop_the_module_path() {
    assert_eq!(short_name("app::models::User"), "User");
    assert_eq!(short_name("User"), "User");
}

#[test]
fn index_names_keys_and_flags_are_checked() {
    let rules = Rules {
        max_identifier_length: Some(8),
        avoid_reserved_words: true,
        forbid_composite_primary_keys: true,
        require_timestamps: true,
        require_soft_delete: true,
        snake_case_names: true,
        ..Rules::NONE
    };
    let mut user = model("User", "app.users", rules);
    user.columns
        .push(column("ORDER", ColumnType::Text, false, false));
    user.primary_keys.push("tenant_id".to_string());
    user.indexes.push(IndexDefinition::new(
        "users_email_index",
        vec!["email".into()],
        false,
    ));
    let broken = broken_rules(std::slice::from_ref(&user)).join("\n");
    for expected in [
        "User's index name `users_email_index` is 17 bytes, over the 8 max_identifier_length allows",
        "User's column name `ORDER` is an SQL reserved word (avoid_reserved_words)",
        "User's column name `ORDER` is not lowercase snake_case (snake_case_names)",
        "User's primary key spans 2 columns (forbid_composite_primary_keys)",
        "User has no created_at/updated_at timestamps (require_timestamps)",
        "User does not soft delete (require_soft_delete)",
    ] {
        assert!(
            broken.contains(expected),
            "missing `{expected}` in:\n{broken}"
        );
    }
    // The schema is not part of the table's name.
    assert!(!broken.contains("app.users"), "{broken}");

    user.columns.pop();
    user.primary_keys.pop();
    user.indexes.clear();
    user.timestamps = true;
    user.soft_delete = true;
    assert!(broken_rules(&[user]).is_empty());
}

#[test]
fn depth_follows_the_longest_owned_chain() {
    let rules = Rules {
        max_relation_depth: Some(2),
        forbid_relation_cycles: true,
        ..Rules::NONE
    };
    // A owns B and C, both own D, and D owns E: 3 deep along either branch.
    let mut a = model("A", "a", rules);
    a.relations.extend([owns("b"), owns("c")]);
    let mut b = model("B", "b", Rules::NONE);
    b.relations.push(owns("d"));
    let mut c = model("C", "c", Rules::NONE);
    c.relations.push(owns("d"));
    let mut d = model("D", "d", Rules::NONE);
    d.relations.push(owns("e"));
    let e = model("E", "e", Rules::NONE);
    assert_eq!(
        broken_rules(&[a, b, c, d, e]),
        ["A's relations nest 3 deep (A → B → D → E), over the 2 max_relation_depth allows"]
    );
}

#[test]
fn a_cycle_below_a_model_is_endless_but_does_not_lead_back() {
    let rules = Rules {
        max_relation_depth: Some(5),
        forbid_relation_cycles: true,
        ..Rules::NONE
    };
    let mut a = model("A", "a", rules);
    a.relations.push(owns("b"));
    let mut b = model("B", "b", Rules::NONE);
    b.relations.push(owns("c"));
    let mut c = model("C", "c", Rules::NONE);
    c.relations.push(owns("b"));
    assert_eq!(
        broken_rules(&[a, b, c]),
        ["A's relations reach a cycle, so they nest without end (max_relation_depth)"]
    );
}

#[test]
fn self_references_and_unknown_tables_are_left_alone() {
    let rules = Rules {
        max_relation_depth: Some(0),
        forbid_relation_cycles: true,
        require_foreign_key_index: true,
        ..Rules::NONE
    };
    let mut category = model("Category", "categories", rules);
    category.relations.push(owns("categories"));
    category.relations.push(RelationOutline {
        foreign_keys: vec![("archived_posts".to_string(), "category_id")],
        ..owns("archived_posts")
    });
    assert!(broken_rules(&[category]).is_empty());
}

#[tideorm::model(table = "outline_teams", hidden = "pass_code")]
struct OutlineTeam {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "secret_pass")]
    pass_code: String,
    #[tideorm(has_many = "OutlineMember", foreign_key = "team_id")]
    members: crate::relations::HasMany<OutlineMember>,
}

#[tideorm::model(table = "outline_members")]
struct OutlineMember {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "team_ref")]
    team_id: i64,
    #[tideorm(belongs_to = "OutlineTeam", foreign_key = "team_id")]
    team: crate::relations::BelongsTo<OutlineTeam>,
}

#[test]
fn a_derived_model_outlines_its_columns_and_relations() {
    let team = __outline::<OutlineTeam>();
    assert!(team.name.ends_with("OutlineTeam"));
    assert_eq!(team.table, "outline_teams");
    assert_eq!(team.rules, Rules::NONE);
    assert_eq!(team.primary_keys, ["id"]);
    // A hidden field is protected by its column's name.
    assert_eq!(team.protected_columns, ["secret_pass"]);
    // A foreign key named by its field is read as its column.
    assert_eq!(
        team.relations,
        [RelationOutline {
            field: "members",
            related: Some("outline_members".to_string()),
            owns: true,
            foreign_keys: vec![("outline_members".to_string(), "team_ref")],
        }]
    );
    let member = __outline::<OutlineMember>();
    assert_eq!(
        member.relations,
        [RelationOutline {
            field: "team",
            related: Some("outline_teams".to_string()),
            owns: false,
            foreign_keys: vec![("outline_members".to_string(), "team_ref")],
        }]
    );
}

#[test]
fn models_on_one_table_share_its_indexes_and_relations() {
    let rules = Rules {
        require_foreign_key_index: true,
        max_relation_depth: Some(1),
        ..Rules::NONE
    };
    let mut user = model("User", "users", rules);
    user.relations.push(RelationOutline {
        foreign_keys: vec![("posts".to_string(), "user_id")],
        ..owns("posts")
    });
    // A read model of `posts` declaring no index, listed before the model that does.
    let mut summary = model("PostSummary", "posts", rules);
    summary.relations.push(owns("comments"));
    let mut post = model("Post", "posts", rules);
    post.indexes.push(IndexDefinition::new(
        "posts_user",
        vec!["user_id".into()],
        false,
    ));
    let comment = model("Comment", "comments", rules);
    assert_eq!(
        broken_rules(&[user, summary, post, comment]),
        [
            "User's relations nest 2 deep (User → PostSummary → Comment), over the 1 max_relation_depth allows"
        ]
    );
}
