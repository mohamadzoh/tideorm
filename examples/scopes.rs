use tideorm::prelude::*;

#[tideorm::model(table = "example_users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    active: bool,
}

#[tideorm::scopes]
impl User {
    pub fn active(query: QueryBuilder<Self>) -> QueryBuilder<Self> {
        query.where_eq(Self::ACTIVE, true)
    }
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .auto_models()
        .sync(true)
        .connect()
        .await?;
    User {
        id: 0,
        name: "Ada".into(),
        active: true,
    }
    .save()
    .await?;
    let users = User::query().active().order_asc(User::ID).get().await?;
    assert_eq!(users.len(), 1);
    Ok(())
}
