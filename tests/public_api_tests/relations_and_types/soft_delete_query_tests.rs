use tideorm::prelude::*;

#[tideorm::model(
    table = "query_soft_delete_override",
    soft_delete,
    deleted_at_column = "archived_on"
)]
struct CustomSoftDeleteModel {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    archived_on: Option<chrono::DateTime<chrono::Utc>>,
}

#[test]
fn test_soft_delete_scopes_use_the_overridden_column() {
    let live = CustomSoftDeleteModel::query().build_sql_preview();
    assert!(live.contains("\"archived_on\" IS NULL"), "{live}");
    assert!(!live.contains("deleted_at"), "{live}");

    let trashed = CustomSoftDeleteModel::query()
        .only_trashed()
        .build_sql_preview();
    assert!(trashed.contains("\"archived_on\" IS NOT NULL"), "{trashed}");
    assert!(!trashed.contains("deleted_at"), "{trashed}");
}
