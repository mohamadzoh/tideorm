//! Default-only and empty insert batches preserve their row count and semantics.

use tideorm::orm::{
    ConnectionTrait, Database, DatabaseConnection, DatabaseExecutor, DbBackend, EntityTrait,
    QueryTrait, Set, TransactionTrait, TryInsertResult,
};

async fn database() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE default_entries (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL DEFAULT 'from database')").await.unwrap();
    db
}

fn batch() -> tideorm::orm::InsertMany<entry::ActiveModel> {
    entry::Entity::insert_many(std::iter::repeat_with(entry::ActiveModel::default).take(3))
}

#[tokio::test]
async fn empty_batches_stay_empty_after_configuring_conflicts() {
    use tideorm::orm::sql_builder::OnConflict;
    use tideorm::orm::{DbErr, ExecResult, QueryResult, Statement, TryInsert};

    struct NoSql(DbBackend);
    impl ConnectionTrait for NoSql {
        fn get_database_backend(&self) -> DbBackend {
            self.0
        }
        async fn execute_raw(&self, _: Statement) -> Result<ExecResult, DbErr> {
            panic!("empty insert executed SQL")
        }
        async fn execute_unprepared(&self, _: &str) -> Result<ExecResult, DbErr> {
            panic!("empty insert executed SQL")
        }
        async fn query_one_raw(&self, _: Statement) -> Result<Option<QueryResult>, DbErr> {
            panic!("empty insert executed SQL")
        }
        async fn query_all_raw(&self, _: Statement) -> Result<Vec<QueryResult>, DbErr> {
            panic!("empty insert executed SQL")
        }
    }
    let empty = || {
        TryInsert::<entry::ActiveModel>::many(Vec::<entry::ActiveModel>::new()).on_conflict(
            OnConflict::column(entry::Column::Id)
                .do_nothing()
                .to_owned(),
        )
    };
    for backend in [DbBackend::SQLite, DbBackend::Postgres, DbBackend::MySQL] {
        let db = NoSql(backend);
        assert!(matches!(
            empty().try_insert().exec(&db).await.unwrap(),
            TryInsertResult::Empty
        ));
        assert!(matches!(
            empty()
                .try_insert()
                .exec_without_returning(&db)
                .await
                .unwrap(),
            TryInsertResult::Empty
        ));
        assert!(matches!(
            empty().try_insert().exec_with_returning(&db).await.unwrap(),
            TryInsertResult::Empty
        ));
        assert!(matches!(
            empty()
                .try_insert()
                .exec_with_returning_keys(&db)
                .await
                .unwrap(),
            TryInsertResult::Empty
        ));
        assert!(matches!(
            empty()
                .try_insert()
                .exec_with_returning_many(&db)
                .await
                .unwrap(),
            TryInsertResult::Empty
        ));
        assert!(matches!(
            empty().on_conflict_do_nothing().exec(&db).await.unwrap(),
            TryInsertResult::Empty
        ));
        assert!(matches!(
            empty()
                .on_conflict_do_nothing_on([entry::Column::Id])
                .exec(&db)
                .await
                .unwrap(),
            TryInsertResult::Empty
        ));
        assert_eq!(empty().exec_without_returning(&db).await.unwrap(), 0);
        assert!(
            empty()
                .exec_with_returning_many(&db)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            empty()
                .exec_with_returning_keys(&db)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            empty().exec(&db).await,
            Err(DbErr::RecordNotInserted)
        ));
        assert!(matches!(
            empty().exec_with_returning(&db).await,
            Err(DbErr::RecordNotInserted)
        ));
    }
}

#[tokio::test]
async fn configured_nonempty_inserts_keep_default_rows_and_conflict_outcomes() {
    use tideorm::orm::{TryInsert, sql_builder::OnConflict};
    let db = database().await;
    let inserted = TryInsert::one(entry::ActiveModel::default())
        .exec_with_returning(&db)
        .await
        .unwrap();
    let TryInsertResult::Inserted(row) = inserted else {
        panic!("one default model is not empty input")
    };
    assert_eq!(row.label, "from database");
    let duplicate = TryInsert::one(entry::ActiveModel {
        id: Set(row.id),
        label: Set("replacement".into()),
    })
    .on_conflict(
        OnConflict::column(entry::Column::Id)
            .do_nothing()
            .to_owned(),
    )
    .try_insert()
    .exec(&db)
    .await
    .unwrap();
    assert!(matches!(duplicate, TryInsertResult::Conflicted));
    assert_eq!(entry::Entity::find().all(&db).await.unwrap(), [row]);
}

#[tideorm::model(table = "facade_default_rows")]
struct FacadeDefaultRow {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
}

