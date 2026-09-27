use tideorm::prelude::*;

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(has_one = "Profile", foreign_key = "user_id")]
    profile: Option<HasOne<Profile>>,
}

#[tideorm::model(table = "profiles")]
struct Profile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
}

fn main() {}
