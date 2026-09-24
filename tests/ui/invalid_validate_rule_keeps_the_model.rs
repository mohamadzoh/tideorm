use tideorm::prelude::*;

#[tideorm::model(table = "tasks")]
struct Task {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(range = "1,5")]
    priority: i32,
    title: String,
}

async fn uses_the_model() -> tideorm::Result<()> {
    let task = Task::default().save().await?;
    let _ = Task::query().where_eq("title", task.title.clone()).get().await?;
    Ok(())
}

fn main() {
    let _ = uses_the_model();
}
