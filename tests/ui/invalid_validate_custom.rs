#[tideorm::model(table = "accounts")]
struct Account {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[validate(custom = "no_reserved_names")]
    name: String,
}

fn main() {}
