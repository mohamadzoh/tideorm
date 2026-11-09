use tideorm::orm::sql_builder::{Query, ValueTuple};
use tideorm::orm::*;

mod row {
    #[tideorm::model(table = "conversion_rows")]
    #[derive(PartialEq)]
    pub struct Model {
        #[tideorm(primary_key, auto_increment)]
        pub id: i32,
        pub bytes: Vec<u8>,
    }

    pub use __tideorm_internal_model::{ActiveModel, Column, Entity};
}

#[derive(Clone, Debug)]
struct InvalidModel(row::ActiveModel);

impl ActiveModelTrait for InvalidModel {
    type Entity = row::Entity;
    fn take(&mut self, c: row::Column) -> ActiveValue<Value> {
        self.0.take(c)
    }
    fn get(&self, c: row::Column) -> ActiveValue<Value> {
        self.0.get(c)
    }
    fn set_if_not_equals(&mut self, c: row::Column, v: Value) {
        self.0.set_if_not_equals(c, v);
    }
    fn try_set(&mut self, c: row::Column, v: Value) -> Result<(), DbErr> {
        self.0.try_set(c, v)
    }
    fn not_set(&mut self, c: row::Column) {
        self.0.not_set(c);
    }
    fn is_not_set(&self, c: row::Column) -> bool {
        self.0.is_not_set(c)
    }
    fn default() -> Self {
        Self(<row::ActiveModel as ActiveModelTrait>::default())
    }
    fn default_values() -> Self {
        Self(row::ActiveModel {
            id: Set(1),
            bytes: Set(vec![255]),
        })
    }
    fn reset(&mut self, c: row::Column) {
        self.0.reset(c);
    }
    fn get_primary_key_value(&self) -> Option<ValueTuple> {
        Some(ValueTuple::One("wrong variant".into()))
    }
}
impl IntoActiveModel<InvalidModel> for row::Model {
    fn into_active_model(self) -> InvalidModel {
        InvalidModel(<Self as IntoActiveModel<row::ActiveModel>>::into_active_model(self))
    }
}
impl TryIntoModel<row::Model> for InvalidModel {
    fn try_into_model(self) -> Result<row::Model, DbErr> {
        self.0.try_into_model()
    }
}

struct NoExecution;
impl ConnectionTrait for NoExecution {
    fn get_database_backend(&self) -> DbBackend {
        DbBackend::MySQL
    }
    async fn execute_raw(&self, _: Statement) -> Result<ExecResult, DbErr> {
        panic!("invalid key reached execution")
    }
    async fn execute_unprepared(&self, _: &str) -> Result<ExecResult, DbErr> {
        panic!("invalid key reached execution")
    }
    async fn query_one_raw(&self, _: Statement) -> Result<Option<QueryResult>, DbErr> {
        panic!("invalid key reached execution")
    }
    async fn query_all_raw(&self, _: Statement) -> Result<Vec<QueryResult>, DbErr> {
        panic!("invalid key reached execution")
    }
}

#[test]
fn invalid_binary_defaults_return_json_errors() {
    assert!(matches!(
        InvalidModel::from_json(serde_json::json!({})),
        Err(DbErr::Json(_))
    ));
}

#[tokio::test]
async fn invalid_primary_key_tuples_return_errors_before_writes() {
    for key in [
        ValueTuple::One("bad".into()),
        ValueTuple::Two(1.into(), 2.into()),
    ] {
        let insert = Inserter::<row::ActiveModel>::new(
            Some(key),
            Query::insert()
                .into_table("conversion_rows")
                .or_default_values()
                .to_owned(),
        );
        assert!(matches!(
            insert.exec(&NoExecution).await,
            Err(DbErr::UnpackInsertId)
        ));
    }
    let model = InvalidModel(row::ActiveModel {
        id: Unchanged(1),
        bytes: Set(vec![1]),
    });
    assert!(matches!(
        Update::one(model).exec(&NoExecution).await,
        Err(DbErr::UpdateGetPrimaryKey)
    ));
}

#[tokio::test]
async fn invalid_custom_fragments_cannot_execute_a_write() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE guarded (id INTEGER)")
        .await
        .unwrap();
    let invalid =
        sql_builder::Expr::cust_with_values("? + ?", [1], &sql_builder::SqliteQueryBuilder)
            .unwrap_or_else(sql_builder::Expr::Invalid);
    let query = Query::insert()
        .into_table("guarded")
        .columns(["id"])
        .values_panic([invalid])
        .to_owned();
    assert!(db.execute(&query).await.is_err());
    let rows = db
        .query_all(&Query::select().column("id").from("guarded").to_owned())
        .await
        .unwrap();
    assert!(rows.is_empty());
}
