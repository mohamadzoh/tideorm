#[tideorm::model(table = "users", hidden = "pasword")]
struct User {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    password: String,
}

fn main() {}
