use super::message::Message;
use super::*;
use crate::orm::sql_builder::ColumnKind;

const fn node(
    id: u64,
    name: &'static str,
    table: &'static str,
    owned: &'static [&'static ModelNode],
) -> ModelNode {
    ModelNode {
        id,
        name,
        table,
        owned,
        leading_columns: &["id"],
        indexes_foreign_keys: true,
    }
}

static AUTHOR: ModelNode = node(1, "Author", "authors", &[&BOOK]);
static BOOK: ModelNode = node(2, "Book", "books", &[&CHAPTER]);
static CHAPTER: ModelNode = node(3, "Chapter", "chapters", &[]);

static TEAM: ModelNode = node(4, "Team", "teams", &[&MEMBER]);
static MEMBER: ModelNode = node(5, "Member", "members", &[&TEAM]);
static LEAGUE: ModelNode = node(6, "League", "leagues", &[&TEAM]);

/// A tree: its children are rows of its own table, and so are a read
/// model's, which shares the table.
static CATEGORY: ModelNode = node(7, "Category", "categories", &[&CATEGORY, &CATEGORY_VIEW]);
static CATEGORY_VIEW: ModelNode = node(8, "CategoryView", "categories", &[]);

/// Two chains meeting at one model.
static SHOP: ModelNode = node(9, "Shop", "shops", &[&ORDER, &CART]);
static ORDER: ModelNode = node(10, "Order", "orders", &[&LINE]);
static CART: ModelNode = node(11, "Cart", "carts", &[&LINE]);
static LINE: ModelNode = node(12, "Line", "lines", &[]);

// The checks run in a `const`, as the derive's do.
const _: () = assert!(__check_relation_depth(&AUTHOR, 2).is_none());
const _: () = assert!(__check_relation_cycles(&AUTHOR).is_none());
const _: () = assert!(__check_foreign_key(&BOOK, "id").is_none());

fn text(message: Option<Message>) -> Option<String> {
    message.map(|message| message.as_str().to_string())
}

#[test]
fn owned_relations_nest_no_deeper_than_allowed() {
    assert_eq!(text(__check_relation_depth(&AUTHOR, 2)), None);
    assert_eq!(
        text(__check_relation_depth(&AUTHOR, 1)).as_deref(),
        Some(
            "Author's relations nest 2 deep (Author → Book → Chapter), over the limit of 1 \
             (max_relation_depth in tideorm.toml)"
        )
    );
    assert_eq!(
        text(__check_relation_depth(&SHOP, 1)).as_deref(),
        Some(
            "Shop's relations nest 2 deep (Shop → Order → Line), over the limit of 1 \
             (max_relation_depth in tideorm.toml)"
        )
    );
}

#[test]
fn cycles_are_reported_from_every_model_on_them() {
    assert_eq!(
        text(__check_relation_cycles(&TEAM)).as_deref(),
        Some(
            "Team's relations lead back to it: Team → Member → Team \
             (forbid_relation_cycles in tideorm.toml)"
        )
    );
    assert_eq!(
        text(__check_relation_cycles(&MEMBER)).as_deref(),
        Some(
            "Member's relations lead back to it: Member → Team → Member \
             (forbid_relation_cycles in tideorm.toml)"
        )
    );
    assert_eq!(
        text(__check_relation_depth(&TEAM, 9)).as_deref(),
        Some(
            "Team's relations reach a cycle, so they nest without end \
             (max_relation_depth in tideorm.toml)"
        )
    );
}

#[test]
fn a_cycle_below_a_model_is_endless_but_does_not_lead_back() {
    assert_eq!(text(__check_relation_cycles(&LEAGUE)), None);
    assert!(
        text(__check_relation_depth(&LEAGUE, 9))
            .unwrap()
            .contains("reach a cycle")
    );
}

