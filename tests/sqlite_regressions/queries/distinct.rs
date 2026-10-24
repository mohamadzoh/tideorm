//! `DISTINCT` plucks and aggregates follow database equality and the requested order.

use crate::fixtures::memory_database;
use tideorm::prelude::*;

#[tideorm::model(table = "collation_names")]
struct Name {
    #[tideorm(primary_key)]
    id: i64,
    name: Option<String>,
}

async fn database(collation: &str, second: &str) -> Database {
    memory_database(&format!(
        "CREATE TABLE collation_names(id INTEGER PRIMARY KEY,name TEXT COLLATE {collation}); \
         INSERT INTO collation_names VALUES(1,'Alpha'),(2,'{second}'),(3,'Beta'),(4,NULL),(5,NULL)"
    ))
    .await
}

#[tokio::test]
async fn distinct_pluck_uses_database_equality_for_every_projection_shape() {
    for (collation, second) in [
        ("NOCASE", "alpha"),
        ("RTRIM", "Alpha "),
        ("BINARY", "alpha"),
    ] {
        let db = database(collation, second).await;
        let expected = Name::query_with(&db)
            .distinct()
            .order_asc("name")
            .pluck::<Option<String>>("name")
            .await
            .unwrap();
        assert_eq!(expected.len(), if collation == "BINARY" { 4 } else { 3 });
        // Each shape selects more than `name`; the pluck is distinct on `name`
        // alone, under its collation, before a page is cut.
        for shape in 0..7 {
            let base = Name::query_with(&db).where_gt("id", 0);
            let query = match shape {
                0 => base.select_raw("id AS extra"),
                1 => base.row_number("extra", None, "id", Order::Asc),
                2 => base.select_raw("row_number() OVER (ORDER BY id) AS extra"),
                3 => base.select_subquery(
                    Name::query_with(&db)
                        .select(vec!["id"])
                        .where_eq("id", 1)
                        .limit(1),
                    "extra",
                ),
                4 => base.select_raw("id AS tideorm_pluck_group"),
                5 => base
                    .select(vec!["id", "name"])
                    .where_lte("id", 2)
                    .union_all(
                        Name::query_with(&db)
                            .select(vec!["id", "name"])
                            .where_gt("id", 2),
                    ),
                _ => base.select(vec!["id", "name"]).where_lte("id", 2).union(
                    Name::query_with(&db)
                        .select(vec!["id", "name"])
                        .where_gt("id", 2),
                ),
            }
            .distinct()
            .order_asc("name");
            assert_eq!(
                query.clone().pluck::<Option<String>>("name").await.unwrap(),
                expected,
                "{collation}, {shape}"
            );
            for offset in 0..=expected.len() {
                assert_eq!(
                    query
                        .clone()
                        .offset(offset as u64)
                        .limit(1)
                        .pluck::<Option<String>>("name")
                        .await
                        .unwrap(),
                    expected
                        .iter()
                        .skip(offset)
                        .take(1)
                        .cloned()
                        .collect::<Vec<_>>(),
                    "{collation}, {shape}, {offset}",
                );
            }
            assert!(
                query
                    .limit(0)
                    .pluck::<Option<String>>("name")
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[tokio::test]
async fn distinct_pluck_keeps_first_value_in_requested_order() {
    let db = database("NOCASE", "alpha").await;
    for compound in [false, true] {
        let query = if compound {
            Name::query_with(&db)
                .select(vec!["id", "name AS label"])
                .where_eq("id", 1)
                .union_all(
                    Name::query_with(&db)
                        .select(vec!["id", "name AS label"])
                        .where_gt("id", 1),
                )
        } else {
            Name::query_with(&db).select_raw("id AS extra")
        }
        .distinct();
        let output = if compound { "label" } else { "name" };
        for ordering in 0..4 {
            let ordered = match ordering {
                0 => query.clone().order_desc("id"),
                1 => query.clone().order_desc("collation_names.id"),
                2 => query
                    .clone()
                    .order_by_raw("\"collation_names\".\"id\"", Order::Desc),
                _ => query.clone().order_by_raw("id", Order::Desc),
            };
            assert_eq!(
                ordered.pluck::<Option<String>>(output).await.unwrap(),
                vec![None, Some("Beta".into()), Some("alpha".into())],
                "compound={compound}, ordering={ordering}",
            );
        }
        assert_eq!(
            query
                .order_desc("id")
                .offset(1)
                .limit(1)
                .pluck::<Option<String>>(output)
                .await
                .unwrap(),
            vec![Some("Beta".into())],
        );
    }
}

#[tokio::test]
async fn distinct_union_pluck_preserves_selected_raw_order_expression() {
    let db = database("NOCASE", "alpha").await;
    let arm = || {
        Name::query_with(&db)
            .select(vec!["name"])
            .select_raw("id + 0 AS sortkey")
    };
    let query = arm()
        .where_lte("id", 2)
        .union_all(arm().where_gt("id", 2))
        .distinct()
        .order_by_raw("id + 0", Order::Desc);
    assert_eq!(query.clone().get_json().await.unwrap().len(), 5);
    assert_eq!(
        query.pluck::<Option<String>>("name").await.unwrap(),
        vec![None, Some("Beta".into()), Some("alpha".into())],
    );
}

#[tideorm::model(table = "resume_entries")]
struct Entry {
    #[tideorm(primary_key)]
    id: i64,
    amount: i64,
}

#[tokio::test]
async fn distinct_aggregates_resolve_swapped_output_aliases_first() {
    let db = memory_database(
        "CREATE TABLE resume_entries(id INTEGER PRIMARY KEY,amount INTEGER); \
         INSERT INTO resume_entries VALUES(1,10),(2,20)",
    )
    .await;
    let q = Entry::query_with(&db)
        .select(vec!["id AS amount", "amount AS id"])
        .distinct();
    assert_eq!(q.clone().sum::<i64>("amount").await.unwrap(), 3);
    assert_eq!(q.clone().min::<i64>("id").await.unwrap(), Some(10));
    assert_eq!(
        q.clone().sum::<i64>("resume_entries.amount").await.unwrap(),
        30
    );
    assert_eq!(
        q.aggregates::<(i64, i64)>(&[Aggregate::sum("amount"), Aggregate::min("id")])
            .await
            .unwrap(),
        (3, 10)
    );
}
