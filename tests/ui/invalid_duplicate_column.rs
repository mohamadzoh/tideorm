// Two fields mapped to one column: the insert kept one value and dropped the
// other, and every read failed on the repeated column.
#[tideorm::model(table = "accounts")]
struct Account {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    name: String,
    #[tideorm(column = "name")]
    display_name: String,
}

fn main() {}
