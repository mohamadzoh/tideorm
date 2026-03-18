use tideorm::{BelongsTo, Model};

#[derive(Model)]
#[tideorm(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    email: String,
}

#[derive(Model)]
#[tideorm(table = "articles")]
struct Article {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,

    #[tideorm(belongs_to = "User", foreign_key = "user_id", owner_key = "uuid")]
    author: BelongsTo<User>,
}

fn main() {}
