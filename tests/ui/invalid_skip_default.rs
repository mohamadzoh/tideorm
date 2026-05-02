#[tideorm::model(table = "users", skip_default)]
struct User {
    #[tideorm(primary_key)]
    id: i64,
}

fn main() {
    let _: User = Default::default();
}
