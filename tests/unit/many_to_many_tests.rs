use super::HasManyThrough;
use crate::config::DatabaseType;
use crate::model::Model;
use serde_json::json;

#[tideorm::model(table = "m2m_render_tags")]
struct M2mRenderTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
}

#[tideorm::model(table = "m2m_render_post_tags")]
struct M2mRenderPostTag {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    post_id: i64,
    tag_id: i64,
}

/// A related model carrying a `json` column — the type PostgreSQL has no
/// equality operator for, and therefore cannot `SELECT DISTINCT` over.
#[tideorm::model(table = "m2m_render_documents")]
struct M2mRenderDocument {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    metadata: serde_json::Value,
}

/// A pivot whose key fields are renamed columns.
#[tideorm::model(table = "m2m_render_renamed_links")]
struct M2mRenderRenamedLink {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "pid")]
    post_id: i64,
    #[tideorm(column = "tid")]
    tag_id: i64,
}

fn relation() -> HasManyThrough<M2mRenderTag, M2mRenderPostTag> {
    HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_post_tags")
}

fn load_sql<R: Model, P: Model>(relation: HasManyThrough<R, P>, key: serde_json::Value) -> String {
    relation
        .with_parent_pk(key)
        .load_query("test")
        .expect("load query")
        .build_select_sql_for_db(DatabaseType::Postgres)
}

/// `load` and `count` read the related rows a live pivot row links through a
/// semi-join, which lists each once however many pivot rows link it. A join
/// would repeat it, and collapsing that takes a `GROUP BY` beside
/// `SELECT related.*` (refused by MariaDB's `ONLY_FULL_GROUP_BY`) or a
/// `DISTINCT` (refused by PostgreSQL for a `json` column).
#[test]
fn test_load_reads_each_related_row_once_through_a_semi_join() {
    let sql = load_sql(relation(), json!(1));

    assert!(
        sql.starts_with(
            "SELECT \"m2m_render_tags\".\"id\", \"m2m_render_tags\".\"name\" FROM \"m2m_render_tags\" WHERE"
        ),
        "{sql}"
    );
    assert!(
        sql.contains("WHERE \"id\" IN (SELECT \"m2m_render_post_tags\".\"tag_id\" FROM \"m2m_render_post_tags\" WHERE \"post_id\" = 1)"),
        "{sql}"
    );
    for refused in ["JOIN", "GROUP BY", "DISTINCT"] {
        assert!(!sql.contains(refused), "{refused}: {sql}");
    }
}

#[test]
fn test_load_does_not_distinct_over_a_json_column() {
    let relation: HasManyThrough<M2mRenderDocument, M2mRenderPostTag> =
        HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_post_tags");
    let sql = load_sql(relation, json!(1));

    assert!(
        sql.starts_with(
            "SELECT \"m2m_render_documents\".\"id\", \"m2m_render_documents\".\"metadata\" FROM"
        ),
        "{sql}"
    );
    assert!(
        !sql.contains("DISTINCT") && !sql.contains("GROUP BY"),
        "{sql}"
    );
}

/// A pivot key naming the field of a renamed column reads that column.
#[test]
fn test_pivot_keys_resolve_renamed_columns() {
    let relation: HasManyThrough<M2mRenderTag, M2mRenderRenamedLink> =
        HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_renamed_links");
    let sql = load_sql(relation, json!(1));
    assert!(
        sql.contains("SELECT \"m2m_render_renamed_links\".\"tid\" FROM"),
        "{sql}"
    );
    assert!(sql.contains("WHERE \"pid\" = 1)"), "{sql}");

    let relation: HasManyThrough<M2mRenderTag, M2mRenderRenamedLink> =
        HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_renamed_links");
    let joined = relation
        .scope_to_pivot(M2mRenderTag::query(), &json!(1))
        .build_select_sql_for_db(DatabaseType::Postgres);
    assert!(
        joined.contains("ON \"m2m_render_renamed_links\".\"tid\" = \"m2m_render_tags\".\"id\""),
        "{joined}"
    );
    assert!(
        joined.contains("\"m2m_render_renamed_links\".\"pid\" = 1"),
        "{joined}"
    );

    let (insert, _) = super::build_pivot_insert::<M2mRenderRenamedLink>(
        DatabaseType::Postgres,
        "m2m_render_renamed_links",
        "post_id",
        "tag_id",
        &json!(1),
        &json!(2),
    );
    assert!(insert.contains("(\"pid\", \"tid\")"), "{insert}");
}

/// An owner whose key is NULL links no row: the lookup matches nothing
/// rather than every pivot row whose key is NULL.
#[test]
fn test_a_null_owner_key_links_nothing() {
    let sql = load_sql(relation(), json!(null));
    assert!(sql.contains("0 = 1"), "{sql}");
    assert!(!sql.contains("IS NULL"), "{sql}");
}

#[test]
fn test_constrained_load_joins_the_pivot() {
    let sql = relation()
        .scope_to_pivot(M2mRenderTag::query(), &json!(1))
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        sql.starts_with("SELECT \"m2m_render_tags\".\"id\", \"m2m_render_tags\".\"name\" FROM \"m2m_render_tags\" INNER JOIN \"m2m_render_post_tags\" ON \"m2m_render_post_tags\".\"tag_id\" = \"m2m_render_tags\".\"id\""),
        "load_with() joins the pivot so its closure can read pivot columns: {sql}"
    );
    assert!(
        !sql.contains("GROUP BY"),
        "load_with() leaves deduplication to its closure: {sql}"
    );
}

#[test]
fn attach_inserts_only_a_missing_pivot_row_and_tolerates_a_key_conflict() {
    let (sql, params) = super::build_pivot_insert::<M2mRenderPostTag>(
        DatabaseType::Postgres,
        "m2m_render_post_tags",
        "post_id",
        "tag_id",
        &json!(7),
        &json!(9),
    );
    assert_eq!(
        sql,
        "INSERT INTO \"m2m_render_post_tags\" (\"post_id\", \"tag_id\") SELECT $1, $2 \
         WHERE NOT EXISTS (SELECT 1 FROM \"m2m_render_post_tags\" WHERE \"post_id\" = $3 \
         AND \"tag_id\" = $4) ON CONFLICT DO NOTHING"
    );
    assert_eq!(params.len(), 4);

    // MySQL checks with a separate, non-locking read: InnoDB locks what an
    // `INSERT .. SELECT` reads, and two such attaches deadlock.
    let (sql, params) = super::build_pivot_insert::<M2mRenderPostTag>(
        DatabaseType::MySQL,
        "m2m_render_post_tags",
        "post_id",
        "tag_id",
        &json!(7),
        &json!(9),
    );
    assert_eq!(
        sql,
        "INSERT INTO `m2m_render_post_tags` (`post_id`, `tag_id`) VALUES (?, ?) \
         ON DUPLICATE KEY UPDATE `post_id` = `post_id`"
    );
    assert_eq!(params.len(), 2);
}

#[test]
fn a_pivot_named_with_its_schema_is_quoted_part_by_part() {
    let (sql, _) = super::build_pivot_insert::<M2mRenderPostTag>(
        DatabaseType::Postgres,
        "billing.user_roles",
        "post_id",
        "tag_id",
        &json!(7),
        &json!(9),
    );
    assert!(
        sql.starts_with("INSERT INTO \"billing\".\"user_roles\" "),
        "{sql}"
    );
}
