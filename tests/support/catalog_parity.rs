//! The catalog scenario every backend target runs.
//!
//! The including target supplies `mod backend` with `DATABASE_TYPE` and
//! `async fn connect() -> bool`, as for `integration_parity.rs`.

use tideorm::migration::Schema;
use tideorm::schema::ForeignKeyInfo;

use super::backend;

const VIEW: &str = "catalog_child_labels";
const TABLES: [&str; 2] = ["catalog_children", "catalog_parents"];

#[tokio::test]
async fn the_catalog_reads_tables_views_columns_indexes_and_keys() {
    if !backend::connect().await {
        return;
    }
    let db = tideorm::require_db().expect("global database should be installed");
    let mut schema = Schema::new(backend::DATABASE_TYPE);
    db.exec_raw(&format!("DROP VIEW IF EXISTS {VIEW}"))
        .await
        .unwrap();
    for table in TABLES {
        schema.drop_table_if_exists(table).await.unwrap();
    }
    schema
        .create_table("catalog_parents", |t| {
            t.id();
            t.string("code").not_null().unique();
        })
        .await
        .unwrap();
    schema
        .create_table("catalog_children", |t| {
            t.id();
            t.big_integer("parent_id")
                .not_null()
                .references("catalog_parents", "id");
            t.string("label").nullable();
            t.index(&["label"]);
        })
        .await
        .unwrap();
    db.exec_raw(&format!(
        "CREATE VIEW {VIEW} AS SELECT label FROM catalog_children"
    ))
    .await
    .unwrap();

    let catalog = db.catalog();
    let tables = catalog.tables().await.unwrap();
    for table in TABLES {
        assert!(tables.iter().any(|name| name == table), "{tables:?}");
    }
    assert!(!tables.iter().any(|name| name == VIEW), "{tables:?}");
    assert!(
        catalog
            .views()
            .await
            .unwrap()
            .iter()
            .any(|name| name == VIEW)
    );

    let columns = catalog.columns("catalog_children").await.unwrap();
    let keys: Vec<_> = columns
        .iter()
        .map(|column| {
            let name = column.name.as_str();
            (name, column.primary_key, column.foreign_key, column.unique)
        })
        .collect();
    assert_eq!(
        keys,
        [
            ("id", true, false, false),
            ("parent_id", false, true, false),
            ("label", false, false, false),
        ]
    );
    assert!(!columns[1].nullable && columns[2].nullable, "{columns:?}");
    let parents = catalog.columns("catalog_parents").await.unwrap();
    assert!(
        parents
            .iter()
            .any(|column| column.name == "code" && column.unique),
        "{parents:?}"
    );

    let indexes = catalog.indexes("catalog_children").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.columns == ["label"] && !index.unique && !index.primary),
        "{indexes:?}"
    );
    assert_eq!(
        catalog.foreign_keys("catalog_children").await.unwrap(),
        [ForeignKeyInfo {
            column: "parent_id".to_string(),
            references_table: "catalog_parents".to_string(),
            references_column: "id".to_string(),
        }]
    );
    assert!(catalog.columns("catalog_missing").await.unwrap().is_empty());

    db.exec_raw(&format!("DROP VIEW {VIEW}")).await.unwrap();
    for table in TABLES {
        schema.drop_table_if_exists(table).await.unwrap();
    }
}
