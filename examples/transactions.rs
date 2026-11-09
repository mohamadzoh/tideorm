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
    require_db()?
        .atomically(async {
            User {
                id: 0,
                name: "Ada".into(),
                active: true,
            }
            .save()
            .await?;
            // Returning Err here rolls back all writes in this future.
            Ok(())
        })
        .await?;
    assert_eq!(User::query().count().await?, 1);
    Ok(())
}