#[test]
fn relations_to_the_models_own_table_are_left_alone() {
    assert_eq!(text(__check_relation_cycles(&CATEGORY)), None);
    assert_eq!(text(__check_relation_depth(&CATEGORY, 0)), None);
    assert_eq!(text(__check_relation_cycles(&SHOP)), None);
}

#[test]
fn foreign_keys_lead_an_index_where_the_holder_requires_it() {
    assert_eq!(text(__check_foreign_key(&BOOK, "id")), None);
    assert_eq!(
        text(__check_foreign_key(&BOOK, "author_id")).as_deref(),
        Some(
            "Book's foreign key `author_id` leads no index (require_foreign_key_index in tideorm.toml)"
        )
    );
    static UNRULED: ModelNode = ModelNode {
        indexes_foreign_keys: false,
        ..node(13, "Free", "frees", &[])
    };
    assert_eq!(text(__check_foreign_key(&UNRULED, "author_id")), None);
}

#[test]
fn column_rules_read_the_column_kind() {
    let kinds = [ColumnKind::Json, ColumnKind::Text, ColumnKind::Json];
    assert_eq!(text(__check_json_columns("Account", &kinds, 2)), None);
    assert_eq!(
        text(__check_json_columns("Account", &kinds, 1)).as_deref(),
        Some("Account has 2 JSON columns, over the limit of 1 (max_json_columns in tideorm.toml)")
    );
    assert_eq!(
        text(__check_float_column("Item", "price", ColumnKind::Other)),
        None
    );
    assert_eq!(
        text(__check_float_column("Item", "price", ColumnKind::Float)).as_deref(),
        Some(
            "Item's column `price` is a floating-point number; use a decimal or an integer \
             (forbid_float_columns in tideorm.toml)"
        )
    );
    assert_eq!(
        text(__check_key_type(
            "Token",
            "id",
            ColumnKind::Uuid,
            &["bigint", "uuid"]
        )),
        None
    );
    assert_eq!(
        text(__check_key_type(
            "Token",
            "id",
            ColumnKind::Text,
            &["bigint", "uuid"]
        ))
        .as_deref(),
        Some(
            "Token's primary key `id` is string, not one of bigint, uuid \
             (primary_key_types in tideorm.toml)"
        )
    );
    assert!(
        text(__check_key_type(
            "Token",
            "id",
            ColumnKind::Other,
            &["integer"]
        ))
        .unwrap()
        .contains("is another type")
    );
}

#[test]
fn messages_are_cut_at_a_character() {
    let mut message = Message::new();
    message
        .number(0)
        .push(" ")
        .number(18_446_744_073_709_551_615);
    assert_eq!(message.as_str(), "0 18446744073709551615");
    let mut long = Message::new();
    long.push(&"→".repeat(400));
    assert_eq!(long.as_str(), "→".repeat(341));
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

#[tideorm::model(table = "outline_teams")]
struct OutlineTeam {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(has_many = "OutlineMember", foreign_key = "team_id")]
    members: crate::relations::HasMany<OutlineMember>,
}

#[tideorm::model(table = "outline_members")]
#[index("team_id")]
struct OutlineMember {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "team_ref")]
    team_id: i64,
    #[tideorm(belongs_to = "OutlineTeam", foreign_key = "team_id")]
    team: crate::relations::BelongsTo<OutlineTeam>,
}

#[test]
fn a_derived_model_writes_its_node() {
    let team = OutlineTeam::__rules_node();
    let member = OutlineMember::__rules_node();
    assert_eq!((team.name, team.table), ("OutlineTeam", "outline_teams"));
    assert_eq!(team.owned.len(), 1);
    assert_eq!(team.owned[0].id, member.id);
    assert_ne!(team.id, member.id);
    // Without rules the crate requires no index; a relation that does not
    // own its rows is no edge.
    assert!(!member.indexes_foreign_keys);
    assert!(member.owned.is_empty());
    // An index on a renamed field leads with its column, under both names.
    assert_eq!(member.leading_columns, ["id", "team_id", "team_ref"]);
}
