//! Aggregates, HAVING and EXISTS keep their operands' types and the rows asked about.

use crate::fixtures::{
    AuditRow, QueryShape, User, memory_database, query_shapes_database, users_and_posts,
};
use rust_decimal::Decimal;
use tideorm::prelude::*;

#[tokio::test]
async fn aggregate_aliases_and_bounded_ordering_keep_the_original_rows() {
    let db = memory_database(
        "CREATE TABLE audit_rows(id INTEGER PRIMARY KEY,amount INTEGER,aggregate_1 INTEGER); \
         INSERT INTO audit_rows VALUES(1,20,7),(2,10,9),(3,30,11)",
    )
    .await;
    assert_eq!(
        AuditRow::query_with(&db)
            .aggregates::<(i64, i64)>(&[Aggregate::min("aggregate_1"), Aggregate::sum("amount")])
            .await
            .unwrap(),
        (7, 60)
    );
    assert_eq!(
        AuditRow::query_with(&db)
            .select(vec!["id", "amount AS position"])
            .order_by_raw("position", Order::Asc)
            .limit(1)
            .sum::<i64>("id")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        AuditRow::query_with(&db)
            .select(vec!["id AS amount", "amount AS id"])
            .order_asc("amount")
            .limit(1)
            .sum::<i64>("id")
            .await
            .unwrap(),
        1
    );
}

#[tideorm::model(table = "audit_groups")]
struct Grouped {
    #[tideorm(primary_key)]
    id: i64,
    category: String,
    amount: Option<i64>,
}

