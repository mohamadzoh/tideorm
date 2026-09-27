// On a field the attribute is bare: it indexes that field's column.
#[tideorm::model(table = "members")]
struct Member {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[unique_index("email", "name")]
    email: String,
    name: String,
}

fn main() {}
