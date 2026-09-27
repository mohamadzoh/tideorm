use tideorm::relations::HasOne;

#[tideorm::model(table = "profiles")]
struct Profile {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    user_id: i64,
}

#[tideorm::model(table = "users")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,

    profile: HasOne<Profile>,
}

fn main() {}
