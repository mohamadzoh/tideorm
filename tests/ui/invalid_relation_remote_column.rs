use tideorm::BelongsTo;

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    email: String,
}

#[tideorm::model(table = "articles")]
struct Article {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,

    #[tideorm(belongs_to = "User", foreign_key = "user_id", owner_key = "uuid")]
    author: BelongsTo<User>,
}

fn main() {}
