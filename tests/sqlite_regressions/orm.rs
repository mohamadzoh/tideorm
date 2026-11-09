//! The `tideorm::orm` API on engine entities, read back row for row: updates
//! that change nothing, and typed pagination.
//!
//! The submodules also run against a server: CI points `ENGINE_PROBE_DATABASE_URL`
//! at PostgreSQL, MySQL or MariaDB and runs them by path (`orm::insert_defaults::`).

#[path = "orm/builder_conversions.rs"]
mod builder_conversions;
#[path = "orm/insert_defaults.rs"]
mod insert_defaults;

use tideorm::orm::{
    ColumnTrait, ConnectionTrait, Database, DatabaseConnection, DbErr, EntityTrait, QueryFilter,
};

/// An update that sets no field and finds no row to read back.
fn assert_not_found_after_update<T: std::fmt::Debug>(result: &Result<T, DbErr>) {
    assert!(
        matches!(result, Err(DbErr::RecordNotFound(message)) if message == "Failed to find updated item"),
        "{result:?}"
    );
}

#[tokio::test]
async fn unchanged_updates_do_not_bypass_additional_filters() {
    use tideorm::orm::{Condition, IntoActiveModel, QueryTrait, Set, Unchanged};

    let db = bakery().await;
    db.execute_unprepared("CREATE TRIGGER forbid_cake_update BEFORE UPDATE ON cake BEGIN SELECT RAISE(ABORT, 'no-op must not write'); END")
        .await.unwrap();
    let original = cake::Entity::find_by_id(1).one(&db).await.unwrap().unwrap();
    let unchanged = original.clone().into_active_model();
    assert_eq!(unchanged.name, Unchanged(original.name.clone()));
    let result = cake::Entity::update(unchanged)
        .validate()
        .unwrap()
        .filter(cake::Column::Name.eq("not the stored name"))
        .exec(&db)
        .await;
    assert_not_found_after_update(&result);

    let matched = cake::Entity::update(original.clone().into_active_model())
        .validate()
        .unwrap()
        .filter(
            Condition::any()
                .add(cake::Column::Name.eq("not the stored name"))
                .add(cake::Column::Name.eq(original.name.clone())),
        )
        .exec(&db)
        .await
        .unwrap();
    assert_eq!(matched, original);

    let mut limited = cake::Entity::update(original.into_active_model())
        .validate()
        .unwrap();
    QueryTrait::query(&mut limited).limit(0);
    let result = limited.exec(&db).await;
    assert_not_found_after_update(&result);
    let result = cake::Entity::update(cake::ActiveModel {
        id: Set(999),
        ..Default::default()
    })
    .exec(&db)
    .await;
    assert_not_found_after_update(&result);
}

#[tokio::test]
async fn unchanged_updates_without_returning_check_the_matching_row() {
    use tideorm::orm::{IntoActiveModel, Set};

    let db = bakery().await;
    db.execute_unprepared("CREATE TRIGGER forbid_cake_update BEFORE UPDATE ON cake BEGIN SELECT RAISE(ABORT, 'no-op must not write'); END")
        .await.unwrap();
    let original = cake::Entity::find_by_id(1).one(&db).await.unwrap().unwrap();
    let result = cake::Entity::update(original.clone().into_active_model())
        .validate()
        .unwrap()
        .filter(cake::Column::Name.eq("not the stored name"))
        .exec_without_returning(&db)
        .await;
    assert!(matches!(result, Err(DbErr::RecordNotUpdated)), "{result:?}");
    let result = cake::Entity::update(cake::ActiveModel {
        id: Set(999),
        ..Default::default()
    })
    .exec_without_returning(&db)
    .await;
    assert!(matches!(result, Err(DbErr::RecordNotUpdated)), "{result:?}");
    let result = cake::Entity::update(original.into_active_model())
        .exec_without_returning(&db)
        .await
        .unwrap();
    assert_eq!(result.rows_affected, 0);
}

#[tokio::test]
async fn unchanged_updates_preserve_composed_query_predicates() {
    use tideorm::orm::sql_builder::{CommonTableExpression, Expr, ExprTrait, Order, Query};
    use tideorm::orm::{IntoActiveModel, QueryTrait};

    let db = bakery().await;
    db.execute_unprepared("CREATE TRIGGER forbid_cake_update BEFORE UPDATE ON cake BEGIN SELECT RAISE(ABORT, 'no-op must not write'); END")
        .await.unwrap();
    let original = cake::Entity::find_by_id(1).one(&db).await.unwrap().unwrap();

    for expected_name in [original.name.as_str(), "excluded ' gâteau"] {
        for returning in [true, false] {
            let mut update = cake::Entity::update(original.clone().into_active_model())
                .validate()
                .unwrap();
            QueryTrait::query(&mut update)
                .with_cte(
                    CommonTableExpression::new()
                        .table_name("permitted")
                        .query(
                            Query::select()
                                .expr_as(Expr::val(expected_name), "name")
                                .to_owned(),
                        )
                        .to_owned(),
                )
                .from("permitted")
                .and_where(
                    Expr::col((cake::Entity, cake::Column::Name))
                        .eq(Expr::col(("permitted", "name"))),
                )
                .and_where(
                    Expr::col((cake::Entity, cake::Column::Id))
                        .in_subquery(Query::select().expr(Expr::val(1)).to_owned()),
                )
                .order_by_expr(
                    Expr::col((cake::Entity, cake::Column::Id)).add(10),
                    Order::Desc,
                )
                .limit(2);
            let result = if returning {
                update
                    .exec(&db)
                    .await
                    .map(|model| assert_eq!(model, original))
            } else {
                update.exec_without_returning(&db).await.map(|result| {
                    assert_eq!(result.rows_affected, 0);
                })
            };
            if expected_name == original.name {
                result.unwrap();
            } else if returning {
                assert_not_found_after_update(&result);
            } else {
                assert!(matches!(result, Err(DbErr::RecordNotUpdated)), "{result:?}");
            }
        }
    }
}

mod cake {
    #[tideorm::model(table = "cake")]
    #[derive(PartialEq)]
    pub struct Model {
        #[tideorm(primary_key, auto_increment)]
        pub id: i32,
        pub name: String,
    }

    pub use __tideorm_internal_model::{ActiveModel, Column, Entity};
}

/// Three cakes.
async fn bakery() -> DatabaseConnection {
    let db = tideorm::orm::Database::connect("sqlite::memory:")
        .await
        .expect("open in-memory SQLite");
    db.execute_unprepared(
        "CREATE TABLE cake (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         INSERT INTO cake VALUES (1, 'Chocolate'), (2, 'Lemon'), (3, 'Plain');",
    )
    .await
    .expect("create the bakery");
    db
}

#[tokio::test]
async fn typed_pagination_rejects_overflowing_offsets() {
    use tideorm::orm::PaginatorTrait;

    // The offset overflows before any statement runs, so the table need not exist.
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let pages = cake::Entity::find().paginate(&db, 2);
    assert!(matches!(
        pages.fetch_page(u64::MAX).await,
        Err(DbErr::Custom(_))
    ));
}
