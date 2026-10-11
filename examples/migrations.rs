use tideorm::migration::{Migration, Schema};
use tideorm::prelude::*;

struct CreateNotes;

#[tideorm::async_trait::async_trait]
impl Migration for CreateNotes {
    fn version(&self) -> &str {
        "20261005_001"
    }
    fn name(&self) -> &str {
        "create_notes"
    }
    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("notes", |table| {
                table.id();
                table.string("body");
            })
            .await
    }
    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("notes").await
    }
}

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    TideConfig::init()
        .database("sqlite::memory:")
        .max_connections(1)
        .migration(CreateNotes)
        .run_migrations(true)
        .connect()
        .await?;
    Database::execute("INSERT INTO notes (body) VALUES ('hello')").await?;
    Ok(())
}