#[tokio::test]
async fn aggregate_comparisons_bind_null_and_numeric_values_correctly() {
    let db = memory_database(
        "CREATE TABLE audit_groups(id INTEGER PRIMARY KEY,category TEXT,amount INTEGER); \
         INSERT INTO audit_groups VALUES(1,'empty',NULL),(2,'a',2),(3,'a',3),(4,'b',8)",
    )
    .await;
    let q = Grouped::query_with(&db)
        .select(vec!["category"])
        .group_by("category");
    assert_eq!(
        q.clone()
            .having(Aggregate::min("amount").eq(None::<i64>))
            .get_json()
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        q.clone()
            .having(Aggregate::min("amount").ne(None::<i64>))
            .get_json()
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(
        q.clone()
            .having(Aggregate::min("amount").gt(None::<i64>))
            .get_json()
            .await
            .is_err()
    );
    assert_eq!(
        q.clone()
            .having(Aggregate::count().gt(Decimal::ONE))
            .get_json()
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        q.having(Aggregate::avg("amount").gt("2.4".parse::<Decimal>().unwrap()))
            .get_json()
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tideorm::model(table = "audit_text_groups")]
struct TextGrouped {
    #[tideorm(primary_key)]
    id: i64,
    category: String,
    amount: String,
}

#[tokio::test]
async fn numeric_aggregates_do_not_inherit_text_parameter_types() {
    let db = memory_database(
        "CREATE TABLE audit_text_groups(id INTEGER PRIMARY KEY,category TEXT,amount TEXT); \
         INSERT INTO audit_text_groups VALUES(1,'a','2'),(2,'a','3'),(3,'b','8')",
    )
    .await;
    let q = TextGrouped::query_with(&db)
        .select(vec!["category"])
        .group_by("category");
    assert_eq!(
        q.clone()
            .having(Aggregate::sum("amount").gt(6))
            .get_json()
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        q.having(Aggregate::avg("amount").gt(2.4))
            .get_json()
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tideorm::model(table = "resume_tokens")]
struct Token {
    #[tideorm(primary_key)]
    id: i64,
    token: Uuid,
    active: bool,
    day: chrono::NaiveDate,
}

#[tideorm::model(table = "resume_hosts")]
struct Host {
    #[tideorm(primary_key)]
    id: i64,
}

#[tokio::test]
async fn nested_scalar_types_and_having_join_order_are_preserved() {
    let _ = TideConfig::init().models::<(Host, Token)>();
    let db = memory_database(
        "CREATE TABLE resume_tokens(id INTEGER PRIMARY KEY,token BLOB,active INTEGER,day TEXT); \
         INSERT INTO resume_tokens VALUES(1,X'00112233445566778899AABBCCDDEEFF',1,'2024-02-29'); \
         CREATE TABLE resume_hosts(id INTEGER PRIMARY KEY); INSERT INTO resume_hosts VALUES(1)",
    )
    .await;
    let token = Uuid::parse_str("00112233-4455-6677-8899-aabbccddeeff").unwrap();
    for (column, expected) in [
        ("token", json!(token)),
        ("active", json!(true)),
        ("day", json!("2024-02-29")),
    ] {
        let scalar = Host::query_with(&db)
            .select_subquery(
                Token::query_with(&db).select(vec![column]).limit(1),
                "picked",
            )
            .limit(1);
        assert_eq!(
            scalar.clone().get_json().await.unwrap()[0]["picked"],
            expected
        );
        assert_eq!(
            Host::query_with(&db)
                .select_subquery(scalar.clone(), "nested")
                .get_json()
                .await
                .unwrap()[0]["nested"],
            expected
        );
        assert_eq!(
            scalar
                .distinct()
                .min::<serde_json::Value>("picked")
                .await
                .unwrap(),
            Some(expected)
        );
    }
    for alias in [false, true] {
        let qualifier = if alias { "t" } else { "resume_tokens" };
        let base = || {
            Host::query_with(&db)
                .select(vec!["resume_hosts.id"])
                .group_by("resume_hosts.id")
        };
        let join = |q: QueryBuilder<Host>| {
            if alias {
                q.inner_join_as("resume_tokens", "t", "resume_hosts.id", "t.id")
            } else {
                q.inner_join("resume_tokens", "resume_hosts.id", "resume_tokens.id")
            }
        };
        for column in ["token", "day"] {
            let value = if column == "token" {
                json!(token)
            } else {
                json!("2024-02-29")
            };
            let operand = format!("{qualifier}.{column}");
            let before = join(base().having(Aggregate::min(&operand).eq(value.clone())));
            let after = join(base()).having(Aggregate::min(&operand).eq(value.clone()));
            assert_eq!(before.get_json().await.unwrap(), vec![json!({"id":1})]);
            assert_eq!(after.get_json().await.unwrap(), vec![json!({"id":1})]);
            let fragment = base()
                .having(Aggregate::max(&operand).eq(value))
                .consolidate();
            assert_eq!(
                join(base())
                    .apply(&fragment)
                    .get_json()
                    .await
                    .unwrap()
                    .len(),
                1
            );
        }
    }
}

#[tokio::test]
async fn exists_preserves_aggregate_cardinality_and_having_aliases() {
    let db = query_shapes_database().await;
    for expression in ["COUNT(*) AS total", "SUM(id) AS total", "MIN(id) AS total"] {
        for id in [-1, 1] {
            let query = QueryShape::query_with(&db)
                .where_eq("id", id)
                .select_raw(expression);
            assert_eq!(query.clone().get_json().await.unwrap().len(), 1);
            assert!(query.clone().exists().await.unwrap());
            assert!(query.limit(0).offset(100).exists().await.unwrap());
        }
    }
    assert!(
        !QueryShape::query_with(&db)
            .where_eq("id", -1)
            .exists()
            .await
            .unwrap()
    );
    assert!(
        !QueryShape::query_with(&db)
            .select(vec!["id"])
            .where_eq("id", -1)
            .exists()
            .await
            .unwrap()
    );
    for (id, expected) in [(-1, false), (1, true)] {
        assert_eq!(
            QueryShape::query_with(&db)
                .where_eq("id", id)
                .select_raw("COUNT(*) AS total")
                .having("total > 0")
                .exists()
                .await
                .unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn distinct_aggregate_does_not_substitute_another_tables_column() {
    let db = users_and_posts("aggregate", 100).await;
    let query = User::query_with(&db)
        .inner_join("review_posts", "review_users.id", "review_posts.user_id")
        .select(vec!["review_users.id"])
        .distinct();
    assert!(query.clone().sum::<i64>("review_posts.id").await.is_err());
    assert_eq!(query.sum::<i64>("review_users.id").await.unwrap(), 1);
    let query = User::query_with(&db)
        .inner_join("review_posts", "review_users.id", "review_posts.user_id")
        .select(vec!["review_posts.id AS post_id"])
        .distinct();
    assert_eq!(
        query.clone().sum::<i64>("review_posts.id").await.unwrap(),
        100
    );
    assert_eq!(query.sum::<i64>("post_id").await.unwrap(), 100);
}

#[tokio::test]
async fn aggregate_rejects_unknown_operands_instead_of_sqlite_literals() {
    let db = users_and_posts("aggregate", 10).await;
    let query = User::query_with(&db);
    assert!(query.clone().sum::<f64>("ammount").await.is_err());
    assert!(query.clone().min::<String>("ammount").await.is_err());
    assert!(query.clone().max::<String>("ammount").await.is_err());
    assert!(query.clone().avg::<f64>("ammount").await.is_err());
    assert!(query.clone().count_distinct("ammount").await.is_err());
    assert!(query.clone().limit(1).sum::<i64>("ammount").await.is_err());
    assert!(
        query
            .clone()
            .aggregates::<serde_json::Value>(&[Aggregate::count(), Aggregate::sum("ammount")])
            .await
            .is_err()
    );
    assert_eq!(query.clone().sum::<i64>("id").await.unwrap(), 1);
    assert_eq!(
        query
            .aggregates::<serde_json::Value>(&[Aggregate::count()])
            .await
            .unwrap(),
        json!([1])
    );
}

#[tokio::test]
async fn aggregate_aliases_do_not_inherit_model_field_types() {
    #[tideorm::model(table = "review_aliases")]
    struct Aliases {
        #[tideorm(primary_key)]
        id: i64,
        count: bool,
        agg_result: bool,
        aggregate_0: bool,
        exists_result: String,
    }
    let db = memory_database(
        "CREATE TABLE review_aliases(id INTEGER PRIMARY KEY, count BOOLEAN, agg_result BOOLEAN, \
         aggregate_0 BOOLEAN, exists_result TEXT); \
         INSERT INTO review_aliases VALUES(1,1,1,1,'yes'),(2,0,0,0,'no')",
    )
    .await;
    let query = Aliases::query_with(&db);
    assert_eq!(query.clone().count().await.unwrap(), 2);
    assert_eq!(query.clone().sum::<i64>("id").await.unwrap(), 3);
    assert_eq!(
        query
            .clone()
            .aggregates::<serde_json::Value>(&[Aggregate::count(), Aggregate::sum("id")])
            .await
            .unwrap(),
        json!([2, 3])
    );
    assert!(query.clone().exists().await.unwrap());
    assert_eq!(query.min::<bool>("count").await.unwrap(), Some(false));
}

#[tokio::test]
async fn aggregate_projection_alias_uses_source_type() {
    let db = users_and_posts("Alice", 1).await;
    db.exec_raw("UPDATE review_users SET id=7").await.unwrap();
    let query = User::query_with(&db)
        .select(vec!["id AS active"])
        .distinct();
    assert_eq!(query.clone().min::<i64>("active").await.unwrap(), Some(7));
    assert_eq!(query.clone().max::<i64>("active").await.unwrap(), Some(7));
    assert_eq!(
        query
            .aggregates::<(i64, i64)>(&[Aggregate::min("active"), Aggregate::max("active")])
            .await
            .unwrap(),
        (7, 7)
    );
    assert_eq!(
        User::query_with(&db)
            .select_raw("id+1 AS active")
            .distinct()
            .min::<i64>("active")
            .await
            .unwrap(),
        Some(8)
    );
}
