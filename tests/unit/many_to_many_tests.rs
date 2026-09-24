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

/// A related model whose identity spans two columns, so the grouping has to
/// name both.
#[tideorm::model(table = "m2m_render_regions")]
struct M2mRenderRegion {
    #[tideorm(primary_key)]
    country_id: i64,
    #[tideorm(primary_key)]
    region_id: i64,
    name: String,
}

fn relation() -> HasManyThrough<M2mRenderTag, M2mRenderPostTag> {
    HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_post_tags")
}

/// The projection and the join `load` reads the relation through.
const EXPECTED_LOAD_PREFIX: &str = "SELECT \"m2m_render_tags\".\"id\", \"m2m_render_tags\".\"name\" FROM \"m2m_render_tags\" INNER JOIN \"m2m_render_post_tags\" ON \"m2m_render_post_tags\".\"tag_id\" = \"m2m_render_tags\".\"id\"";

#[test]
fn test_load_collapses_duplicate_pivot_rows_in_sql() {
    let sql = relation()
        .scope_to_pivot(M2mRenderTag::query(), &json!(1), true)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(sql.starts_with(EXPECTED_LOAD_PREFIX), "{sql}");
    assert!(
        sql.contains("\"m2m_render_post_tags\".\"post_id\""),
        "{sql}"
    );
    assert!(
        sql.contains("GROUP BY \"m2m_render_tags\".\"id\""),
        "duplicate pivot rows are collapsed by grouping on the related primary key: {sql}"
    );
}

/// PostgreSQL has no equality operator for `json`, so `SELECT DISTINCT` over
/// a projection containing one aborts the statement with "could not identify
/// an equality operator for type json". Deduplication must therefore never
/// compare whole rows.
#[test]
fn test_load_does_not_distinct_over_a_json_column() {
    let relation: HasManyThrough<M2mRenderDocument, M2mRenderPostTag> =
        HasManyThrough::new("post_id", "tag_id", "id", "id", "m2m_render_post_tags");

    let sql = relation
        .scope_to_pivot(M2mRenderDocument::query(), &json!(1), true)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        !sql.contains("DISTINCT"),
        "SELECT DISTINCT over \"m2m_render_documents\" cannot compare its json column: {sql}"
    );
    assert!(
        sql.starts_with(
            "SELECT \"m2m_render_documents\".\"id\", \"m2m_render_documents\".\"metadata\" FROM"
        ),
        "{sql}"
    );
    assert!(
        sql.contains("GROUP BY \"m2m_render_documents\".\"id\""),
        "{sql}"
    );
}

#[test]
fn test_load_groups_by_every_primary_key_column() {
    let relation: HasManyThrough<M2mRenderRegion, M2mRenderPostTag> = HasManyThrough::new(
        "post_id",
        "tag_id",
        "id",
        "country_id",
        "m2m_render_post_tags",
    );

    let sql = relation
        .scope_to_pivot(M2mRenderRegion::query(), &json!(1), true)
        .build_select_sql_for_db(DatabaseType::Postgres);

    let expected_grouping = concat!(
        "GROUP BY \"m2m_render_regions\".\"country_id\", ",
        "\"m2m_render_regions\".\"region_id\""
    );

    assert!(
        sql.contains(expected_grouping),
        "a composite identity needs every column to stay a functional dependency: {sql}"
    );
}

#[test]
fn test_constrained_load_is_left_undeduplicated() {
    let sql = relation()
        .scope_to_pivot(M2mRenderTag::query(), &json!(1), false)
        .build_select_sql_for_db(DatabaseType::Postgres);

    assert!(
        sql.starts_with("SELECT \"m2m_render_tags\".\"id\", \"m2m_render_tags\".\"name\" FROM"),
        "load_with() leaves the projection to its closure: {sql}"
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