#[tokio::test]
async fn facade_default_only_batches_insert_every_model() {
    use tideorm::Model as _;

    let db = tideorm::Database::connect("sqlite::memory:").await.unwrap();
    db.exec_raw("CREATE TABLE facade_default_rows (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .unwrap();
    tideorm::Database::set_global(db).unwrap();
    let result = FacadeDefaultRow::insert_all(vec![
        FacadeDefaultRow::default(),
        FacadeDefaultRow::default(),
    ])
    .await;
    tideorm::Database::reset_global();

    let rows = result.unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].id, rows[1].id);
}

mod entry {
    #[tideorm::model(table = "default_entries")]
    #[derive(PartialEq)]
    pub struct Model {
        #[tideorm(primary_key, auto_increment)]
        pub id: i32,
        pub label: String,
    }

    pub use __tideorm_internal_model::{ActiveModel, Column, Entity};
}

#[tokio::test]
async fn a_default_only_batch_inserts_every_model() {
    let db = database().await;
    let insert = batch();
    let sql = insert.build(db.get_database_backend()).unwrap().sql;
    let result = insert.exec_with_returning(&db).await;
    assert!(result.is_ok(), "{sql}: {result:?}");
    let rows = result.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.label == "from database"));
    assert_eq!(entry::Entity::find().all(&db).await.unwrap().len(), 3);
}

#[tokio::test]
async fn default_batches_keep_affected_counts_keys_and_try_insert_results() {
    let db = database().await;
    assert_eq!(batch().exec_without_returning(&db).await.unwrap(), 3);
    assert_eq!(batch().exec(&db).await.unwrap().last_insert_id, Some(6));
    assert_eq!(
        batch().exec_with_returning_keys(&db).await.unwrap(),
        [7, 8, 9]
    );
    let result = batch()
        .try_insert()
        .exec_without_returning(&db)
        .await
        .unwrap();
    assert!(matches!(result, TryInsertResult::Inserted(3)));
    let result = entry::Entity::insert_many(Vec::<entry::ActiveModel>::new())
        .try_insert()
        .exec(&db)
        .await
        .unwrap();
    assert!(matches!(result, TryInsertResult::Empty));
    assert_eq!(entry::Entity::find().all(&db).await.unwrap().len(), 12);
}

#[tokio::test]
async fn a_failed_default_batch_rolls_back_all_of_its_rows() {
    for returning in [false, true] {
        let db = database().await;
        db.execute_unprepared("CREATE TRIGGER stop_batch BEFORE INSERT ON default_entries WHEN (SELECT COUNT(*) FROM default_entries WHERE label = 'from database') >= 1 BEGIN SELECT RAISE(ABORT, 'second default row'); END").await.unwrap();
        let result = if returning {
            batch().exec_with_returning(&db).await.map(|_| ())
        } else {
            batch().exec_without_returning(&db).await.map(|_| ())
        };
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("second default row")
        );
        assert!(entry::Entity::find().all(&db).await.unwrap().is_empty());

        let transaction = db.begin().await.unwrap();
        entry::Entity::insert(entry::ActiveModel {
            label: Set("outer transaction".into()),
            ..Default::default()
        })
        .exec(&transaction)
        .await
        .unwrap();
        let executor = DatabaseExecutor::from(&transaction);
        let result = if returning {
            batch().exec_with_returning(&executor).await.map(|_| ())
        } else {
            batch().exec_without_returning(&executor).await.map(|_| ())
        };
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("second default row")
        );
        let remaining = entry::Entity::find().all(&transaction).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].label, "outer transaction");
        transaction.commit().await.unwrap();
        assert_eq!(entry::Entity::find().all(&db).await.unwrap().len(), 1);
    }
}

mod generated_key {
    #[tideorm::model(table = "generated_keys")]
    #[derive(PartialEq)]
    pub struct Model {
        #[tideorm(primary_key)]
        pub id: String,
    }

    pub use __tideorm_internal_model::{ActiveModel, Entity};
}

/// A SQLite batch of default rows names the rowid, which a `WITHOUT ROWID`
/// table lacks: SQLite refuses the statement before it writes, and such a
/// table takes its default rows one at a time.
#[tokio::test]
async fn default_batches_without_a_rowid_insert_one_row_at_a_time() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE generated_keys (id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16))))) WITHOUT ROWID").await.unwrap();
    let error = generated_key::Entity::insert_many(
        std::iter::repeat_with(generated_key::ActiveModel::default).take(3),
    )
    .exec_with_returning_keys(&db)
    .await
    .unwrap_err();
    assert!(error.to_string().contains("rowid"), "{error}");
    let mut keys = std::collections::HashSet::new();
    for _ in 0..3 {
        let key = generated_key::Entity::insert(generated_key::ActiveModel::default())
            .exec_with_returning_keys(&db)
            .await
            .unwrap();
        assert!(key.iter().all(|key| key.len() == 32));
        keys.extend(key);
    }
    assert_eq!(keys.len(), 3);
}

