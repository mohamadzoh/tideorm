// DATABASE_URL=postgres://... cargo run --example postgres
// Uses a SELECT only; no schema setup is required.
use tideorm::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")?;
    TideConfig::init().database(&url).connect().await?;
    let rows = Database::raw_json("SELECT 1 AS value").await?;
    println!("{rows:?}");
    Ok(())
}
