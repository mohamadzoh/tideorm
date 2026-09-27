#[tideorm::model(table = "secrets", encrypted = "code")]
struct Secret {
    #[tideorm(primary_key)]
    code: String,
    value: String,
}

fn main() {}
