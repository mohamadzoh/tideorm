use tideorm::prelude::*;

#[tideorm::model(table = "example_users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    active: bool,
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
    let mut user = User {
        id: 0,
        name: "Ada".into(),
        active: true,
    }
    .save()
    .await?;
    let found = User::query()
        .where_eq(User::ACTIVE, true)
        .first_or_fail()
        .await?;
    assert_eq!(found.id, user.id);
    user.name = "Ada Lovelace".into();
    let user = user.update().await?;
    assert_eq!(user.name, "Ada Lovelace");
    user.delete().await?;
    assert_eq!(User::query().count().await?, 0);
    Ok(())
}
