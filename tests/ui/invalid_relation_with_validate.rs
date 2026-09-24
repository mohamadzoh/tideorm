use tideorm::prelude::*;

#[tideorm::model(table = "posts")]
struct Post {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
}

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,

    #[tideorm(has_many = "Post", foreign_key = "user_id")]
    #[validate(required)]
    posts: HasMany<Post>,
}

fn main() {}