#[tokio::test]
async fn explicit_values_replace_the_default_batch_and_sql_rendering_keeps_all_rows() {
    use tideorm::orm::sql_builder::{Expr, Query};
    let db = database().await;
    let mut insert = batch();
    QueryTrait::query(&mut insert)
        .columns([entry::Column::Label])
        .values_panic([Expr::val("explicit")]);
    assert_eq!(insert.exec_without_returning(&db).await.unwrap(), 1);
    let sql = batch().build(DbBackend::SQLite).unwrap();
    assert!(
        sql.sql.contains("(rowid) VALUES (NULL), (NULL), (NULL)"),
        "{}",
        sql.sql
    );
    let mut raw = Query::insert()
        .into_table("default_entries")
        .or_default_values_many(3)
        .to_owned();
    raw.returning(Query::returning().column("id"));
    let mut keys = db
        .query_all(&raw)
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<i32>("id").unwrap())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(keys, [2, 3, 4]);
    assert_eq!(entry::Entity::find().all(&db).await.unwrap().len(), 4);
}

#[tokio::test]
async fn default_batches_on_the_configured_backend() {
    let db = Database::connect(
        std::env::var("ENGINE_PROBE_DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".into()),
    )
    .await
    .unwrap();
    let primary_key = match db.get_database_backend() {
        DbBackend::Postgres => "SERIAL PRIMARY KEY",
        DbBackend::MySQL | DbBackend::MariaDB => "INTEGER PRIMARY KEY AUTO_INCREMENT",
        DbBackend::SQLite => "INTEGER PRIMARY KEY AUTOINCREMENT",
    };
    db.execute_unprepared(&format!("CREATE TABLE default_entries (id {primary_key}, label VARCHAR(64) NOT NULL DEFAULT 'from database')")).await.unwrap();
    assert_eq!(batch().exec_without_returning(&db).await.unwrap(), 3);
    let rows = entry::Entity::find().all(&db).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.label == "from database"));
    if db.get_database_backend().supports_returning() {
        assert_eq!(batch().exec_with_returning(&db).await.unwrap().len(), 3);
    }
    db.execute_unprepared("DROP TABLE default_entries")
        .await
        .unwrap();
}

#[tokio::test]
async fn default_batch_sql_keeps_cte_parameters() {
    use tideorm::orm::sql_builder::{CommonTableExpression, Expr, Query};
    let db = database().await;
    let insert = Query::insert()
        .into_table("default_entries")
        .or_default_values_many(3)
        .with_cte(
            CommonTableExpression::new()
                .table_name("input")
                .query(
                    Query::select()
                        .expr(Expr::val("quoted ' gâteau"))
                        .to_owned(),
                )
                .to_owned(),
        )
        .returning(Query::returning().column("id"))
        .to_owned();
    let statement = db.get_database_backend().build(&insert).unwrap();
    assert_eq!(
        statement.values.as_ref().unwrap().0,
        vec!["quoted ' gâteau".into()]
    );
    assert_eq!(db.query_all_raw(statement).await.unwrap().len(), 3);
    assert_eq!(db.query_all(&insert).await.unwrap().len(), 3);
    assert_eq!(entry::Entity::find().all(&db).await.unwrap().len(), 6);
}

/// A column one model of a batch leaves unset while another sets it takes
/// `DEFAULT` on PostgreSQL and MySQL, never NULL. SQLite has no `DEFAULT`
/// value, so it refuses such a batch before writing anything.
#[tokio::test]
async fn a_column_one_model_leaves_unset_takes_its_default() {
    let mixed = || {
        entry::Entity::insert_many([
            entry::ActiveModel {
                label: Set("explicit".into()),
                ..Default::default()
            },
            entry::ActiveModel::default(),
        ])
    };
    for backend in [DbBackend::Postgres, DbBackend::MySQL] {
        let sql = mixed().build(backend).unwrap().sql;
        assert!(sql.ends_with("), (DEFAULT)"), "{sql}");
    }
    assert!(mixed().build(DbBackend::SQLite).is_err());
    let db = database().await;
    assert!(mixed().exec_without_returning(&db).await.is_err());
    assert!(entry::Entity::find().all(&db).await.unwrap().is_empty());
}

/// A returned key that does not decode reports why, not only that the insert
/// id could not be read.
#[tokio::test]
async fn an_undecodable_returned_key_keeps_its_cause() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE default_entries (id INTEGER, label TEXT NOT NULL)")
        .await
        .unwrap();
    let error = entry::Entity::insert(entry::ActiveModel {
        label: Set("keyless".into()),
        ..Default::default()
    })
    .exec(&db)
    .await
    .unwrap_err();
    assert!(error.to_string().contains("null"), "{error}");
}
