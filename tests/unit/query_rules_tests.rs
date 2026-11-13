use crate::model::Model;
use crate::query::{Aggregate, Order, QueryBuilder, WindowFunction, WindowFunctionType};

#[tideorm::model(table = "rule_users")]
struct RuleUser {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

fn raw_in(query: QueryBuilder<RuleUser>) -> Option<&'static str> {
    query.raw_sql_used()
}

#[test]
fn a_query_without_raw_sql_uses_none() {
    let query = RuleUser::query()
        .distinct()
        .select(["id", "name"])
        .where_eq("name", "Ada")
        .where_in("id", [1, 2])
        .order_desc("id");
    assert_eq!(raw_in(query), None);

    // Typed helpers that TideORM renders itself are not raw SQL.
    let query = RuleUser::query()
        .where_null("name")
        .group_by("name")
        .having(Aggregate::count().gt(1))
        .row_number("position", (), "id", Order::Asc)
        .lag("previous_id", "id", 1, (), "id", Order::Asc)
        .where_exists(RuleUser::query().where_eq("id", 1))
        .with_query("named", RuleUser::query().where_eq("id", 2))
        .union(RuleUser::query().where_eq("id", 3));
    assert_eq!(raw_in(query), None);
}

#[test]
fn each_raw_fragment_is_named() {
    let base = RuleUser::query;
    assert_eq!(raw_in(base().where_raw("1 = 1")), Some("where_raw()"));
    assert_eq!(
        raw_in(base().begin_or().or_where_raw("1 = 1").end_or()),
        Some("where_raw()")
    );
    assert_eq!(
        raw_in(base().select_raw("COUNT(*) AS n")),
        Some("select_raw()")
    );
    assert_eq!(
        raw_in(base().order_by_raw("LOWER(name)", Order::Asc)),
        Some("order_by_raw()")
    );
    assert_eq!(
        raw_in(base().having_raw("COUNT(*) > 1")),
        Some("having_raw()")
    );
    assert_eq!(
        raw_in(base().union_raw("SELECT id, name FROM rule_users")),
        Some("union_raw()")
    );
    let custom = WindowFunctionType::Custom("NTILE(4)".to_string());
    assert_eq!(
        raw_in(base().window(WindowFunction::new(custom, "quartile"))),
        Some("WindowFunctionType::Custom")
    );
}

#[test]
fn conditions_of_or_groups_are_read() {
    let query = RuleUser::query()
        .where_eq("id", 1)
        .begin_or()
        .or_where_eq("name", "a")
        .or_where_in("name", ["b", "c"])
        .end_or();
    assert_eq!(query.all_conditions().len(), 3);
}
