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
fn test_soft_delete_query_uses_overridden_column() {
    let sql = CustomSoftDeleteModel::query().build_sql_preview();
    assert!(sql.contains("\"archived_on\" IS NULL"));
    assert!(!sql.contains("\"deleted_at\" IS NULL"));
}

#[test]
fn test_only_trashed_query_uses_overridden_column() {
    let sql = CustomSoftDeleteModel::query()
        .only_trashed()
        .build_sql_preview();
    assert!(sql.contains("\"archived_on\" IS NOT NULL"));
    assert!(!sql.contains("\"deleted_at\" IS NOT NULL"));
}
