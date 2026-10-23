// `email` is one field's name and the other's column: by declaration order
// it encrypted `email` and stored `email_enc` in plaintext.
#[tideorm::model(table = "accounts", encrypted = "email")]
struct Account {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(column = "email_plain")]
    email: String,
    #[tideorm(column = "email")]
    email_enc: String,
}

fn main() {}
